//! The labyrinth, dressed as the persona's own website: content pages in a
//! theme of the persona's (or its CMS's) look, directory listings in the
//! front server's autoindex format. Every link resolves.

use minijinja::context;
use rand::RngExt;

use super::errors::{server_page, server_redirect};
use super::pages::newest_posts;
use crate::persona::{Area, AreaKind, PhpPlace, capitalize};
use crate::render::doc::{Doc, Origin};
use crate::render::php;
use crate::render::scope::{Scope, ser};
use crate::world::content::{self, Entry};

pub fn render(scope: &Scope) -> Doc {
    let p = scope.persona();
    let path = scope.req.path.to_ascii_lowercase();
    let area = p
        .site
        .areas
        .iter()
        .filter(|a| path.starts_with(&a.prefix.to_ascii_lowercase()) || path == a.prefix.trim_end_matches('/').to_ascii_lowercase())
        .max_by_key(|a| a.prefix.len());
    let is_leak_dir = || p.leak_dirs().iter().any(|d| d.eq_ignore_ascii_case(&scope.req.path));
    match area {
        // Directories are only served with their trailing slash.
        Some(a) if a.kind == AreaKind::Autoindex && !scope.req.path.ends_with('/') => {
            let query = scope.req.query.as_deref().map(|q| format!("?{q}")).unwrap_or_default();
            server_redirect(scope, &format!("{}/{query}", scope.url(&scope.req.path)))
        }
        Some(a) if a.kind == AreaKind::Autoindex => autoindex(scope, content::listing(&scope.world, &scope.req.path)),
        None if is_leak_dir() => autoindex(scope, content::leftovers(&scope.world, &scope.req.path)),
        _ => page(scope, area),
    }
}

fn autoindex(scope: &Scope, entries: Vec<Entry>) -> Doc {
    let dir = if scope.req.path.ends_with('/') { scope.req.path.clone() } else { format!("{}/", scope.req.path) };
    let parent = match dir.trim_end_matches('/').rfind('/') {
        Some(i) => dir[..=i].to_string(),
        None => "/".to_string(),
    };
    let body = server_page(scope, "autoindex.html.j2", context! { dir => dir, parent => parent, entries => ser(&entries) });
    Doc::new(Origin::Server, Some("text/html"), body)
}

/// Themes available in the catalog (`site/themes/*.html.j2`); overlays may
/// add their own.
fn theme(scope: &Scope) -> String {
    let own = scope.stack_template("page.html.j2");
    if scope.catalog.has(&own) && !own.starts_with("stacks/default/") {
        return own;
    }
    let mut themes: Vec<String> = scope.catalog.template_names().into_iter().filter(|n| n.starts_with("site/themes/")).collect();
    themes.sort();
    let i = scope.world.fixed_rng("theme").random_range(0..themes.len().max(1));
    themes.get(i).cloned().unwrap_or(own)
}

fn page(scope: &Scope, area: Option<&Area>) -> Doc {
    let p = scope.persona();
    let mut r = scope.world.log_rng("page", &scope.req.path);
    let home = area.is_none();
    let trimmed = scope.req.path.trim_end_matches('/');
    let slug = trimmed.rsplit('/').next().unwrap_or("");
    let heading = if home || slug.is_empty() { p.company.clone() } else { slug.split('-').map(capitalize).collect::<Vec<_>>().join(" ") };
    let paragraphs: Vec<String> = (0..r.random_range(1..4))
        .map(|_| {
            let n = r.random_range(2..6);
            p.paragraph(&mut r, n)
        })
        .collect();

    // Deeper links: children of this page, or recent posts at the top level.
    let base = if home { p.site.areas.first().map(|a| a.prefix.clone()).unwrap_or_else(|| "/".into()) } else { format!("{trimmed}/") };
    let posts: Vec<content::Post> = if home {
        newest_posts(scope, r.random_range(4..12))
    } else {
        let seed = r.random_range(1..5_000u64);
        (0..r.random_range(4..14)).map(|k| content::post(&scope.world, seed + k * 7)).collect()
    };
    let links: Vec<_> = posts
        .iter()
        .map(|post| context! { href => format!("{base}{}/", post.slug), title => post.title.clone(), time => post.time })
        .collect();
    let nav: Vec<_> = p
        .site
        .areas
        .iter()
        .map(|a| context! { href => a.prefix.clone(), label => capitalize(&a.prefix.trim_matches('/').rsplit('/').next().unwrap_or("").replace('-', " ")) })
        .collect();
    let body = scope.render_or_empty(
        &theme(scope),
        context! {
            home => home,
            heading => heading,
            paragraphs => paragraphs,
            links => links,
            nav => nav,
            tagline => p.sentence(&mut scope.world.fixed_rng("tagline"), 12),
            php_body => php::output(scope, PhpPlace::Body),
        },
    );
    Doc::new(Origin::App { session: true }, Some("text/html; charset=UTF-8"), body)
}
