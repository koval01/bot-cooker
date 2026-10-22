//! End-to-end tests over real TCP against an in-process server: what a
//! scanner on the wire sees. Delays are zeroed so the tarpit answers at once;
//! everything else is the production path (wire format, routing, rendering,
//! streaming).

mod common;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use bot_cooker::catalog::Catalog;
use bot_cooker::config::{Config, HostOsSetting, Profile, TarpitConfig};
use bot_cooker::data::Data;
use bot_cooker::engine::Engine;
use bot_cooker::http::{AppState, serve};
use bot_cooker::persona::Frontend;
use bot_cooker::stats::Stats;
use common::{Reply, parse, request};

const TOKEN: &str = "integration-admin-token";
const MAX_BYTES: u64 = 48 * 1024;
const HOST: &str = "shop.example.net";

struct Trap {
    addr: SocketAddr,
    engine: Arc<Engine>,
}

fn config(key: &str, templates_dir: Option<PathBuf>) -> Config {
    let mut cfg = Config::default();
    cfg.deception.master_key = key.to_string();
    cfg.deception.templates_dir = templates_dir;
    cfg.deception.host_os = HostOsSetting::Unix;
    cfg.stats.admin_token = TOKEN.to_string();
    cfg.tarpit = TarpitConfig {
        initial_delay_ms: 0,
        delay_ms: 0,
        abort_probability: 0.0,
        max_bytes: MAX_BYTES,
        max_seconds: 10,
        ..Default::default()
    };
    cfg
}

async fn trap_with(cfg: Config) -> Trap {
    let catalog = Arc::new(Catalog::load(cfg.deception.templates_dir.as_deref()).expect("catalog"));
    let data = Data::load(&catalog).expect("data");
    let engine = Arc::new(Engine::new(&cfg.deception, catalog, data, cfg.routes.clone()));
    let app = AppState::new(cfg, engine.clone(), Stats::new(1_000), TOKEN.to_string());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(serve(listener, app, std::future::pending()));
    Trap { addr, engine }
}

async fn trap(key: &str) -> Trap {
    trap_with(config(key, None)).await
}

impl Trap {
    /// Send raw bytes, read until the server closes.
    async fn raw(&self, bytes: &[u8]) -> Vec<u8> {
        let mut s = TcpStream::connect(self.addr).await.unwrap();
        // A server may answer and close before a huge request is sent.
        let _ = s.write_all(bytes).await;
        let mut out = Vec::new();
        tokio::time::timeout(Duration::from_secs(30), s.read_to_end(&mut out)).await.expect("server answered in time").ok();
        out
    }

    async fn send(&self, method: &str, path: &str, host: &str, extra: &[(&str, &str)]) -> Reply {
        parse(&self.raw(&request(method, path, host, extra)).await, method == "HEAD")
    }

    async fn get(&self, path: &str) -> Reply {
        self.send("GET", path, HOST, &[]).await
    }

    fn frontend(&self) -> Frontend {
        self.engine.current().persona.frontend
    }

    /// The first concrete path the persona serves with `profile`.
    fn path_for(&self, profile: Profile) -> Option<String> {
        let inc = self.engine.current();
        inc.persona.routes().into_iter().find(|r| r.profile == profile && !r.pattern.contains(['*', '?', '['])).map(|r| r.pattern)
    }
}

/// The largest of the first blocks of the stream behind `path`.
fn largest_block(t: &Trap, path: &str) -> usize {
    let inc = t.engine.current();
    let info = bot_cooker::engine::RequestInfo { method: &http::Method::GET, path, query: None, host: Some(HOST), https: false };
    let art = t.engine.render(&inc, inc.router.profile_for(path), &info);
    let c = art.stream.expect("a streamed log");
    (0..c.blocks.min(64)).map(|seq| t.engine.continue_stream(&inc.persona, &c, seq).len()).max().unwrap_or(0)
}

fn keys() -> impl Iterator<Item = String> {
    (0..6).map(|i| format!("integration-key-{i}"))
}

