//! The persona's types. Everything here is plain data: derivation lives in
//! `derive/`, facts per stack and server in `templates/data/*.yaml`. All of it
//! serializes, so templates can read the persona directly.

use serde::{Deserialize, Serialize};

use super::version::Ver;
use crate::data::{Data, Industry};

macro_rules! key_enum {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $variant:ident = $id:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub enum $name {
            $($(#[$vm])* #[serde(rename = $id)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The id used in data files and template paths.
            pub fn id(self) -> &'static str {
                match self {
                    $($name::$variant => $id),+
                }
            }
        }
    };
}

key_enum!(
    /// Application stack.
    Stack {
        Laravel = "laravel",
        Symfony = "symfony",
        Django = "django",
        Rails = "rails",
        Express = "express",
        SpringBoot = "spring_boot",
        AspNet = "asp_net",
        WordPress = "wordpress",
        Joomla = "joomla",
        Drupal = "drupal",
    }
);

key_enum!(
    /// Language runtime family of a stack.
    Family {
        Php = "php",
        Python = "python",
        Ruby = "ruby",
        Node = "node",
        Java = "java",
        Dotnet = "dotnet",
    }
);

key_enum!(
    /// The web server in front of the application.
    Frontend {
        Nginx = "nginx",
        Apache = "apache",
        OpenResty = "openresty",
        Caddy = "caddy",
        Iis = "iis",
        LiteSpeed = "litespeed",
    }
);

key_enum!(
    DbEngine {
        MySql = "mysql",
        MariaDb = "mariadb",
        Postgres = "postgres",
        MsSql = "mssql",
    }
);

key_enum!(
    Cloud {
        Aws = "aws",
        Gcp = "gcp",
        Azure = "azure",
        DigitalOcean = "digitalocean",
        Hetzner = "hetzner",
        BareMetal = "bare_metal",
    }
);

key_enum!(
    /// The operating system the trap really runs on. Its TCP/IP stack is
    /// visible to OS fingerprinting (TTL, window size, options), so the
    /// persona's server and distro must agree with it.
    HostOs {
        Unix = "unix",
        Windows = "windows",
    }
);

impl HostOs {
    /// The OS this binary was built for.
    pub fn current() -> Self {
        if cfg!(windows) { HostOs::Windows } else { HostOs::Unix }
    }
}

key_enum!(
    Distro {
        Ubuntu = "ubuntu",
        Debian = "debian",
        Rhel = "rhel",
        Alpine = "alpine",
        Windows = "windows",
    }
);

impl Distro {
    pub fn host_os(self) -> HostOs {
        if self == Distro::Windows { HostOs::Windows } else { HostOs::Unix }
    }
}

key_enum!(
    /// Field naming convention of the stack's ORM / JSON.
    Naming {
        Snake = "snake",
        Camel = "camel",
        Pascal = "pascal",
    }
);

key_enum!(
    /// How the team quotes `.env` values.
    Quote {
        Bare = "bare",
        Double = "double",
        Single = "single",
        WhenNeeded = "needed",
    }
);

key_enum!(
    /// How config files separate their groups.
    Sections {
        BlankLines = "blank_lines",
        Comments = "comments",
        Dense = "dense",
    }
);

key_enum!(
    /// What the front server does with dotfiles.
    DotPolicy {
        /// Dotfiles that exist are served (the leak); others 404.
        Serve = "serve",
        /// `deny all` for dotfiles: every one is a 403.
        Deny = "deny",
    }
);

key_enum!(
    /// JSON list envelope of the stack's API.
    Envelope {
        DataLinksMeta = "data_links_meta",
        CountResults = "count_results",
        SpringPage = "spring_page",
        BareArray = "bare_array",
        Items = "items",
        Hal = "hal",
    }
);

