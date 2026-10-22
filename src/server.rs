//! Process-level wiring shared by the console binary and the Windows service:
//! load everything from a config, bind, serve until told to stop.

use std::future::Future;
use std::io::IsTerminal;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::catalog::Catalog;
use crate::config::Config;
use crate::data::Data;
use crate::engine::Engine;
use crate::http;
use crate::stats::{self, Stats};

/// Log to stderr, or append to `file` (services have no console).
pub fn init_logging(file: Option<&Path>) -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter).with_target(false);
    match file {
        Some(path) => {
            let f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .with_context(|| format!("opening log file {}", path.display()))?;
            builder.with_ansi(false).with_writer(std::sync::Mutex::new(f)).init();
        }
        None => builder.with_writer(std::io::stderr).with_ansi(std::io::stderr().is_terminal()).init(),
    }
    Ok(())
}

/// What `--check` prints: the config is valid and what the instance looks like.
pub fn check(cfg: &Config) -> anyhow::Result<String> {
    let catalog = Arc::new(Catalog::load(cfg.deception.templates_dir.as_deref()).context("loading template catalog")?);
    let data = Data::load(&catalog).context("loading persona data")?;
    let engine = Engine::new(&cfg.deception, catalog.clone(), data, cfg.routes.clone());
    let inc = engine.current();
    Ok(format!(
        "configuration OK\n  http:  enabled={} bind={}\n  templates: {} compiled\n  persona: {}\n  routes: {} rules ({})",
        cfg.http.enabled,
        cfg.http.bind,
        catalog.template_names().len(),
        inc.persona.summary(),
        inc.router.len(),
        if cfg.routes.is_some() { "from config" } else { "from persona" }
    ))
}

/// Serve until `shutdown` resolves, then drain open connections.
pub async fn run(mut cfg: Config, shutdown: impl Future<Output = ()> + Send + 'static) -> anyhow::Result<()> {
    cfg.limits.max_concurrent = connection_budget(cfg.limits.max_concurrent);
    let catalog = Arc::new(Catalog::load(cfg.deception.templates_dir.as_deref()).context("loading template catalog")?);
    let data = Data::load(&catalog).context("loading persona data")?;
    let engine = Arc::new(Engine::new(&cfg.deception, catalog, data, cfg.routes.clone()));
    let stats = Stats::new(cfg.stats.max_tracked_ips);

    let admin_token = if cfg.stats.admin_token.is_empty() {
        let t = random_token();
        warn!("no admin_token set — generated one for this run: {t}");
        t
    } else {
        cfg.stats.admin_token.clone()
    };

    stats::spawn_reporter(stats.clone(), Duration::from_secs(cfg.stats.report_interval_secs));
    announce(&cfg, &engine, &admin_token);

    // TLS is terminated at a reverse proxy (Caddy, Nginx, IIS ARR, Cloudflare).
    if !cfg.http.enabled {
        anyhow::bail!("http.enabled is false — nothing to serve");
    }

    let app = http::AppState::new(cfg.clone(), engine, stats, admin_token);
    // Bind in-line so port/permission errors surface immediately.
    let listener =
        tokio::net::TcpListener::bind(cfg.http.bind).await.with_context(|| format!("binding HTTP listener on {}", cfg.http.bind))?;
    info!("HTTP tarpit listening on http://{}", listener.local_addr()?);
    http::serve(listener, app, shutdown).await;
    info!("shutdown complete");
    Ok(())
}

/// Raise the open-files limit as far as the hard limit allows and keep the
/// connection cap below it: past the limit `accept` fails and the trap would
/// stop answering altogether.
fn connection_budget(wanted: usize) -> usize {
    #[cfg(unix)]
    {
        // The listener, the log file, stdio and some headroom.
        const RESERVED: u64 = 64;
        let need = wanted as u64 + RESERVED;
        match rlimit::increase_nofile_limit(need) {
            Ok(limit) if limit < need => {
                let cap = limit.saturating_sub(RESERVED).max(1) as usize;
                warn!("open-files limit is {limit}: holding at most {cap} connections (raise LimitNOFILE / ulimit -n for {wanted})");
                cap
            }
            Ok(_) => wanted,
            Err(e) => {
                warn!("could not read the open-files limit ({e}); keeping max_concurrent at {wanted}");
                wanted
            }
        }
    }
    #[cfg(not(unix))]
    {
        wanted
    }
}

/// Resolve on Ctrl-C, or what the platform sends a console program to stop:
/// SIGTERM on Unix; Ctrl-Break, console close, logoff and shutdown on Windows.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let platform = async {
        if let Ok(mut sig) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            sig.recv().await;
        }
    };
    #[cfg(windows)]
    let platform = async {
        use tokio::signal::windows;
        match (windows::ctrl_break(), windows::ctrl_close(), windows::ctrl_shutdown(), windows::ctrl_logoff()) {
            (Ok(mut a), Ok(mut b), Ok(mut c), Ok(mut d)) => {
                tokio::select! {
                    _ = a.recv() => {},
                    _ = b.recv() => {},
                    _ = c.recv() => {},
                    _ = d.recv() => {},
                }
            }
            _ => std::future::pending().await,
        }
    };
    #[cfg(not(any(unix, windows)))]
    let platform = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = platform => {},
    }
    info!("shutdown requested");
}

fn random_token() -> String {
    use rand::RngExt;
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::rng();
    (0..32).map(|_| A[rng.random_range(0..A.len())] as char).collect()
}

/// The startup banner on a terminal, one log line otherwise.
fn announce(cfg: &Config, engine: &Engine, admin_token: &str) {
    let inc = engine.current();
    let t = &cfg.tarpit;
    let token = &admin_token[..admin_token.len().min(6)];
    if !std::io::stdout().is_terminal() {
        info!(
            "persona: {} | routes: {} | stats: GET {} (X-Admin-Token: {token}…)",
            inc.persona.summary(),
            inc.router.len(),
            cfg.stats.admin_path
        );
        return;
    }
    println!(
        "\n\x1b[1m\x1b[33m  bot-cooker\x1b[0m — slow-cooking scanners since boot\n\
         \x1b[2m  persona: {}\x1b[0m\n\
         \x1b[2m  routes:{} | drip:{}B/{}ms | ttfb:{}ms | abort:{:.0}% | epoch:{}h/{}h\x1b[0m\n\
         \x1b[2m  stats: GET {} (X-Admin-Token: {token}…)\x1b[0m",
        inc.persona.summary(),
        inc.router.len(),
        t.chunk_bytes,
        t.delay_ms,
        t.initial_delay_ms,
        t.abort_probability * 100.0,
        cfg.deception.config_epoch_hours,
        cfg.deception.log_epoch_hours,
        cfg.stats.admin_path,
    );
}
