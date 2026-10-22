//! Configuration model for bot-cooker.
//!
//! Loaded from a YAML file (see `bot-cooker.yaml`). Every field has a sane
//! default so a minimal config — or none at all — still produces a working
//! tarpit. Environment variables can override a few key fields so the Docker
//! "one command" workflow needs no file edits.

use std::net::SocketAddr;
use std::path::PathBuf;

use serde::Deserialize;

use crate::persona::HostOs;

/// Top-level configuration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Plain-HTTP listener. Always on — this is what you point a reverse proxy
    /// (Caddy / Nginx / Cloudflare DNS-only) at, and what catches port-80 bots.
    pub http: HttpConfig,

    /// Tarpit behaviour: how slowly and how much to feed each trapped client.
    pub tarpit: TarpitConfig,

    /// Deterministic deception engine (epoch-stable fake artifacts).
    pub deception: DeceptionConfig,

    /// Routing rules: which request paths map to which deception profile.
    /// Evaluated top-to-bottom; first match wins; unmatched paths get the
    /// persona's realistic 404. When omitted (the default), the table is
    /// derived from the instance persona so every instance exposes a different,
    /// stack-consistent set of paths. When set, it replaces that table.
    pub routes: Option<Vec<RouteRule>>,

    /// OPT-IN escape hatch, empty by default. On a dedicated trap subdomain you
    /// want to catch *everything*, so leave this empty. Only relevant if you run
    /// bot-cooker alongside a real site. NOTE: User-Agent matching is trivially
    /// spoofable (a scanner just sends `User-Agent: Googlebot`), so trust only
    /// the CIDR list for anything that matters.
    pub allowlist: Allowlist,

    /// Global safety limits — these protect *your* box, not the bots'.
    pub limits: Limits,

    /// Live statistics: console reporting + optional token-gated JSON endpoint.
    pub stats: StatsConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpConfig {
    pub enabled: bool,
    pub bind: SocketAddr,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TarpitConfig {
    /// Typical time until the status line and headers go out, in
    /// milliseconds: the application's response time. A PHP or Rails app
    /// answers in a few hundred milliseconds; much longer and a scanner takes
    /// the server for dead and moves on. The time is spent in the body.
    pub initial_delay_ms: u64,
    /// Spread (0.0..=1.0) of that response time.
    pub initial_delay_jitter: f64,
    /// Bytes emitted per drip tick.
    pub chunk_bytes: usize,
    /// Delay between drip ticks, in milliseconds.
    pub delay_ms: u64,
    /// Add +/- this fraction of jitter to each delay (0.0..=1.0) so the cadence
    /// isn't a clean constant a scanner can fingerprint.
    pub delay_jitter: f64,
    /// Probability (0.0..=1.0) that a connection is abruptly cut mid-response,
    /// leaving the scanner with a truncated body and a read error to handle.
    pub abort_probability: f64,
    /// Cap on bytes fed to a single connection. Once reached, the block in
    /// progress (a few log lines, sitemap entries or rows) is finished and the
    /// document is closed cleanly, so a body can run slightly past it.
    pub max_bytes: u64,
    /// Hard cap on how long a single connection may hang, in seconds.
    pub max_seconds: u64,
    /// Longest silence on an open connection, in milliseconds. Kept below the
    /// read timeouts scanners use, so the server never looks dead and the
    /// client keeps waiting for the next bytes. Each instance stays some way
    /// below it, never at a fixed ceiling.
    pub max_idle_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DeceptionConfig {
    /// Master secret seeding all deterministic generation. If empty, a random
    /// one is generated at startup (then artifacts reset on every restart —
    /// set a fixed value in production so they stay stable across restarts).
    pub master_key: String,
    /// Epoch length for config-style artifacts (.env, .git), in hours. Within
    /// one epoch the same path always yields identical content.
    pub config_epoch_hours: u64,
    /// Epoch length for log/structure artifacts, in hours.
    pub log_epoch_hours: u64,
    /// Random jitter (hours) added to epoch boundaries so rotations don't land
    /// on predictable wall-clock times.
    pub epoch_jitter_hours: u64,
    /// How long one persona (company, stack, layout, style) lives, in days.
    /// `0` = forever: the persona only changes with the master key.
    pub persona_era_days: u64,
    /// Optional directory overlaid on the embedded template catalog: files
    /// with the same relative path replace the built-in ones.
    pub templates_dir: Option<PathBuf>,
    /// The OS whose TCP/IP stack scanners see (`auto` = the one this binary
    /// runs on). Personas only use servers that run on it. Set it when the
    /// connection terminates elsewhere, e.g. `windows` for a Linux container
    /// published through Docker Desktop on a Windows host.
    pub host_os: HostOsSetting,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostOsSetting {
    #[default]
    Auto,
    Unix,
    Windows,
}

impl HostOsSetting {
    pub fn resolve(self) -> HostOs {
        match self {
            HostOsSetting::Auto => HostOs::current(),
            HostOsSetting::Unix => HostOs::Unix,
            HostOsSetting::Windows => HostOs::Windows,
        }
    }
}

/// A single routing rule.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteRule {
    /// Glob-ish pattern matched against the request path. `*` matches any run
    /// of characters. Examples: `/.env`, `/.git/*`, `*.sql`, `/wp-login.php`.
    pub pattern: String,
    /// Which generator to serve for a match.
    pub profile: Profile,
}

/// The set of deception generators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// A leaked config file; the format follows the path and the persona's
    /// stack (`.env`, `wp-config.php`, `database.yml`, `appsettings.json`, …).
    Env,
    /// Fake `.git/config`, `.git/HEAD`, `.git/logs/HEAD` style metadata.
    Git,
    /// A fake SQL dump in the persona's DB dialect, schema and data.
    SqlDump,
    /// An endless, chronologically-consistent fake application/access log.
    Log,
    /// The stack's login page (WordPress, Django admin, Devise, …).
    #[serde(alias = "wp_login")]
    Login,
    /// A phpinfo()-style page.
    PhpInfo,
    /// Apache `mod_status` page.
    ServerStatus,
    /// Joomla version manifest (`administrator/manifests/files/joomla.xml`).
    Joomla,
    /// Drupal `CHANGELOG.txt` (version disclosure).
    Drupal,
    /// A "leaky" JSON API response shaped like the stack's API.
    Json,
    /// A (very long) sitemap of the persona's site.
    Sitemap,
    /// The site's RSS/Atom feed.
    Rss,
    /// Secret-looking blobs: fake PEM private key, `.htpasswd`, AWS credentials.
    Secrets,
    /// Site pages and directory listings linking deeper into fake paths
    /// (crawler / AI-scraper trap), in the persona's look.
    Labyrinth,
    /// The front server's or framework's real-looking 404.
    NotFound,
    /// A real-looking 403 (e.g. denied dotfiles).
    Forbidden,
    /// An executed-but-silent script (`wp-config.php` returns an empty 200).
    Blank,
    /// A static file the site's pages link to (stylesheet, script, favicon),
    /// served at once like any file off disk.
    Asset,
}

/// Empty by default: on a dedicated trap host, nothing legitimate exists, and
/// sparing clients by User-Agent would just hand scanners a one-header bypass.
/// Populate `cidrs` only if you knowingly run alongside a real site.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Allowlist {
    /// Case-insensitive substrings matched against the User-Agent header.
    pub user_agents: Vec<String>,
    /// CIDR ranges (IPv4/IPv6) whose clients bypass the tarpit.
    pub cidrs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Maximum simultaneous tarpitted connections. Beyond this, new clients get
    /// an instant fake response (no slow hold) so your own box can't be
    /// exhausted by the sheer number of bots. Each held connection costs about
    /// 30 KiB of memory and one open file; at startup the cap is lowered to
    /// what the open-files limit allows.
    pub max_concurrent: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StatsConfig {
    /// Seconds between console status reports (0 disables console reporting).
    pub report_interval_secs: u64,
    /// Expose a JSON stats endpoint at `admin_path` when a token is set.
    pub admin_path: String,
    /// Secret token required in the `X-Admin-Token` header (or `?token=`). If
    /// empty, a random one is generated and printed at startup.
    pub admin_token: String,
    /// Max number of distinct client IPs kept in the per-IP table.
    pub max_tracked_ips: usize,
}

// ------------------------- Defaults -------------------------

impl Default for HttpConfig {
    fn default() -> Self {
        Self { enabled: true, bind: "0.0.0.0:8080".parse().unwrap() }
    }
}

impl Default for TarpitConfig {
    fn default() -> Self {
        Self {
            initial_delay_ms: 350, // headers after ~0.2-0.5 s, like a PHP app
            initial_delay_jitter: 0.5,
            chunk_bytes: 48,
            delay_ms: 800,
            delay_jitter: 0.4,
            abort_probability: 0.15,    // ~15% of connections get cut mid-stream
            max_bytes: 4 * 1024 * 1024, // 4 MiB fed before we give up on them
            max_seconds: 1200,          // 20 minutes max hold
            max_idle_ms: 6_000,         // never silent long enough to look dead
        }
    }
}

impl Default for DeceptionConfig {
    fn default() -> Self {
        Self {
            master_key: String::new(),
            config_epoch_hours: 168, // ~7 days: content rotates weekly
            log_epoch_hours: 8,      // logs shift faster, like a live server
            epoch_jitter_hours: 6,
            persona_era_days: 0,
            templates_dir: None,
            host_os: HostOsSetting::Auto,
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_concurrent: 8_000 }
    }
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self { report_interval_secs: 60, admin_path: "/_cooker/stats".to_string(), admin_token: String::new(), max_tracked_ips: 50_000 }
    }
}

