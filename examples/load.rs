//! Load test for a trap you run: hold many slow connections the way a scan
//! farm would and report what happened to them. Point it only at your own
//! instance (by default a local one).
//!
//! ```text
//! cargo run --release --example load -- --addr 127.0.0.1:8080 --conns 5000 --secs 60
//! ```
//!
//! Watch the server meanwhile, e.g. `ps -o rss,%cpu -p <pid>`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use clap::Parser;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Parser, Debug)]
#[command(about = "Hold many slow connections against your own bot-cooker instance.")]
struct Cli {
    /// Address of the trap.
    #[arg(long, default_value = "127.0.0.1:8080")]
    addr: SocketAddr,
    /// Host header to send.
    #[arg(long, default_value = "load.example.net")]
    host: String,
    /// Connections to hold at once.
    #[arg(long, default_value_t = 2000)]
    conns: usize,
    /// How long to run, in seconds.
    #[arg(long, default_value_t = 60)]
    secs: u64,
    /// New connections opened per second while ramping up.
    #[arg(long, default_value_t = 500)]
    rate: u64,
}

#[derive(Default)]
struct Counters {
    opened: AtomicU64,
    open_now: AtomicU64,
    first_byte: AtomicU64,
    finished: AtomicU64,
    /// Ended without the closing chunk: the trap cut the body short.
    truncated: AtomicU64,
    reset: AtomicU64,
    connect_failed: AtomicU64,
    bytes: AtomicU64,
    ttfb_ms_total: AtomicU64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let paths = Arc::new(bait_paths(&cli).await?);
    println!("{} paths from robots.txt plus / and misses; holding {} connections for {}s", paths.len() - 2, cli.conns, cli.secs);

    let c = Arc::new(Counters::default());
    let deadline = Instant::now() + Duration::from_secs(cli.secs);
    let gap = Duration::from_micros(1_000_000 / cli.rate.max(1));
    let host = Arc::new(cli.host.clone());

    // Keep `conns` connections open: each worker reconnects when its
    // connection ends, like a scanner moving to the next path.
    let mut workers = Vec::new();
    for w in 0..cli.conns {
        let (c, paths, host) = (c.clone(), paths.clone(), host.clone());
        workers.push(tokio::spawn(async move {
            let mut n = w;
            while Instant::now() < deadline {
                let path = &paths[n % paths.len()];
                one(cli.addr, &host, path, &c, deadline).await;
                n += cli.conns;
            }
        }));
        tokio::time::sleep(gap).await;
    }

    let report = {
        let c = c.clone();
        tokio::spawn(async move {
            let start = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                println!(
                    "{:>4}s  open {:>6}  opened {:>7}  first byte {:>7}  finished {:>6}  truncated {:>5}  reset {:>5}  connect failed {:>5}  {:>9} KiB",
                    start.elapsed().as_secs(),
                    c.open_now.load(Ordering::Relaxed),
                    c.opened.load(Ordering::Relaxed),
                    c.first_byte.load(Ordering::Relaxed),
                    c.finished.load(Ordering::Relaxed),
                    c.truncated.load(Ordering::Relaxed),
                    c.reset.load(Ordering::Relaxed),
                    c.connect_failed.load(Ordering::Relaxed),
                    c.bytes.load(Ordering::Relaxed) / 1024,
                );
            }
        })
    };
    for w in workers {
        let _ = w.await;
    }
    report.abort();
    let fb = c.first_byte.load(Ordering::Relaxed).max(1);
    println!(
        "done: opened {}, finished {}, truncated {}, connect failures {}, resets {}, mean TTFB {} ms, {} KiB read",
        c.opened.load(Ordering::Relaxed),
        c.finished.load(Ordering::Relaxed),
        c.truncated.load(Ordering::Relaxed),
        c.connect_failed.load(Ordering::Relaxed),
        c.reset.load(Ordering::Relaxed),
        c.ttfb_ms_total.load(Ordering::Relaxed) / fb,
        c.bytes.load(Ordering::Relaxed) / 1024
    );
    Ok(())
}

/// One request, read slowly until the server ends it or time is up.
async fn one(addr: SocketAddr, host: &str, path: &str, c: &Counters, deadline: Instant) {
    let Ok(Ok(mut s)) = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(addr)).await else {
        c.connect_failed.fetch_add(1, Ordering::Relaxed);
        tokio::time::sleep(Duration::from_millis(200)).await;
        return;
    };
    c.opened.fetch_add(1, Ordering::Relaxed);
    c.open_now.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();
    let req = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: load-test\r\nAccept: */*\r\nConnection: close\r\n\r\n");
    let mut buf = [0u8; 512];
    let mut first = true;
    // Enough of the response to see its framing: the head, then the last bytes.
    let mut head = Vec::new();
    let mut last = Vec::new();
    if s.write_all(req.as_bytes()).await.is_ok() {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match tokio::time::timeout(left, s.read(&mut buf)).await {
                Ok(Ok(0)) => {
                    let text = String::from_utf8_lossy(&head).to_ascii_lowercase();
                    let chunked = text.contains("transfer-encoding: chunked");
                    if chunked && !last.ends_with(b"0\r\n\r\n") {
                        c.truncated.fetch_add(1, Ordering::Relaxed);
                    } else {
                        c.finished.fetch_add(1, Ordering::Relaxed);
                    }
                    break;
                }
                Ok(Ok(n)) => {
                    if head.len() < 2048 {
                        head.extend_from_slice(&buf[..n]);
                    }
                    last.extend_from_slice(&buf[..n]);
                    let keep = last.len().saturating_sub(8);
                    last.drain(..keep);
                    if first {
                        first = false;
                        c.first_byte.fetch_add(1, Ordering::Relaxed);
                        c.ttfb_ms_total.fetch_add(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                    }
                    c.bytes.fetch_add(n as u64, Ordering::Relaxed);
                    // A slow reader: small reads with a pause between them.
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Ok(Err(_)) => {
                    c.reset.fetch_add(1, Ordering::Relaxed);
                    break;
                }
                Err(_) => break,
            }
        }
    }
    c.open_now.fetch_sub(1, Ordering::Relaxed);
}

/// The trap's own bait (robots.txt), the site root and two misses.
async fn bait_paths(cli: &Cli) -> anyhow::Result<Vec<String>> {
    let mut s = TcpStream::connect(cli.addr).await?;
    s.write_all(format!("GET /robots.txt HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", cli.host).as_bytes()).await?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).await?;
    let text = String::from_utf8_lossy(&raw);
    let mut paths: Vec<String> = text
        .lines()
        .filter_map(|l| l.strip_prefix("Disallow:").or_else(|| l.strip_prefix("Allow:")))
        .map(|p| p.trim().replace('*', "x"))
        .filter(|p| p.starts_with('/') && p.len() > 1)
        .collect();
    paths.sort();
    paths.dedup();
    paths.extend(["/".to_string(), "/no-such-page.php".to_string(), "/.git/HEAD".to_string()]);
    Ok(paths)
}
