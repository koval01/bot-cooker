//! Randomised requests against the real HTTP stack: every byte a client can
//! send goes through the parser, the protocol checks, routing, rendering and
//! the wire writer. Nothing may panic, every answer must be HTTP, and the
//! server must keep serving.
//!
//! Deterministic by default; `FUZZ_SEED=<n>` picks another corpus and
//! `FUZZ_CASES=<n>` sets how many requests each instance gets. The libFuzzer
//! targets under `fuzz/` go deeper on the parser with a nightly toolchain.

mod common;

use std::panic;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rand::seq::IndexedRandom;
use rand::{Rng, RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;

use bot_cooker::catalog::Catalog;
use bot_cooker::config::{Config, HostOsSetting, TarpitConfig};
use bot_cooker::data::Data;
use bot_cooker::engine::Engine;
use bot_cooker::http::{AppState, serve};
use bot_cooker::stats::Stats;

static PANICS: AtomicUsize = AtomicUsize::new(0);

/// Requests in flight at once: enough to overlap, few enough for the test
/// machine's own socket limits.
const PARALLEL: usize = 64;

const METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "DELETE", "OPTIONS", "TRACE", "CONNECT", "PATCH", "PROPFIND", "get", "G\x01T", ""];
const VERSIONS: &[&str] = &["HTTP/1.1", "HTTP/1.0", "HTTP/2.0", "HTTP/0.9", "HTTP/1.1 ", "http/1.1", "HTTP/1", "HTTPS/1.1", ""];
const TARGETS: &[&str] = &[
    "/",
    "*",
    "/.env",
    "/.git/HEAD",
    "/wp-login.php",
    "/../../etc/passwd",
    "/%2e%2e/%2e%2e/etc/passwd",
    "/..%2f..%2fetc",
    "/a/./b/../../../c",
    "/%00",
    "/%",
    "/%zz",
    "/ü/ñ/日本",
    "/@fs/../.env?raw??",
    "/index.php?s=1&page=999999999999999999999",
    "/api/v1/users?page=-1&per_page=0",
    "http://example.com/x?y",
    "https://[::1]:99999/",
    "//double//slash",
    "/a\\b\\..\\c",
    "/robots.txt?",
    "/sitemap.xml",
    "/favicon.ico",
    "/_cooker/stats?token=",
];
const HEADER_NAMES: &[&str] = &[
    "Host",
    "Content-Length",
    "Transfer-Encoding",
    "Connection",
    "X-Forwarded-For",
    "X-Forwarded-Host",
    "X-Forwarded-Proto",
    "X-Real-IP",
    "User-Agent",
    "Accept",
    "Range",
    "If-None-Match",
    "Cookie",
    "X-Admin-Token",
    "Next-Action",
    "Expect",
];
const HEADER_VALUES: &[&str] = &[
    "",
    "0",
    "-1",
    "18446744073709551616",
    "5, 6",
    "chunked",
    "gzip, chunked",
    "close",
    "keep-alive",
    "100-continue",
    "bytes=0-0,5-",
    "evil\"<script>",
    "203.0.113.1, 10.0.0.1",
    "a.example.com, b",
    "https",
    "\u{7f}",
    "x",
];

fn pick<'a>(rng: &mut impl Rng, items: &[&'a str]) -> &'a str {
    items.choose(rng).copied().unwrap_or("")
}

fn random_bytes(rng: &mut impl Rng, max: usize) -> Vec<u8> {
    let n = rng.random_range(0..max);
    (0..n).map(|_| rng.random()).collect()
}

/// A request assembled from plausible and hostile parts.
fn assembled(rng: &mut impl Rng) -> Vec<u8> {
    let mut target = pick(rng, TARGETS).to_string();
    match rng.random_range(0..6) {
        0 => target.push_str(&"A".repeat(rng.random_range(1..12_000))),
        1 => target.push_str(&String::from_utf8_lossy(&random_bytes(rng, 40))),
        _ => {}
    }
    let mut out = format!("{} {} {}\r\n", pick(rng, METHODS), target, pick(rng, VERSIONS)).into_bytes();
    for _ in 0..rng.random_range(0..12) {
        let name = if rng.random_bool(0.1) {
            String::from_utf8_lossy(&random_bytes(rng, 12)).into_owned()
        } else {
            pick(rng, HEADER_NAMES).to_string()
        };
        out.extend(format!("{name}: {}\r\n", pick(rng, HEADER_VALUES)).into_bytes());
    }
    if rng.random_bool(0.1) {
        for _ in 0..rng.random_range(100..400) {
            out.extend(b"X-Pad: y\r\n");
        }
    }
    if rng.random_bool(0.9) {
        out.extend(b"\r\n");
    }
    out.extend(random_bytes(rng, 300));
    out
}

