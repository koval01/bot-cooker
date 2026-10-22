//! Sitemaps and feeds. Every URL points at a page that really renders on this
//! instance (a content-area page), on the Host the client asked for.

use minijinja::context;
use rand::RngExt;
use serde::Serialize;

use super::pages::newest_posts;
use crate::persona::AreaKind;
use crate::render::artifact::StreamKind;
use crate::render::doc::{Doc, Origin};
use crate::render::scope::{Scope, ser};
use crate::render::static_origin;
use crate::world::content;

const URLS_PER_BLOCK: u64 = 12;
const URLS_PER_SITEMAP: u64 = 2_000;

fn origin(scope: &Scope) -> Origin {
    if scope.persona().cms { Origin::App { session: false } } else { static_origin(scope, "sitemap") }
}

fn content_type(scope: &Scope) -> &'static str {
    if scope.persona().cms { "application/xml; charset=UTF-8" } else { "text/xml" }
}

fn url_total(scope: &Scope) -> u64 {
    scope.world.fixed_rng("sitemap-size").random_range(40..8_000)
}

pub fn sitemap(scope: &Scope) -> Doc {
    let p = scope.persona();
    let wp = p.stack.id() == "wordpress";
    let index_path = p.site.sitemap.clone().unwrap_or_else(|| "/sitemap.xml".into());
    let is_index =
        index_path.eq_ignore_ascii_case(&scope.req.path) && (scope.req.path.contains("index") || wp || url_total(scope) > URLS_PER_SITEMAP);
    if is_index {
        let stem = index_path.trim_end_matches(".xml").trim_end_matches("_index").to_string();
        let count = url_total(scope).div_ceil(URLS_PER_SITEMAP).max(1);
        let parts: Vec<String> = if wp {
            let mut v: Vec<String> = (1..=count).map(|n| format!("{stem}-posts-post-{n}.xml")).collect();
            v.extend(["posts-page-1", "taxonomies-category-1", "users-1"].map(|s| format!("{stem}-{s}.xml")));
            v
        } else {
            (1..=count).map(|n| format!("{stem}-{n}.xml")).collect()
        };
        let body = scope.render_or_empty("site/sitemap_index.xml.j2", context! { parts => parts });
        return Doc::new(origin(scope), Some(content_type(scope)), body);
    }
    let head = scope.render_or_empty("site/urlset_head.xml.j2", context! {});
    let tail = scope.render_or_empty("site/urlset_tail.xml.j2", context! {});
    let n = url_total(scope).min(URLS_PER_SITEMAP);
    Doc::new(origin(scope), Some(content_type(scope)), head).streamed(StreamKind::Sitemap, n.div_ceil(URLS_PER_BLOCK), tail)
}

#[derive(Serialize)]
struct Url {
    n: u64,
    path: String,
    time: i64,
}

pub fn sitemap_block(scope: &Scope, seq: u64) -> Vec<u8> {
    let p = scope.persona();
    // Child sitemaps continue where their siblings stopped.
    let part: u64 = scope.req.path.trim_end_matches(".xml").rsplit('-').next().and_then(|s| s.parse().ok()).unwrap_or(1).max(1);
    let pages: Vec<_> = p.site.areas.iter().filter(|a| a.kind != AreaKind::Autoindex).collect();
    let pool = if pages.is_empty() { p.site.areas.iter().collect() } else { pages };
    if pool.is_empty() {
        return Vec::new();
    }
    let urls: Vec<Url> = (0..URLS_PER_BLOCK)
        .map(|i| {
            let n = (part - 1) * URLS_PER_SITEMAP + seq * URLS_PER_BLOCK + i + 1;
            let post = content::post(&scope.world, n);
            Url { n, path: format!("{}{}/", pool[(n as usize) % pool.len()].prefix, post.slug), time: post.time }
        })
        .collect();
    scope.render_or_empty("site/urlset_urls.xml.j2", context! { urls => ser(&urls) }).into_bytes()
}

#[derive(Serialize)]
struct FeedItem {
    #[serde(flatten)]
    post: content::Post,
    summary: String,
    author: String,
    author_login: String,
    category: String,
}

pub fn rss(scope: &Scope) -> Doc {
    let p = scope.persona();
    let area = p
        .site
        .areas
        .iter()
        .find(|a| a.kind == AreaKind::Blog)
        .or(p.site.areas.first())
        .map(|a| a.prefix.clone())
        .unwrap_or_else(|| "/".into());
    let count = 10 + scope.world.fixed_rng("rss-count").random_range(0..11);
    let posts: Vec<FeedItem> = newest_posts(scope, count)
        .into_iter()
        .map(|post| {
            let mut r = scope.world.log_rng("rss-item", &post.slug);
            let author = &p.staff[(post.n as usize) % p.staff.len()];
            FeedItem {
                summary: p.sentence(&mut r, 30),
                author: author.display(),
                author_login: author.login.clone(),
                category: crate::persona::capitalize(&p.site.vocabulary[(post.n as usize) % p.site.vocabulary.len()].replace('-', " ")),
                post,
            }
        })
        .collect();
    let atom = scope.req.path.contains("atom");
    let name = if atom {
        "site/atom.xml.j2".to_string()
    } else {
        scope.pick(&[format!("stacks/{}/rss.xml.j2", p.stack.id()), "site/rss.xml.j2".into()]).unwrap_or_default()
    };
    let tagline = p.sentence(&mut scope.world.fixed_rng("tagline"), 12);
    let body = scope.render_or_empty(&name, context! { posts => ser(&posts), area => area, tagline => tagline });
    let ct = if atom { "application/atom+xml" } else { "application/rss+xml; charset=UTF-8" };
    Doc::new(Origin::App { session: false }, Some(ct), body)
}
