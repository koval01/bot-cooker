//! A rendering scope: the world (persona + epochs), the request, and the
//! catalog. It builds the base context every template sees.

use minijinja::value::{Serde, Value};
use serde::Serialize;

use super::objects::Keyed;
use crate::catalog::Catalog;
use crate::persona::Persona;
use crate::util;
use crate::world::World;

/// The request as rendering needs it (owned, so streams can keep it).
#[derive(Clone, Debug, Serialize)]
pub struct Req {
    pub method: String,
    /// Lowercased, validated Host, or the persona's domain.
    pub host: String,
    pub scheme: &'static str,
    pub path: String,
    pub query: Option<String>,
}

impl Req {
    pub fn base_url(&self) -> String {
        format!("{}://{}", self.scheme, self.host)
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url())
    }

    /// Host without a port, as servers print their own name.
    pub fn server_name(&self) -> &str {
        self.host.rsplit_once(':').filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit())).map(|(h, _)| h).unwrap_or(&self.host)
    }

    pub fn port(&self) -> u16 {
        if self.scheme == "https" { 443 } else { 80 }
    }

    /// `?page=` from the query string.
    pub fn page(&self) -> u64 {
        self.param("page").and_then(|v| v.parse().ok()).unwrap_or(1).clamp(1, 1_000_000)
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.query.as_deref()?.split('&').find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
    }
}

#[derive(Serialize)]
struct RequestView<'a> {
    #[serde(flatten)]
    req: &'a Req,
    base_url: String,
    server_name: &'a str,
    port: u16,
}

/// Versions as of the current config epoch.
#[derive(Serialize)]
struct VersionsNow {
    server: String,
    runtime: String,
    runtime_short: String,
    framework: String,
    framework_short: String,
    framework_major: u32,
    db: String,
    db_short: String,
    db_major: u32,
}

pub struct Scope<'a> {
    pub world: World<'a>,
    pub req: Req,
    pub catalog: &'a Catalog,
}

pub fn ser<T: Serialize>(value: &T) -> Value {
    Value::from(Serde(value))
}

impl<'a> Scope<'a> {
    pub fn new(world: World<'a>, req: Req, catalog: &'a Catalog) -> Self {
        Scope { world, req, catalog }
    }

    pub fn persona(&self) -> &'a Persona {
        self.world.persona
    }

    pub fn url(&self, path: &str) -> String {
        self.req.url(path)
    }

    /// What every template sees.
    pub fn base(&self) -> Value {
        let p = self.persona();
        let e = &self.world.epochs;
        let v = &p.versions;
        let versions = VersionsNow {
            server: v.server.at(e.cfg_anchor),
            runtime: v.runtime.at(e.cfg_anchor),
            runtime_short: v.runtime.short(),
            framework: v.framework.at(e.cfg_anchor),
            framework_short: v.framework.short(),
            framework_major: v.framework.major,
            db: v.db.at(e.cfg_anchor),
            db_short: v.db.short(),
            db_major: v.db.major,
        };
        let tok_key = util::hmac(&p.mac(&[b"tok"]), &[self.req.path.as_bytes(), &e.log.to_le_bytes()]);
        minijinja::context! {
            persona => ser(p),
            web => ser(&p.web),
            site => ser(&p.site),
            assets => ser(&p.assets),
            style => ser(&p.style),
            epochs => ser(e),
            now => e.now,
            v => ser(&versions),
            request => ser(&RequestView { req: &self.req, base_url: self.req.base_url(), server_name: self.req.server_name(), port: self.req.port() }),
            secret => Keyed::value(self.world.secret_key(), p.secrets.alphabet, p.secrets.len),
            tok => Keyed::value(tok_key, util::ALNUM, 32),
            docroot => self.docroot(),
            // How the application's own JSON encoder writes API responses.
            api_json => if p.family == crate::persona::Family::Php { "php" } else { "compact" },
            persona_seed => u64::from_le_bytes(p.mac(&[b"template-seed"])[..8].try_into().unwrap_or([0; 8])) >> 1,
        }
    }

    /// Document root on the imaginary server (stable per persona).
    pub fn docroot(&self) -> String {
        use rand::RngExt;
        let p = self.persona();
        let short = crate::persona::short_slug(&p.slug);
        let mut rng = p.rng("docroot");
        if p.frontend == crate::persona::Frontend::Iis {
            return format!("C:\\inetpub\\{}", if rng.random_bool(0.5) { "wwwroot" } else { short });
        }
        let base = match (p.distro, rng.random_range(0..4)) {
            (crate::persona::Distro::Rhel, 0) => format!("/var/www/{short}"),
            (_, 0) => "/var/www/html".to_string(),
            (_, 1) => format!("/var/www/{}", p.domain),
            (_, 2) => format!("/home/{}/{}", if p.is_php() { "forge" } else { "deploy" }, p.domain),
            _ => format!("/srv/{short}"),
        };
        match p.stack.id() {
            "laravel" | "symfony" => format!("{base}/public"),
            "rails" => format!("{base}/current/public"),
            "drupal" if p.versions.framework.major >= 9 => format!("{base}/web"),
            _ => base,
        }
    }

    /// Render `name` with the base context plus `extra`.
    pub fn render(&self, name: &str, extra: Value) -> Result<String, minijinja::Error> {
        self.catalog.render(name, minijinja::context! { ..self.base(), ..extra })
    }

    /// Render, and on failure log it and return an empty body: a broken
    /// overlay template must not take the trap down. (The test suite renders
    /// every embedded template for many personas, so this never fires there.)
    pub fn render_or_empty(&self, name: &str, extra: Value) -> String {
        self.render(name, extra).unwrap_or_else(|e| {
            // Unit tests must see the failure, not an empty page.
            #[cfg(test)]
            panic!("template {name}: {e:#}");
            #[cfg(not(test))]
            {
                tracing::error!("template {name}: {e:#}");
                String::new()
            }
        })
    }

    /// The first template that exists among `names`.
    pub fn pick(&self, names: &[String]) -> Option<String> {
        names.iter().find(|n| self.catalog.has(n)).cloned()
    }

    /// `stacks/<stack>/<file>`, falling back to `stacks/default/<file>`.
    pub fn stack_template(&self, file: &str) -> String {
        let own = format!("stacks/{}/{file}", self.persona().stack.id());
        self.pick(&[own.clone(), format!("stacks/default/{file}")]).unwrap_or(own)
    }

    /// `servers/<frontend>/<file>`, falling back to `servers/default/<file>`.
    pub fn server_template(&self, file: &str) -> String {
        let own = format!("servers/{}/{file}", self.persona().frontend.id());
        self.pick(&[own.clone(), format!("servers/default/{file}")]).unwrap_or(own)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(host: &str, query: Option<&str>) -> Req {
        Req { method: "GET".into(), host: host.into(), scheme: "https", path: "/x".into(), query: query.map(str::to_string) }
    }

    #[test]
    fn request_helpers() {
        let r = req("shop.example.com:8443", Some("a=1&page=3"));
        assert_eq!(r.server_name(), "shop.example.com");
        assert_eq!(r.url("/y"), "https://shop.example.com:8443/y");
        assert_eq!(r.page(), 3);
        assert_eq!(req("h", Some("page=0")).page(), 1);
        assert_eq!(req("h", None).page(), 1);
        assert_eq!(r.param("pa"), None);
    }
}