/// A well-formed request with a few bytes flipped, dropped or duplicated.
fn mutated(rng: &mut impl Rng) -> Vec<u8> {
    let mut out = format!("GET {} HTTP/1.1\r\nHost: shop.example.net\r\nUser-Agent: fuzz\r\n\r\n", pick(rng, TARGETS)).into_bytes();
    for _ in 0..rng.random_range(1..6) {
        if out.is_empty() {
            break;
        }
        let at = rng.random_range(0..out.len());
        match rng.random_range(0..4) {
            0 => out[at] = rng.random(),
            1 => {
                out.remove(at);
            }
            2 => out.insert(at, rng.random()),
            _ => out.truncate(at),
        }
    }
    out
}

fn case(rng: &mut impl Rng) -> Vec<u8> {
    match rng.random_range(0..10) {
        0 => random_bytes(rng, 2_000),
        1..=4 => mutated(rng),
        _ => assembled(rng),
    }
}

async fn trap(key: &str) -> std::net::SocketAddr {
    let mut cfg = Config::default();
    cfg.deception.master_key = key.to_string();
    cfg.deception.host_os = HostOsSetting::Unix;
    cfg.tarpit = TarpitConfig { initial_delay_ms: 0, delay_ms: 0, max_bytes: 32 * 1024, max_seconds: 5, ..Default::default() };
    let catalog = Arc::new(Catalog::load(None).expect("catalog"));
    let data = Data::load(&catalog).expect("data");
    let engine = Arc::new(Engine::new(&cfg.deception, catalog, data, None));
    let app = AppState::new(cfg, engine, Stats::new(1_000), "fuzz-token".into());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(serve(listener, app, std::future::pending()));
    addr
}

/// Send `bytes`, close our side, and return what came back.
async fn exchange(addr: std::net::SocketAddr, bytes: Vec<u8>) -> Vec<u8> {
    // Long runs can use up the client's ephemeral ports (TIME_WAIT); wait
    // for some to free up rather than failing the server's test.
    let mut tries = 0;
    let mut s = loop {
        match TcpStream::connect(addr).await {
            Ok(s) => break s,
            Err(e) if e.kind() == std::io::ErrorKind::AddrNotAvailable && tries < 600 => {
                tries += 1;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(e) => panic!("connect: {e}"),
        }
    };
    let _ = s.write_all(&bytes).await;
    let _ = s.shutdown().await;
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(15), s.read_to_end(&mut out)).await;
    out
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hostile_requests_never_break_the_server() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        PANICS.fetch_add(1, Ordering::SeqCst);
        default_hook(info);
    }));

    let seed = env_u64("FUZZ_SEED", 0x00b0_7c00);
    let cases = env_u64("FUZZ_CASES", 300);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut answered = 0;
    // Different keys give different front servers and stacks.
    for key in ["fuzz-a", "fuzz-b", "fuzz-c", "fuzz-d", "fuzz-e", "fuzz-f"] {
        let addr = trap(key).await;
        let slots = Arc::new(Semaphore::new(PARALLEL));
        let mut batch = Vec::new();
        for _ in 0..cases {
            let bytes = case(&mut rng);
            let slot = slots.clone().acquire_owned().await.unwrap();
            batch.push(tokio::spawn(async move {
                let reply = exchange(addr, bytes.clone()).await;
                drop(slot);
                (bytes, reply)
            }));
        }
        for task in batch {
            let (sent, reply) = task.await.expect("client task");
            if !reply.is_empty() {
                answered += 1;
                assert!(
                    reply.starts_with(b"HTTP/1."),
                    "{key}: not an HTTP answer to {:?}: {:?}",
                    String::from_utf8_lossy(&sent),
                    String::from_utf8_lossy(&reply[..reply.len().min(200)])
                );
            }
        }
        // Still alive and serving normally.
        let ok = exchange(addr, b"GET /robots.txt HTTP/1.1\r\nHost: shop.example.net\r\nConnection: close\r\n\r\n".to_vec()).await;
        assert!(ok.starts_with(b"HTTP/1.1 200"), "{key}: server stopped serving after the corpus");
    }
    assert_eq!(PANICS.load(Ordering::SeqCst), 0, "a request made the server panic (seed {seed})");
    assert!(answered > cases as usize, "only {answered} answers");
}