key_enum!(
    AreaKind {
        /// A browsable directory listing in the front server's autoindex style.
        Autoindex = "autoindex",
        /// Article-like pages rendered by the application.
        Blog = "blog",
        /// Documentation / knowledge-base pages.
        Docs = "docs",
    }
);

key_enum!(
    LogKind {
        Access = "access",
        Error = "error",
        App = "app",
    }
);

key_enum!(
    AccessFormat {
        Combined = "combined",
        VhostCombined = "vhost_combined",
        Json = "json",
        Iis = "iis",
    }
);

#[derive(Clone, Debug, Serialize)]
pub struct Versions {
    pub server: Ver,
    /// PHP / Python / Ruby / Node / Java / .NET runtime.
    pub runtime: Ver,
    /// Framework or CMS.
    pub framework: Ver,
    pub db: Ver,
}

#[derive(Clone, Debug, Serialize)]
pub struct Person {
    pub first: &'static str,
    pub last: &'static str,
    pub email: String,
    pub login: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Database {
    pub engine: DbEngine,
    pub host: String,
    pub port: u16,
    pub name: String,
    pub user: String,
    pub table_prefix: String,
    /// mysqldump `--extended-insert` (multi-row INSERTs) vs one row per statement.
    pub extended_insert: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Cache {
    pub redis: bool,
    pub host: String,
    pub port: u16,
    pub has_password: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Mail {
    pub provider: &'static str,
    pub host: &'static str,
    pub port: u16,
    pub from: String,
}

/// How this "team" writes config files.
#[derive(Clone, Debug, Serialize)]
pub struct Style {
    pub crlf: bool,
    pub env_export: bool,
    pub env_quote: Quote,
    pub env_spaced: bool,
    pub sections: Sections,
    pub comment_density: f64,
    pub trailing_newline: bool,
    pub indent: &'static str,
    pub php_double_quotes: bool,
    pub php_short_arrays: bool,
    pub json_pretty: Option<usize>,
    pub naming: Naming,
}

#[derive(Clone, Debug, Serialize)]
pub struct SecretStyle {
    #[serde(skip)]
    pub alphabet: &'static [u8],
    pub len: usize,
    /// Secrets rotate every this many config epochs.
    pub rotation: u64,
}

/// The instance's timing temperament; per-request values are sampled around
/// these by the tarpit, scaled by the operator's `tarpit` config.
#[derive(Clone, Debug, Serialize)]
pub struct Timing {
    /// The application's usual response time relative to the operator's
    /// `initial_delay_ms`, and its log-normal spread.
    pub ttfb_factor: f64,
    pub ttfb_sigma: f64,
    /// How long after the headers the body starts, relative to the response
    /// time (a page still rendering after the headers were flushed).
    pub body_lag: f64,
    /// Overall drip pace relative to the operator's `delay_ms`.
    pub tick_factor: f64,
    /// How much a whole response runs faster or slower than the instance's
    /// usual pace (log-normal shape of a per-request multiplier).
    pub request_sigma: f64,
    /// Delivery regimes a response moves between, indexed by [`Pace`].
    pub regimes: [Regime; Pace::ALL.len()],
    /// Chance that a stall ends in a brisk burst rather than any regime.
    pub burst_after_stall: f64,
    pub abort_factor: f64,
    /// Relative weights for aborting before the body / within the head / later.
    pub abort_stages: [u32; 3],
    pub fast_prob: f64,
    /// How close to the operator's `max_idle_ms` this instance's longest
    /// silences get.
    pub idle_share: f64,
    /// Chance per chunk of a network hiccup: a lost packet that stalls the
    /// stream for one or more retransmission timeouts, after which what was
    /// held back arrives at once.
    pub hiccup_prob: f64,
    /// The path's retransmission timeout, in milliseconds.
    pub rto_ms: f64,
    /// This instance's typical piece of the body relative to the operator's
    /// `chunk_bytes`, so instances do not share one piece size.
    pub chunk_scale: f64,
    /// Size of the HTTP chunks the stack emits in a chunked body: the
    /// buffer it flushes (nginx, FastCGI, Node), not the trickle beneath.
    pub frame_bytes: usize,
}

/// A stretch of delivery within one slow response.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Pace {
    /// Larger chunks with short gaps, like a warm buffer flushing.
    Brisk,
    /// Around the operator's chunk size and delay.
    Steady,
    /// Tiny chunks that still come out slowly.
    Sluggish,
    /// A byte or two, then a long silence.
    Stalled,
}

impl Pace {
    pub const ALL: [Pace; 4] = [Pace::Brisk, Pace::Steady, Pace::Sluggish, Pace::Stalled];
}

/// One regime of the instance's drip: gap and chunk medians relative to the
/// instance's tick and the operator's `chunk_bytes`, their log-normal
/// shapes, how long it lasts and how often a response enters it.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Regime {
    pub gap: f64,
    pub gap_sigma: f64,
    pub chunk: f64,
    pub chunk_sigma: f64,
    /// Mean number of chunks spent in the regime once entered.
    pub dwell: f64,
    /// Relative weight for entering (or starting in) the regime.
    pub weight: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Web {
    pub server_banner: Option<String>,
    pub powered_by: Option<String>,
    pub session_cookie: String,
    pub csrf_cookie: Option<String>,
    pub request_id_header: Option<&'static str>,
    pub security_headers: Vec<(&'static str, String)>,
    pub dotfiles: DotPolicy,
    pub access_format: AccessFormat,
    /// Per-header ordering salt, so header order is instance-specific.
    pub order_salt: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Area {
    pub prefix: String,
    pub kind: AreaKind,
}

key_enum!(
    /// What `/favicon.ico` is on this site.
    Favicon {
        /// The site's own icon.
        Own = "own",
        /// A zero-byte file (Laravel's skeleton ships one).
        Empty = "empty",
        /// No file: a 404.
        None = "none",
    }
);

/// The static files the site's pages link to, fixed once per persona so
/// pages, logs and the route table agree on every name.
#[derive(Clone, Debug, Serialize)]
pub struct Assets {
    /// By the name templates use: `assets.files.css.href`.
    pub files: std::collections::BTreeMap<String, AssetFile>,
    pub favicon: Favicon,
    pub brand: Brand,
    /// Values picked for path placeholders (`{pick:wp_theme}` -> `wp_theme`).
    pub picks: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AssetFile {
    pub path: String,
    /// What pages put in `href`/`src`: the path plus any cache-busting query.
    pub href: String,
    /// Template that renders the file.
    #[serde(skip)]
    pub body: &'static str,
}

/// The site's own look: what its stylesheet and icon are made of.
#[derive(Clone, Debug, Serialize)]
pub struct Brand {
    /// `#rrggbb`.
    pub primary: String,
    pub accent: String,
    pub ink: String,
    pub paper: String,
    pub font: &'static str,
    /// Border radius in px.
    pub radius: u8,
    /// Main column width in px.
    pub width: u16,
    pub icon: IconShape,
}

key_enum!(
    /// The figure on the site's own favicon.
    IconShape {
        Square = "square",
        Rounded = "rounded",
        Circle = "circle",
        Diamond = "diamond",
        Split = "split",
        Ring = "ring",
    }
);

#[derive(Clone, Debug, Serialize)]
pub struct Site {
    pub areas: Vec<Area>,
    pub vocabulary: Vec<&'static str>,
    pub env_paths: Vec<String>,
    pub config_leaks: Vec<String>,
    pub backups: Vec<String>,
    pub logs: Vec<(String, LogKind)>,
    pub git: bool,
    pub aws_credentials: bool,
    pub ssh_key: bool,
    pub htpasswd: bool,
    pub phpinfo: Option<String>,
    pub server_status: bool,
    pub docker_compose: bool,
    pub sitemap: Option<String>,
    pub feed: Option<String>,
    pub login_path: String,
    pub api_prefix: String,
    pub api_resources: Vec<&'static str>,
    pub envelope: Envelope,
    pub per_page: u32,
    pub git_remote: String,
    pub default_branch: &'static str,
}

key_enum!(
    /// What lets PHP errors reach a page.
    PhpDisplay {
        /// Nothing does.
        Off = "off",
        /// php.ini `display_errors = On`.
        Ini = "ini",
        /// The application turns it on (`WP_DEBUG`, Joomla's error reporting).
        App = "app",
    }
);

key_enum!(
    /// An error level as PHP labels it.
    PhpLevel {
        Error = "error",
        Warning = "warning",
        Notice = "notice",
        Deprecated = "deprecated",
    }
);

key_enum!(
    /// Where a page shows an error: before any markup (raised while the
    /// application boots) or inside the content it was rendering.
    #[derive(Default)]
    PhpPlace {
        #[default]
        Top = "top",
        Body = "body",
    }
);

/// How the persona's PHP application reports errors, and what goes wrong in
/// its code. Only stacks that let PHP print raw errors have one.
#[derive(Clone, Debug, Serialize)]
pub struct PhpErrors {
    /// What the config files say: `WP_DEBUG`, Joomla's `error_reporting`.
    pub setting: &'static str,
    pub display: PhpDisplay,
    /// Levels error_reporting lets through.
    pub levels: Vec<PhpLevel>,
    /// php.ini `memory_limit`, in MiB.
    pub memory_limit: u32,
    /// Faulty places, fixed for the persona era; each config epoch some of
    /// them fire.
    pub faults: Vec<PhpFault>,
    /// Where pages that run out of memory die, and how many do.
    pub fatal: Option<PhpFault>,
    pub fatal_share: f64,
}

/// One place in the code that raises an error.
#[derive(Clone, Debug, Serialize)]
pub struct PhpFault {
    /// Message kind (`php_errors.yaml`).
    pub kind: &'static str,
    /// Relative to the application root. The application's own notices
    /// have no line of their own: it moves with the release.
    pub file: &'static str,
    pub line: Option<u32>,
    pub place: PhpPlace,
    /// Values for the message's `{slots}`.
    pub slots: Vec<(&'static str, String)>,
}

/// One imaginary organisation and the system it runs.
#[derive(Clone, Debug, Serialize)]
pub struct Persona {
    #[serde(skip)]
    pub(super) seed: [u8; 32],
    /// The data the persona was derived from (word lists etc.).
    #[serde(skip)]
    pub data: &'static Data,
    pub company: String,
    pub slug: String,
    pub domain: String,
    pub industry: &'static Industry,
    pub stack: Stack,
    pub stack_name: &'static str,
    pub family: Family,
    pub cms: bool,
    pub frontend: Frontend,
    pub distro: Distro,
    pub versions: Versions,
    pub db: Database,
    pub cache: Option<Cache>,
    pub mail: Mail,
    pub cloud: Cloud,
    pub region: &'static str,
    pub(super) net: (u8, u8, u8),
    pub hostname: String,
    pub app_name: String,
    pub app_env: &'static str,
    pub tz_offset: i32,
    pub tz_name: &'static str,
    pub staff: Vec<Person>,
    /// Name locales and their weights (from the time zone, perturbed).
    #[serde(skip)]
    pub name_mix: Vec<(&'static str, u32)>,
    /// The persona's own prose model (trained on its subset of the corpus).
    #[serde(skip)]
    pub text: std::sync::Arc<crate::text::TextModel>,
    /// The persona's word choices.
    #[serde(skip)]
    pub voice: std::sync::Arc<crate::text::Voice>,
    pub style: Style,
    pub secrets: SecretStyle,
    pub timing: Timing,
    pub web: Web,
    pub site: Site,
    pub assets: Assets,
    pub php_errors: Option<PhpErrors>,
}
