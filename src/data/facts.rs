//! `stacks.yaml`, `servers.yaml`, `infra.yaml`: what personas are drawn from.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::Weights;
use crate::config::RouteRule;
use crate::persona::{AccessFormat, Cloud, DbEngine, Distro, Envelope, Family, Frontend, Naming};

/// A path that exists with probability `p`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Maybe {
    pub path: String,
    pub p: f64,
}

/// With probability `p`, one of `paths`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OneOf {
    pub p: f64,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoweredBy {
    pub p: f64,
    pub value: String,
    /// Only emitted behind this front server.
    pub frontend: Option<Frontend>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackFacts {
    pub name: String,
    pub family: Family,
    #[serde(default)]
    pub cms: bool,
    pub weight: u32,
    pub frontends: Weights<Frontend>,
    pub databases: Weights<DbEngine>,
    /// Overrides for cloud weights (others keep their default weight).
    #[serde(default)]
    pub clouds: Weights<Cloud>,
    pub versions: Vec<String>,
    pub naming: Weights<Naming>,
    #[serde(default = "default_crlf")]
    pub crlf: f64,
    /// Probability that the app is configured through a `.env` file.
    #[serde(default)]
    pub dotenv: f64,
    #[serde(default)]
    pub dotenv_extras: Vec<Maybe>,
    #[serde(default)]
    pub config_leaks: Vec<Maybe>,
    /// Leaked when none of `config_leaks` came up, so every box leaks something.
    pub config_fallback: Option<String>,
    pub app_logs: Option<OneOf>,
    pub login_paths: Vec<String>,
    pub api_prefixes: Vec<String>,
    pub api_resources: Option<Vec<String>>,
    pub envelopes: Weights<Envelope>,
    pub areas: Option<Vec<String>>,
    pub sitemaps: Option<OneOf>,
    pub feeds: Option<OneOf>,
    pub repos: Option<Vec<String>>,
    pub db_names: Option<Vec<String>>,
    pub db_users: Option<Vec<String>>,
    #[serde(default)]
    pub table_prefixes: Weights<String>,
    pub session_cookies: Weights<String>,
    pub csrf_cookie: Option<String>,
    #[serde(default = "always")]
    pub csrf_cookie_p: f64,
    pub powered_by: Option<PoweredBy>,
    /// Request-id header name, `none`, or absent to use the infra default.
    pub request_id: Option<String>,
    #[serde(default)]
    pub security_headers: Vec<(String, String)>,
    pub hash: String,
    #[serde(default)]
    pub routes: Vec<RouteRule>,
    /// Inline templates, see the header of stacks.yaml.
    #[serde(default)]
    pub app_headers: Vec<(String, String)>,
    #[serde(default)]
    pub session_headers: Vec<(String, String)>,
    #[serde(default)]
    pub cookies: Vec<String>,
    pub request_id_format: Option<String>,
    pub options: String,
    pub post: String,
    pub error_type: Option<String>,
}

fn default_crlf() -> f64 {
    0.12
}

fn always() -> f64 {
    1.0
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerFacts {
    pub distros: Weights<Distro>,
    /// Version lists keyed by distro id, with `default` as the fallback.
    pub versions: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub flavours: BTreeMap<Distro, String>,
    pub banner: String,
    pub banner_hidden: Option<String>,
    #[serde(default)]
    pub hide_p: f64,
    pub access_formats: Weights<AccessFormat>,
    /// Inline template for a static file's ETag (vars: mtime, size, salt).
    pub etag: String,
    pub static_headers: Vec<String>,
    pub mime: BTreeMap<String, String>,
    pub mime_default: Option<String>,
    pub page_type: String,
    #[serde(default)]
    pub crlf_pages: bool,
    pub static_options: String,
    pub unknown_method: u16,
    /// How the server writes HTTP/1.1 on the wire.
    pub wire: WireFacts,
}

/// The front server's protocol habits: what a scanner sees before any
/// content (header case and order, connection handling, its own errors).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireFacts {
    pub case: HeaderCase,
    /// Header names written first, in this order, when present. `date`,
    /// `content-length`, `transfer-encoding`, `connection` and `keep-alive`
    /// are placed by these lists like any other header.
    #[serde(default)]
    pub first: Vec<String>,
    /// Header names written last, in this order.
    #[serde(default)]
    pub last: Vec<String>,
    /// All headers sorted by name (Go's net/http).
    #[serde(default)]
    pub sorted: bool,
    /// Answers HTTP/1.0 requests with an `HTTP/1.0` status line (Go);
    /// everyone else always says `HTTP/1.1`.
    #[serde(default)]
    pub echo_version: bool,
    pub keep_alive: KeepAliveStyle,
    /// Idle time before a kept-alive connection is closed.
    pub keepalive_secs: u64,
    /// Requests per connection before the server closes it.
    pub keepalive_max: u32,
    /// Time allowed to send a complete request head.
    pub header_timeout_secs: u64,
    /// Longest request line it accepts; past it, `uri_too_long`.
    pub max_request_line: usize,
    /// Largest request head it accepts; past it, `head_too_large`.
    pub max_head: usize,
    pub uri_too_long: u16,
    pub head_too_large: u16,
    /// Largest request body it reads; past it, 413.
    pub max_body: u64,
    /// Normalises `..` in paths instead of refusing them (Caddy).
    #[serde(default)]
    pub cleans_paths: bool,
    /// Reason phrases that differ from the standard ones.
    #[serde(default)]
    pub reasons: BTreeMap<u16, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HeaderCase {
    /// `Content-Type`, `ETag`, `WWW-Authenticate`: as servers and frameworks
    /// spell them.
    Title,
    /// Go's canonical form: `Etag`, `Www-Authenticate`, `X-Xss-Protection`.
    Go,
    /// `content-type`; `connection` and `keep-alive` stay title case.
    Lower,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeepAliveStyle {
    /// `Connection: keep-alive` (nginx).
    Explicit,
    /// `Keep-Alive: timeout=5, max=99` and `Connection: Keep-Alive`
    /// (Apache, LiteSpeed).
    Counted,
    /// Nothing unless the connection closes (IIS, Go).
    Implicit,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseFacts {
    pub versions: Vec<String>,
    pub ports: Vec<u16>,
    /// Host names people give this engine (`db`, `postgres`, ...).
    pub aliases: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudFacts {
    pub weight: u32,
    pub regions: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailProvider {
    pub id: String,
    pub host: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timezone {
    pub name: String,
    pub offset: i32,
    /// Name locales typical for a company in this zone.
    pub locales: Weights<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityHeader {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Infra {
    pub runtimes: BTreeMap<Family, Vec<String>>,
    pub databases: BTreeMap<DbEngine, DatabaseFacts>,
    pub db_names: Vec<String>,
    pub db_users: Vec<String>,
    pub db_hosts: Vec<String>,
    pub cache_hosts: Vec<String>,
    pub clouds: BTreeMap<Cloud, CloudFacts>,
    pub mail_providers: Vec<MailProvider>,
    pub mail_ports: Vec<u16>,
    pub mail_senders: Vec<String>,
    pub timezones: Vec<Timezone>,
    pub app_envs: Vec<String>,
    pub app_name_suffixes: Vec<String>,
    pub hostnames: Vec<String>,
    pub dotenv_extras: Vec<String>,
    pub dotenv_extra_p: f64,
    pub backup_dirs: Vec<String>,
    pub backup_stems: Vec<String>,
    pub backup_exts: Vec<String>,
    pub access_logs: Vec<String>,
    pub access_log_p: f64,
    pub error_logs: Vec<String>,
    pub error_log_p: f64,
    pub phpinfo_paths: Vec<String>,
    pub default_areas: Vec<String>,
    pub default_sitemaps: OneOf,
    pub default_feeds: OneOf,
    pub default_repos: Vec<String>,
    pub git_hosts: Weights<String>,
    pub git_org_suffixes: Vec<String>,
    pub default_branches: Weights<String>,
    pub request_ids: Weights<String>,
    pub security_headers: Vec<SecurityHeader>,
    pub security_header_p: f64,
}
