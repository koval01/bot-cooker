//! cooker-pack — generate a per-instance content pack with Claude, offline,
//! on the owner's machine. The trap binary never contacts any API.

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;

use bot_cooker::config::Config;
use bot_cooker::pack::{self, Client, Options};

#[derive(Parser, Debug)]
#[command(name = "cooker-pack", version, about = "Generate a unique content pack (templates_dir overlay) for one bot-cooker instance.")]
struct Cli {
    /// Config of the instance the pack is for (its master_key picks the persona).
    #[arg(short, long, default_value = "bot-cooker.yaml")]
    config: PathBuf,
    /// Directory to write the pack to.
    #[arg(short, long, default_value = "pack")]
    out: PathBuf,
    /// Claude model.
    #[arg(long, default_value = "claude-opus-5-5")]
    model: String,
    /// Effort level (low, medium, high, xhigh, max).
    #[arg(long, default_value = "high")]
    effort: String,
    /// Paragraphs per corpus section.
    #[arg(long, default_value_t = 30)]
    paragraphs: u32,
    /// Keep the built-in site theme.
    #[arg(long)]
    no_theme: bool,
    /// Print the prompts and exit without calling the API.
    #[arg(long)]
    dry_run: bool,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(Some(&cli.config)).context("loading configuration")?;
    let client = if cli.dry_run { None } else { Some(Client::from_env(cli.model, cli.effort)?) };
    let opts = Options { out: cli.out, paragraphs: cli.paragraphs, theme: !cli.no_theme, dry_run: cli.dry_run };
    pack::run(&cfg, client.as_ref(), &opts)
}