/// Keys whose personas cover every front server (with a Unix host, IIS
/// excepted; a Windows host covers it).
async fn one_per_frontend() -> Vec<Trap> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for i in 0..400 {
        let mut cfg = config(&format!("wire-{i}"), None);
        cfg.deception.host_os = if i % 2 == 0 { HostOsSetting::Unix } else { HostOsSetting::Windows };
        let t = trap_with(cfg).await;
        if seen.insert(t.frontend()) {
            out.push(t);
        }
        if seen.len() == Frontend::ALL.len() {
            break;
        }
    }
    assert_eq!(seen.len(), Frontend::ALL.len(), "missing front servers: {seen:?}");
    out
}

#[tokio::test]
async fn robots_is_served_fast_and_names_bait_paths() {
    for key in keys() {
        let t = trap(&key).await;
        let r = t.get("/robots.txt").await;
        assert_eq!(r.status, 200, "{key}");
        assert!(r.header("content-type").is_some_and(|c| c.starts_with("text/plain")), "{key}: {:?}", r.headers);
        assert!(r.text().to_ascii_lowercase().contains("user-agent"), "{key}: {}", r.text());
    }
}

#[tokio::test]
async fn a_leaked_config_is_served_with_the_stack_headers() {
    let mut checked = 0;
    for key in keys() {
        let t = trap(&key).await;
        let Some(path) = t.path_for(Profile::Env) else { continue };
        let r = t.get(&path).await;
        assert_eq!(r.status, 200, "{key} {path}");
        assert!(r.body.len() > 40, "{key} {path}: tiny body");
        let inc = t.engine.current();
        assert_eq!(r.header("server"), inc.persona.web.server_banner.as_deref(), "{key}: banner differs from the persona");
        checked += 1;
    }
    assert!(checked > 0, "no persona leaks a config");
}

#[tokio::test]
async fn unknown_paths_get_the_persona_404_not_a_labyrinth() {
    for key in keys() {
        let r = trap(&key).await.get("/definitely-not-here-7f3a/x.bak").await;
        assert_eq!(r.status, 404, "{key}");
    }
}

#[tokio::test]
async fn head_mirrors_get_without_a_body() {
    for key in keys() {
        let t = trap(&key).await;
        let get = t.get("/robots.txt").await;
        let head = t.send("HEAD", "/robots.txt", HOST, &[]).await;
        assert_eq!(head.status, get.status, "{key}");
        assert!(head.body.is_empty(), "{key}: HEAD carried a body");
        assert_eq!(head.header("content-type"), get.header("content-type"), "{key}");
    }
}

#[tokio::test]
async fn odd_methods_get_real_server_answers() {
    for key in keys() {
        let t = trap(&key).await;
        let options = t.send("OPTIONS", "/", HOST, &[]).await;
        assert!(options.status < 500, "{key}: OPTIONS {}", options.status);
        let odd = t.send("PROPFIND", "/", HOST, &[]).await;
        assert!((400..500).contains(&odd.status) || odd.status == 501, "{key}: PROPFIND {}", odd.status);
    }
}

#[tokio::test]
async fn links_follow_the_requested_host() {
    let mut checked = 0;
    for key in keys() {
        let t = trap(&key).await;
        let Some(path) = t.path_for(Profile::Sitemap) else { continue };
        let r = t.send("GET", &path, "mirror.example.org", &[("x-forwarded-proto", "https")]).await;
        assert_eq!(r.status, 200, "{key} {path}");
        assert!(r.text().contains("https://mirror.example.org/"), "{key} {path}: links ignore Host");
        checked += 1;
    }
    assert!(checked > 0, "no persona exposes a sitemap");
}

#[tokio::test]
async fn endless_streams_stop_at_the_byte_cap() {
    let mut checked = 0;
    for key in keys() {
        let t = trap(&key).await;
        let Some(path) = t.path_for(Profile::Log) else { continue };
        let r = t.get(&path).await;
        assert_eq!(r.status, 200, "{key} {path}");
        // Past the cap only the block in hand is finished, so the file still
        // ends on a whole entry and overshoots by less than one block.
        let body = r.text();
        let len = body.len();
        assert!(len as u64 > MAX_BYTES / 2, "{key} {path}: only {len} bytes");
        assert!(body.ends_with('\n'), "{key} {path}: cut mid-line");
        let largest = largest_block(&t, &path);
        assert!(len as u64 <= MAX_BYTES + largest as u64, "{key} {path}: {len} bytes, more than one block ({largest}) past the cap");
        assert!(r.header("content-length").is_none(), "{key}: an endless body claims a length");
        checked += 1;
    }
    assert!(checked > 0, "no persona exposes a log");
}

