//! Rendering: turn a routed request into an `Artifact` by choosing
//! templates and building their context. Profiles live in `profiles/`, text
//! lives in `templates/`, values come from `world/` and the persona.

mod artifact;
mod cache;
mod doc;
mod headers;
mod methods;
mod objects;
pub mod php;
pub mod profiles;
mod scope;

pub use artifact::{Artifact, Continuation, StreamKind};
pub use cache::DocCache;
pub use doc::{Doc, Origin, ext_of};
pub use methods::{Gate, gate};
pub use scope::{Req, Scope, ser};

use rand::RngExt;

/// A static file's origin; its mtime is a stable point before the epoch
/// (or, for live logs, moments before the log epoch).
pub fn static_origin(scope: &Scope, salt: &str) -> Origin {
    let w = &scope.world;
    let mut rng = w.cfg_rng(&format!("mtime:{salt}"), &scope.req.path);
    let mtime = if salt == "log" {
        w.epochs.log_anchor - rng.random_range(0..600)
    } else {
        w.epochs.cfg_anchor - rng.random_range(3_600..40 * 86_400)
    };
    Origin::Static { ext: ext_of(&scope.req.path), mtime }
}

/// Attach headers and freeze the stream parameters.
pub fn finish(scope: &Scope, doc: Doc, fixed: bool) -> Artifact {
    let fixed_length = fixed || (doc.stream.is_none() && matches!(doc.origin, Origin::Static { .. } | Origin::Server));
    let headers = headers::build(scope, &doc);
    Artifact {
        status: doc.status,
        headers,
        head: doc.head,
        stream: doc.stream.map(|(kind, blocks)| Continuation { kind, blocks, req: scope.req.clone(), epochs: scope.world.epochs }),
        tail: doc.tail,
        fixed_length,
        fast: false,
    }
}

/// Render the response for a request routed to `profile`.
pub fn render(scope: &Scope, profile: crate::config::Profile, method: &http::Method, cache: &DocCache) -> Artifact {
    use crate::config::Profile;
    use profiles::{api, assets, config, errors, feeds, logs, pages, secrets, site, sql, vcs};
    let doc = match gate(scope, profile, method) {
        Gate::Answer(doc) => doc,
        Gate::ApiUnauthorized => api::unauthorized(scope),
        Gate::Render => {
            let post = method == http::Method::POST;
            let doc = match profile {
                Profile::Env => config::render(scope),
                Profile::Git => vcs::render(scope),
                // Dumps cost milliseconds to render; the bytes depend only on
                // the file and the config epoch, like a file on disk.
                Profile::SqlDump => {
                    let key = format!("sql|{}|{}|{}|{}", scope.req.scheme, scope.req.host, scope.req.path, scope.world.epochs.cfg);
                    cache.get_or_render(key, || sql::render(scope))
                }
                Profile::Log => logs::render(scope),
                Profile::Login => pages::login(scope, post),
                Profile::PhpInfo => pages::phpinfo(scope),
                Profile::ServerStatus => pages::server_status(scope),
                Profile::Joomla => pages::joomla_manifest(scope),
                Profile::Drupal => pages::drupal_changelog(scope),
                Profile::Json => api::render(scope),
                Profile::Sitemap => feeds::sitemap(scope),
                Profile::Rss => feeds::rss(scope),
                Profile::Secrets => secrets::render(scope),
                Profile::Labyrinth => site::render(scope),
                Profile::NotFound => errors::not_found(scope),
                Profile::Forbidden => errors::forbidden(scope),
                Profile::Blank => pages::blank(scope, post),
                Profile::Asset => assets::render(scope),
            };
            php::booted(scope, profile, doc)
        }
    };
    let mut art = finish(scope, doc, false);
    // Static files go out at once, as a front server sends them.
    art.fast = profile == Profile::Asset && art.status.is_success();
    art
}

/// robots.txt is bait that must be read: served without tarpitting.
pub fn robots(scope: &Scope) -> Artifact {
    let mut art = finish(scope, profiles::pages::robots(scope), true);
    art.fast = true;
    art
}

/// Block `seq` of a streamed artifact.
pub fn block(scope: &Scope, kind: StreamKind, seq: u64) -> Vec<u8> {
    use profiles::{api, feeds, logs, sql};
    match kind {
        StreamKind::Log(k) => logs::block(scope, k, seq),
        StreamKind::Sql => sql::block(scope, seq),
        StreamKind::Json => api::block(scope, seq),
        StreamKind::Sitemap => feeds::sitemap_block(scope, seq),
    }
}

#[cfg(test)]
mod tests;
