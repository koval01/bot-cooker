//! Smoke tests of the real binary: start `bot-cooker` as a process with a
//! config file, talk raw HTTP/1.1 to it over TCP, stop it. Runs the same on
//! Linux, macOS and Windows.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

mod common;

use common::{Reply, parse};

const TOKEN: &str = "smoke-admin-token";

struct Server {
    child: Child,
    addr: SocketAddr,
    dir: PathBuf,
}

impl Server {
    /// Start the binary on an OS-assigned port; `tarpit` is the YAML body of
    /// the `tarpit:` section.
    fn start(name: &str, tarpit: &str) -> Server {
        let dir = std::env::temp_dir().join(format!("bot-cooker-smoke-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("bot-cooker.yaml");
        let log = dir.join("bot-cooker.log");
        std::fs::write(
            &config,
            format!(
                "http:\n  bind: \"127.0.0.1:0\"\ntarpit:\n{tarpit}\ndeception:\n  master_key: \"smoke-{name}\"\nstats:\n  admin_token: \"{TOKEN}\"\n  report_interval_secs: 1\n"
            ),
        )
        .unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_bot-cooker"))
            .arg("--config")
            .arg(&config)
            .arg("--log-file")
            .arg(&log)
            .env_remove("COOKER_HTTP_BIND")
            .env_remove("COOKER_MASTER_KEY")
            .env_remove("COOKER_ADMIN_TOKEN")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start bot-cooker");
        let addr = wait_for_listener(&log);
        Server { child, addr, dir }
    }

    fn fast(name: &str) -> Server {
        Server::start(name, "  initial_delay_ms: 0\n  delay_ms: 0\n  abort_probability: 0.0\n  max_bytes: 65536\n  max_seconds: 10")
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.dir.join("bot-cooker.log")).unwrap_or_default()
    }

    fn request(&self, method: &str, path: &str, headers: &[(&str, &str)]) -> Reply {
        let mut s = TcpStream::connect(self.addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        let mut req = format!("{method} {path} HTTP/1.1\r\nHost: smoke.example.net\r\nConnection: close\r\n");
        for (k, v) in headers {
            req += &format!("{k}: {v}\r\n");
        }
        req += "\r\n";
        s.write_all(req.as_bytes()).unwrap();
        let mut raw = Vec::new();
        s.read_to_end(&mut raw).unwrap();
        parse(&raw, method == "HEAD")
    }

    fn get(&self, path: &str) -> Reply {
        self.request("GET", path, &[])
    }

    fn stats(&self) -> serde_json::Value {
        let r = self.request("GET", "/_cooker/stats", &[("X-Admin-Token", TOKEN)]);
        assert_eq!(r.status, 200);
        serde_json::from_slice(&r.body).unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The server logs its bound address; wait for that line.
fn wait_for_listener(log: &PathBuf) -> SocketAddr {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        if let Some(addr) = text.lines().find_map(|l| l.split("listening on http://").nth(1)) {
            return addr.trim().parse().expect("bound address");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("bot-cooker did not start; log:\n{}", std::fs::read_to_string(log).unwrap_or_default());
}

#[test]
fn serves_robots_pages_and_404s() {
    let s = Server::fast("basic");
    let robots = s.get("/robots.txt");
    assert_eq!(robots.status, 200);
    assert!(robots.text().to_ascii_lowercase().contains("user-agent"), "{}", robots.text());
    assert!(robots.header("date").is_some(), "real servers send Date");

    let missing = s.get("/no-such-thing-4b1d/old.zip");
    assert_eq!(missing.status, 404);

    let head = s.request("HEAD", "/robots.txt", &[]);
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
}

#[test]
fn the_site_root_answers() {
    let s = Server::fast("stream");
    let r = s.get("/");
    assert!(r.status < 500, "{}", r.status);
    assert!(!r.body.is_empty());
}

#[test]
fn stats_need_the_token_and_count_requests() {
    let s = Server::fast("stats");
    for _ in 0..3 {
        s.get("/robots.txt");
    }
    let json = s.stats();
    assert!(json["stats"]["requests_total"].as_u64().unwrap() >= 3, "{json}");
    let probe = s.get("/_cooker/stats");
    assert!(!probe.text().contains("requests_total"), "stats served without a token");
}

#[test]
fn slow_requests_hold_their_connections() {
    let s = Server::start("hold", "  initial_delay_ms: 4000\n  delay_ms: 500\n  abort_probability: 0.0\n  max_seconds: 20");
    let held: Vec<TcpStream> = (0..20)
        .map(|_| {
            let mut c = TcpStream::connect(s.addr).unwrap();
            c.write_all(b"GET /wp-login.php HTTP/1.1\r\nHost: smoke.example.net\r\n\r\n").unwrap();
            c
        })
        .collect();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut active = 0;
    while Instant::now() < deadline {
        active = s.stats()["stats"]["active"].as_u64().unwrap();
        if active >= 10 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(active >= 10, "only {active} of 20 slow connections are held");
    drop(held);
}

#[test]
fn the_periodic_report_goes_to_the_log_as_plain_text() {
    let s = Server::fast("report");
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && !s.log().contains("report: active") {
        std::thread::sleep(Duration::from_millis(100));
    }
    let log = s.log();
    assert!(log.contains("report: active"), "{log}");
    assert!(!log.contains('\x1b'), "ANSI escapes in the log file");
}

#[cfg(unix)]
#[test]
fn sigterm_stops_promptly_even_with_connections_held() {
    let mut s = Server::start("term", "  initial_delay_ms: 30000\n  abort_probability: 0.0\n  max_seconds: 120");
    let mut c = TcpStream::connect(s.addr).unwrap();
    c.write_all(b"GET /.env HTTP/1.1\r\nHost: smoke.example.net\r\n\r\n").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    let ok = Command::new("kill").arg("-TERM").arg(s.child.id().to_string()).status().unwrap().success();
    assert!(ok);
    loop {
        if let Some(status) = s.child.try_wait().unwrap() {
            assert!(status.success(), "exit status {status}");
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "still running 10s after SIGTERM");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(s.log().contains("shutdown complete"), "{}", s.log());
}

#[test]
fn check_reports_the_persona_without_listening() {
    let out = Command::new(env!("CARGO_BIN_EXE_bot-cooker"))
        .arg("--check")
        .env("COOKER_MASTER_KEY", "smoke-check")
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("configuration OK") && text.contains("persona:"), "{text}");
}