/// Sizes of the HTTP chunks of a chunked body, as written on the wire.
fn chunk_sizes(raw: &[u8]) -> Vec<usize> {
    let mut data = &raw[raw.windows(4).position(|w| w == b"\r\n\r\n").expect("head") + 4..];
    let mut sizes = Vec::new();
    while let Some(eol) = data.windows(2).position(|w| w == b"\r\n") {
        let Ok(size) = usize::from_str_radix(std::str::from_utf8(&data[..eol]).unwrap_or("").trim(), 16) else { break };
        if size == 0 || data.len() < eol + 2 + size {
            break;
        }
        sizes.push(size);
        data = data.get(eol + 2 + size + 2..).unwrap_or_default();
    }
    sizes
}

/// Behind a local proxy (these tests connect from 127.0.0.1, as the Caddy
/// sidecar does) every piece goes out as its own chunk: the proxy re-frames
/// the body anyway and would otherwise hold a large chunk back until all of
/// it had trickled in.
#[tokio::test]
async fn behind_a_proxy_every_piece_is_its_own_chunk() {
    let mut checked = 0;
    for key in keys() {
        let t = trap(&key).await;
        let Some(path) = t.path_for(Profile::Log) else { continue };
        // A share of requests is answered fast, as from a cache, and goes out
        // in whole blocks; a broken trickle would do so on every request.
        let mut medians = Vec::new();
        for _ in 0..5 {
            let sizes = chunk_sizes(&t.raw(&request("GET", &path, HOST, &[])).await);
            if sizes.len() < 4 {
                continue;
            }
            let mut sorted = sizes;
            sorted.sort_unstable();
            medians.push(sorted[sorted.len() / 2]);
        }
        if medians.is_empty() {
            continue;
        }
        assert!(medians.iter().any(|&m| m <= 8 * 48), "{key} {path}: chunks of {medians:?} bytes typically behind a proxy");
        checked += 1;
    }
    assert!(checked > 0, "no chunked log among the personas");
}

#[tokio::test]
async fn instances_look_different_on_the_wire() {
    let mut fronts = std::collections::BTreeSet::new();
    let mut robots = std::collections::BTreeSet::new();
    for key in keys() {
        let t = trap(&key).await;
        let r = t.get("/robots.txt").await;
        fronts.insert(format!("{:?}", r.header("server")));
        robots.insert(r.text());
    }
    assert!(fronts.len() >= 3, "server banners too uniform: {fronts:?}");
    assert_eq!(robots.len(), 6, "two instances share a robots.txt");
}

#[tokio::test]
async fn one_instance_is_stable_across_requests() {
    let t = trap("integration-stable").await;
    let a = t.get("/robots.txt").await;
    let b = t.send("GET", "/robots.txt", HOST, &[("user-agent", "curl/8.9.1")]).await;
    assert_eq!(a.body, b.body);
}

#[tokio::test]
async fn stats_need_the_token_and_look_like_a_probe_without_it() {
    let t = trap("integration-admin").await;
    let path = Config::default().stats.admin_path;
    let ok = t.send("GET", &path, HOST, &[("x-admin-token", TOKEN)]).await;
    assert_eq!(ok.status, 200);
    let json: serde_json::Value = serde_json::from_slice(&ok.body).unwrap();
    assert!(json.get("stats").is_some());
    let probe = t.get(&path).await;
    assert!(!probe.text().contains("top_ips"), "stats leaked without a token");
    let wrong = t.send("GET", &format!("{path}?token=nope"), HOST, &[]).await;
    assert!(!wrong.text().contains("top_ips"));
}

#[tokio::test]
async fn a_windows_host_looks_like_windows() {
    for key in keys() {
        let mut cfg = config(&key, None);
        cfg.deception.host_os = HostOsSetting::Windows;
        let t = trap_with(cfg).await;
        let r = t.get("/robots.txt").await;
        let server = r.header("server");
        assert!(server.is_none_or(|s| s.starts_with("Microsoft-IIS/")), "{key}: {server:?} on a Windows host");
    }
}

