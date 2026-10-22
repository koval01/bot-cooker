//! The fingerprint audit: one clue shared by many instances would give the
//! trap away, so no line of our own making may show up on many instances.
//!
//! Lines real software writes identically on every install (an nginx 404,
//! mysqldump boilerplate, a framework's login form) are fine: a scanner sees
//! those on every real host too. They are recognised as the literal lines of
//! templates that reproduce such software, plus a few standard-format tokens.

use std::collections::{HashMap, HashSet};

use http::Method;

use super::Engine;
use super::tests::{catalog, engine, fetch_with, keys};
use crate::config::Profile;
use crate::render::Artifact;

/// Templates that reproduce real software output verbatim.
const VERBATIM_DIRS: &[&str] = &["servers/", "sql/", "stacks/", "pages/", "vcs/", "secrets/", "api/", "logs/access/iis_header"];

/// Responses that are pure software boilerplate wherever they occur.
const BOILERPLATE: &[Profile] = &[
    Profile::NotFound,
    Profile::Forbidden,
    Profile::Login,
    Profile::PhpInfo,
    Profile::ServerStatus,
    Profile::Joomla,
    Profile::Drupal,
    Profile::Blank,
];

/// Standard-format lines (sitemap/feed/listing vocabulary, framework and git
/// defaults) that real sites share by definition.
fn standard(line: &str) -> bool {
    // Laravel's stock .env line and git's branch tracking entry.
    const LINES: &[&str] = &["MAIL_FROM_NAME=\"${APP_NAME}\""];
    if LINES.contains(&line) || line.starts_with("merge = refs/heads/") {
        return true;
    }
    const PREFIXES: &[&str] = &[
        "<urlset ",
        "<sitemapindex ",
        "<priority>",
        "<changefreq>",
        "<rss version=",
        "<feed xmlns=",
        "<head><title>Index of ",
        "<h1>Index of ",
        "<title>Index of ",
    ];
    PREFIXES.iter().any(|p| line.starts_with(p)) || (line.starts_with("<loc>") && line.contains("/sitemap"))
}

fn verbatim_lines() -> HashSet<String> {
    let c = catalog();
    let mut out = HashSet::new();
    for name in c.template_names() {
        if VERBATIM_DIRS.iter().any(|d| name.starts_with(d)) {
            for line in c.source(&name).unwrap_or_default().lines() {
                if !line.contains("{{") && !line.contains("{%") {
                    out.insert(line.trim().to_string());
                }
            }
        }
    }
    out
}

/// The lines of our own making an instance serves, on a Host of its own
/// (as in a real deployment). Wildcards get a slug of the instance's own:
/// pages echo the requested slug, which is the scanner's choice, not ours.
fn surface(d: &Engine, host: &str, slug: &str) -> HashSet<String> {
    let inc = d.current();
    let mut paths: Vec<String> = inc.persona.routes().iter().map(|r| r.pattern.replace('*', slug)).filter(|p| p.starts_with('/')).collect();
    paths.extend(["/robots.txt".to_string(), format!("{}/users?page=2", inc.persona.site.api_prefix)]);
    let mut lines = HashSet::new();
    for p in paths {
        let profile = inc.router.profile_for(&p);
        if BOILERPLATE.contains(&profile) {
            continue;
        }
        let (art, body) = fetch_with(d, Method::GET, &p, Some(host), 3);
        let body = if profile == Profile::SqlDump { rows_only(&art, &body) } else { body };
        lines.extend(body.lines().map(|l| l.trim().to_string()).filter(|l| l.len() >= 24));
    }
    lines
}

/// A dump's DDL and footer are the schema's and the tool's; its rows are ours.
fn rows_only(art: &Artifact, body: &str) -> String {
    let head = String::from_utf8_lossy(&art.head).len();
    let tail = String::from_utf8_lossy(&art.tail).len();
    body.get(head..body.len().saturating_sub(tail)).unwrap_or_default().to_string()
}

/// 15% of instances and lines of 24+ characters: below that, what remains
/// shared is what every real install shares as well (`ref: refs/heads/main`,
/// `NODE_ENV=production`, WordPress body classes).
#[test]
fn no_line_of_our_own_is_shared_across_instances() {
    let n = 40;
    let surfaces: Vec<HashSet<String>> =
        keys(n).iter().enumerate().map(|(i, k)| surface(&engine(k), &format!("node-{i}.example.net"), &format!("page-{i}"))).collect();
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for s in &surfaces {
        for line in s {
            *seen.entry(line.as_str()).or_default() += 1;
        }
    }
    let allowed = verbatim_lines();
    let threshold = n * 15 / 100;
    let mut shared: Vec<(&str, usize)> =
        seen.into_iter().filter(|(line, count)| *count > threshold && !allowed.contains(*line) && !standard(line)).collect();
    shared.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let report: Vec<String> = shared.iter().take(50).map(|(l, c)| format!("{c:>3}x  {}", &l[..l.len().min(140)])).collect();
    assert!(shared.is_empty(), "{} shared lines, e.g.:\n{}", shared.len(), report.join("\n"));
}

#[test]
fn epoch_boundaries_are_instance_specific() {
    let anchors: HashSet<i64> = keys(10).iter().map(|k| engine(k).epochs().cfg_anchor).collect();
    assert!(anchors.len() >= 9, "instances rotate in lockstep");
    for k in keys(10) {
        let e = engine(&k).epochs();
        assert!(e.cfg_anchor <= e.now && e.log_anchor <= e.now);
        assert!(e.now - e.cfg_anchor <= 7 * 86_400 + 6 * 3_600, "anchor too far back");
    }
}

/// Prose is where shared text would be easiest to spot: sentences must
/// rarely be verbatim corpus sentences, and rarely repeat across instances.
#[test]
fn prose_is_not_shared() {
    let data = crate::data::Data::embedded();
    let corpus: HashSet<String> =
        data.corpus.values().flat_map(|t| crate::text::sentences(t)).map(|s| s.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    let mut owners: HashMap<String, HashSet<usize>> = HashMap::new();
    let mut verbatim = 0;
    let mut total = 0;
    for (i, k) in keys(20).iter().enumerate() {
        let e = engine(k);
        let inc = e.current();
        let mut rng = crate::util::keyed_rng(b"prose", &[k.as_bytes()]);
        for _ in 0..150 {
            let s = inc.persona.text.sentence(&mut rng, 40);
            total += 1;
            if corpus.contains(&s) {
                verbatim += 1;
            }
            let voiced = inc.persona.sentence(&mut rng, 40);
            owners.entry(voiced).or_default().insert(i);
        }
    }
    assert!(verbatim * 100 < total * 15, "{verbatim}/{total} raw sentences quote the corpus");
    let widely = owners.values().filter(|o| o.len() > 3).count();
    assert!(widely * 100 < owners.len() * 2, "{widely} of {} sentences appear on 4+ instances", owners.len());
}
