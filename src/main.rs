//! bot-cooker — a defensive HTTP tarpit / honeypot.
//!
//! Runs on YOUR server. It answers every unsolicited request slowly and with
//! convincing fake content, so automated scanners burn their own time, threads,
//! and proxy budget while getting nothing real. It never reaches out to anyone;
//! it only slow-cooks clients that choose to connect to you.

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;

use bot_cooker::config::Config;
use bot_cooker::server;

#[derive(Parser, Debug)]
#[command(name = "bot-cooker", version, about = "A defensive HTTP tarpit for slow-cooking scanners.")]
struct Cli {
    /// Path to the YAML config file. Defaults to ./bot-cooker.yaml if present.
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Validate the configuration and exit without starting any listeners.
    #[arg(long)]
    check: bool,

    /// Append logs to this file instead of writing them to stderr.
    #[arg(long)]
    log_file: Option<PathBuf>,

    /// Run under the Windows Service Control Manager (set by the installer).
    #[cfg(windows)]
    #[arg(long)]
    service: bool,

    /// Service name to register as (with --service).
    #[cfg(windows)]
    #[arg(long, default_value = bot_cooker::service::SERVICE_NAME)]
    service_name: String,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    server::init_logging(cli.log_file.as_deref())?;
    let config_path = cli.config.clone().or_else(|| {
        let default = PathBuf::from("bot-cooker.yaml");
        default.exists().then_some(default)
    });

    #[cfg(windows)]
    if cli.service {
        return bot_cooker::service::dispatch(cli.service_name, config_path);
    }

    let cfg = Config::load(config_path.as_deref()).context("loading configuration")?;
    if cli.check {
        println!("{}", server::check(&cfg)?);
        return Ok(());
    }
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().context("starting the runtime")?;
    runtime.block_on(server::run(cfg, server::shutdown_signal()))
}
