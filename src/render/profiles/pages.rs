//! Single pages: stack-native login screens, phpinfo, Apache server-status,
//! version disclosures, silent PHP scripts, robots.txt.

use http::StatusCode;
use minijinja::context;
use rand::RngExt;

use super::errors::{app_redirect, server_page, server_redirect};
use crate::render::doc::{Doc, Origin};
use crate::render::scope::{Scope, ser};
use crate::render::static_origin;
use crate::util::{self, Tm};
use crate::world::{content, traffic};

pub fn login(scope: &Scope, failed: bool) -> Doc {
    let p = scope.persona();
    let path = scope.req.path.as_str();
    // Protected areas bounce to the login form, as the real thing does.
    match p.stack.id() {
        "wordpress" if path.starts_with("/wp-admin") => {
            let back = urlencode(&scope.url(path));
            return app_redirect(scope, &scope.url(&format!("/wp-login.php?redirect_to={back}&reauth=1")));
        }
        "django" if (path == "/admin/" || path == "/admin") && p.site.login_path != path => {
            return app_redirect(scope, &format!("{}?next=/admin/", p.site.login_path));
        }
        "joomla" if path == "/administrator" => return server_redirect(scope, &scope.url("/administrator/")),
        // Laravel answers a failed login with a redirect back to the form.
        "laravel" if failed => return app_redirect(scope, &scope.url(path)),
        _ => {}
    }
    let body = scope.render_or_empty(&scope.stack_template("login.html.j2"), context! { failed => failed, action => path });
    let mut doc = Doc::new(Origin::App { session: true }, Some("text/html; charset=UTF-8"), body);
    if p.stack.id() == "wordpress" {
        doc = doc.header("set-cookie", "wordpress_test_cookie=WP%20Cookie%20check; path=/; secure");
    }
    doc
}

pub fn phpinfo(scope: &Scope) -> Doc {
    let body = scope.render_or_empty("pages/phpinfo.html.j2", context! {});
    Doc::new(Origin::App { session: false }, Some("text/html; charset=UTF-8"), body)
}

pub fn server_status(scope: &Scope) -> Doc {
    let w = &scope.world;
    let mut r = w.log_rng("server-status", "");
    let workers: Vec<_> = (0..r.random_range(4..14))
        .map(|i| {
            context! {
                slot => i,
                pid => r.random_range(1000..40_000),
                acc => format!("{}/{}/{}", r.random_range(0..40), r.random_range(0..900), r.random_range(1000..90_000)),
                mode => util::pick(&mut r, &["W", "_", "K", "_", "R"]),
                cpu => format!("{:.2}", r.random_range(0.0..9.0)),
                ss => r.random_range(0..300),
                req => r.random_range(0..900),
                client => util::public_ipv4(&mut r),
                path => traffic::probe(w, &mut r),
            }
        })
        .collect();
    let uptime = w.epochs.log_anchor - w.epochs.cfg_anchor + r.random_range(3_600..900_000);
    let body = server_page(
        scope,
        "server-status.html.j2",
        context! {
            workers => workers,
            uptime => uptime,
            accesses => r.random_range(100_000..9_000_000),
            traffic_gb => format!("{:.1}", r.random_range(1.0..400.0)),
            busy => r.random_range(1..12),
            idle => r.random_range(4..40),
            internal_ip => w.persona.internal_ip(10),
        },
    );
    Doc::new(Origin::Server, Some("text/html; charset=ISO-8859-1"), body)
}

pub fn joomla_manifest(scope: &Scope) -> Doc {
    let built = Tm::at(scope.world.epochs.cfg_anchor - 120 * 86_400, 0);
    let body = scope.render_or_empty("stacks/joomla/manifest.xml.j2", context! { year => built.year, month => built.month });
    Doc::new(static_origin(scope, "manifest"), None, body)
}

pub fn drupal_changelog(scope: &Scope) -> Doc {
    let releases: Vec<_> = (0..6).map(|i| ser(&(scope.world.epochs.cfg_anchor - i * 60 * 86_400))).collect();
    let body = scope.render_or_empty("stacks/drupal/CHANGELOG.txt.j2", context! { release_dates => releases });
    Doc::new(static_origin(scope, "changelog"), None, body)
}

/// A script that runs but prints nothing (`wp-config.php`), or a handful of
/// well-known tiny endpoints (`xmlrpc.php`, health checks).
pub fn blank(scope: &Scope, post: bool) -> Doc {
    let path = scope.req.path.to_ascii_lowercase();
    let file = path.rsplit('/').next().unwrap_or("").to_string();
    let name = scope.stack_template(&format!("blank/{file}.j2"));
    if !scope.catalog.has(&name) {
        return Doc::new(Origin::App { session: false }, Some("text/html; charset=UTF-8"), Vec::new());
    }
    let body = scope.render_or_empty(&name, context! { post => post });
    if file == "xmlrpc.php" && !post {
        return Doc::new(Origin::App { session: false }, Some("text/plain;charset=UTF-8"), body)
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .header("allow", "POST");
    }
    let ct = if file == "xmlrpc.php" { "text/xml; charset=UTF-8" } else { "text/html; charset=utf-8" };
    Doc::new(Origin::App { session: false }, Some(ct), body)
}

pub fn robots(scope: &Scope) -> Doc {
    let p = scope.persona();
    let mut r = scope.world.fixed_rng("robots");
    let baits = p.robots_baits();
    let take = r.random_range(2..baits.len().max(3) + 1).min(baits.len());
    let sitemap = p.site.sitemap.as_ref().filter(|_| r.random_bool(0.8) || p.stack.id() == "wordpress").map(|s| scope.url(s));
    let body =
        scope.render_or_empty(&scope.stack_template("robots.txt.j2"), context! { disallow => baits[..take].to_vec(), sitemap => sitemap });
    Doc::new(static_origin(scope, "robots"), None, body)
}

/// Feed for `/` and other content pages.
pub fn newest_posts(scope: &Scope, count: u64) -> Vec<content::Post> {
    let newest = content::newest(&scope.world);
    (0..count).filter_map(|k| newest.checked_sub(k)).filter(|n| *n > 0).map(|n| content::post(&scope.world, n)).collect()
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