impl Config {
    /// Load from a YAML file, or return defaults if the path is `None` or the
    /// file does not exist. Applies env-var overrides last.
    pub fn load(path: Option<&std::path::Path>) -> anyhow::Result<Self> {
        let mut cfg = match path {
            Some(p) if p.exists() => {
                let text = std::fs::read_to_string(p).map_err(|e| anyhow::anyhow!("reading config {}: {e}", p.display()))?;
                serde_norway::from_str(&crate::catalog::normalize(&text))
                    .map_err(|e| anyhow::anyhow!("parsing config {}: {e}", p.display()))?
            }
            Some(p) => {
                tracing::warn!("config file {} not found, using defaults", p.display());
                Config::default()
            }
            None => Config::default(),
        };
        cfg.apply_env_overrides();
        Ok(cfg)
    }

    /// A few env vars make the Docker workflow a one-liner.
    fn apply_env_overrides(&mut self) {
        if let Ok(key) = std::env::var("COOKER_MASTER_KEY")
            && !key.is_empty()
        {
            self.deception.master_key = key;
        }
        if let Ok(token) = std::env::var("COOKER_ADMIN_TOKEN")
            && !token.is_empty()
        {
            self.stats.admin_token = token;
        }
        if let Ok(bind) = std::env::var("COOKER_HTTP_BIND")
            && let Ok(addr) = bind.parse()
        {
            self.http.bind = addr;
        }
        if let Ok(n) = std::env::var("COOKER_MAX_CONCURRENT")
            && let Ok(n) = n.parse()
        {
            self.limits.max_concurrent = n;
        }
        if let Ok(dir) = std::env::var("COOKER_TEMPLATES_DIR")
            && !dir.is_empty()
        {
            self.deception.templates_dir = Some(PathBuf::from(dir));
        }
    }
}