#[tokio::test]
async fn garbage_gets_the_front_servers_own_400() {
    for t in one_per_frontend().await {
        let raw = t.raw(b"\x16\x03\x01\x00\xa5\x01\x00\x00\xa1\x03\x03 garbage\r\n\r\n").await;
        let r = parse(&raw, false);
        let fe = t.frontend();
        assert_eq!(r.status, 400, "{fe:?}: {}", r.head);
        assert!(r.head.starts_with("HTTP/1.1 400 Bad Request"), "{fe:?}: {}", r.head);
        assert_eq!(r.header("server"), t.engine.current().persona.web.server_banner.as_deref(), "{fe:?}: {}", r.head);
        assert!(!r.names().contains(&"content-length") || r.names().contains(&"date"), "{fe:?}: hyper-style head: {}", r.head);
    }
}

#[tokio::test]
async fn header_spelling_and_status_line_follow_the_front_server() {
    for t in one_per_frontend().await {
        let fe = t.frontend();
        let r = t.get("/robots.txt").await;
        let names = r.names();
        match fe {
            Frontend::LiteSpeed => assert!(names.contains(&"content-type"), "{fe:?}: {names:?}"),
            _ => assert!(names.contains(&"Content-Type") && names.contains(&"Date"), "{fe:?}: {names:?}"),
        }
        let raw = t.raw(b"GET /robots.txt HTTP/1.0\r\n\r\n").await;
        let line = String::from_utf8_lossy(&raw).lines().next().unwrap_or_default().to_string();
        let want = if fe == Frontend::Caddy { "HTTP/1.0 200" } else { "HTTP/1.1 200" };
        assert!(line.starts_with(want), "{fe:?}: {line}");
    }
}

#[tokio::test]
async fn the_server_refuses_what_real_servers_refuse() {
    let t = trap("integration-refuse").await;
    let no_host = parse(&t.raw(b"GET / HTTP/1.1\r\n\r\n").await, false);
    assert_eq!(no_host.status, 400, "{}", no_host.head);
    let climb = t.send("GET", "/../../etc/passwd", HOST, &[]).await;
    if t.frontend() != Frontend::Caddy {
        assert_eq!(climb.status, 400, "{}", climb.head);
    }
    let v2 = parse(&t.raw(b"GET / HTTP/2.0\r\nHost: x\r\n\r\n").await, false);
    assert_eq!(v2.status, 505, "{}", v2.head);
    let long = format!("GET /{} HTTP/1.1\r\nHost: x\r\n\r\n", "a".repeat(2 * 1024 * 1024));
    let r = parse(&t.raw(long.as_bytes()).await, false);
    assert!(r.status == 414 || r.status == 431, "{}", r.head);
}

#[tokio::test]
async fn keep_alive_serves_several_requests_on_one_connection() {
    let t = trap("integration-keepalive").await;
    let two = b"GET /robots.txt HTTP/1.1\r\nHost: x\r\n\r\nGET /robots.txt HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n";
    let raw = t.raw(two).await;
    let text = String::from_utf8_lossy(&raw);
    assert_eq!(text.matches("HTTP/1.1 200").count(), 2, "{text}");
}

#[tokio::test]
async fn a_request_body_is_read_and_the_connection_reused() {
    let t = trap("integration-body").await;
    let req = b"POST /robots.txt HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nhelloGET /robots.txt HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n";
    let text = String::from_utf8_lossy(&t.raw(req).await).into_owned();
    assert_eq!(text.matches("HTTP/1.").count(), 2, "{text}");
}

/// Overwrite every stack's robots template from a `templates_dir` overlay.
fn robots_overlay(dir: &Path) {
    let stacks = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/stacks");
    for entry in std::fs::read_dir(stacks).unwrap() {
        let stack = entry.unwrap().file_name();
        let target = dir.join("stacks").join(&stack);
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("robots.txt.j2"), "User-agent: *\nDisallow: /from-the-overlay/\n").unwrap();
    }
}

#[tokio::test]
async fn templates_dir_overrides_the_embedded_catalog() {
    let dir = std::env::temp_dir().join(format!("bot-cooker-overlay-{}", std::process::id()));
    robots_overlay(&dir);
    let t = trap_with(config("integration-overlay", Some(dir.clone()))).await;
    let r = t.get("/robots.txt").await;
    std::fs::remove_dir_all(&dir).ok();
    assert!(r.text().contains("/from-the-overlay/"), "overlay ignored: {}", r.text());
}
