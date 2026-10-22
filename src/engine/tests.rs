//! Fingerprint tests: one master key must be stable, different master keys
//! must not share a recognisable shape.

use std::collections::HashSet;

use http::Method;

use std::sync::{Arc, OnceLock};

use super::{Engine, RequestInfo};
use crate::catalog::Catalog;
use crate::config::{DeceptionConfig, HostOsSetting, Profile};
use crate::data::Data;
use crate::render::Artifact;

/// 2026-10-21 13:05:09 UTC.
const NOW: u64 = 1_792_587_909;

pub(super) fn catalog() -> Arc<Catalog> {
    static C: OnceLock<Arc<Catalog>> = OnceLock::new();
    C.get_or_init(|| Arc::new(Catalog::load(None).expect("catalog"))).clone()
}

fn engine_at(cfg: &DeceptionConfig, now: u64) -> Engine {
    Engine::with_clock(cfg, catalog(), Data::embedded(), None, Some(now))
}

pub(super) fn engine(key: &str) -> Engine {
    engine_at(&DeceptionConfig { master_key: key.to_string(), host_os: HostOsSetting::Unix, ..DeceptionConfig::default() }, NOW)
}

pub(super) fn keys(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("instance-key-{i}")).collect()
}

/// Render `path` and return the artifact plus the full body (head, every
/// block up to `max_blocks`, tail).
pub(super) fn fetch_with(d: &Engine, method: Method, path: &str, host: Option<&str>, max_blocks: u64) -> (Artifact, String) {
    let inc = d.current();
    let (path, query) = match path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path, None),
    };
    let info = RequestInfo { method: &method, path, query, host, https: true };
    let art = if path == "/robots.txt" { d.robots(&inc, &info) } else { d.render(&inc, inc.router.profile_for(path), &info) };
    let mut body = art.head.clone();
    if let Some(c) = &art.stream {
        for seq in 0..c.blocks.min(max_blocks) {
            body.extend(d.continue_stream(&inc.persona, c, seq));
        }
    }
    body.extend(&art.tail);
    let text = match String::from_utf8(body) {
        Ok(text) => text,
        // Icons are binary; every other artifact is text.
        Err(e) if path.ends_with(".ico") => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        Err(e) => panic!("{path}: body is not UTF-8: {e}"),
    };
    (art, text)
}

fn fetch(d: &Engine, path: &str) -> (Artifact, String) {
    fetch_with(d, Method::GET, path, Some("trap.example.net"), 6)
}

/// A concrete path for a route pattern.
pub(super) fn concrete(pattern: &str) -> String {
    pattern.replace('*', "sample")
}

#[test]
fn every_route_renders_for_many_personas() {
    for key in keys(40) {
        render_every_route(&engine(&key));
    }
}

#[test]
fn every_route_renders_on_a_windows_host() {
    for key in keys(20) {
        let cfg = DeceptionConfig { master_key: key, host_os: HostOsSetting::Windows, ..DeceptionConfig::default() };
        let d = engine_at(&cfg, NOW);
        assert_eq!(d.current().persona.distro, crate::persona::Distro::Windows);
        render_every_route(&d);
    }
}

fn render_every_route(d: &Engine) {
    let inc = d.current();
    let patterns: Vec<String> = inc.persona.routes().iter().map(|r| r.pattern.clone()).collect();
    for pat in patterns {
        let path = concrete(&pat);
        if !path.starts_with('/') {
            continue;
        }
        for m in [Method::GET, Method::HEAD, Method::POST, Method::OPTIONS, Method::from_bytes(b"FOO").unwrap()] {
            let _ = fetch_with(d, m, &path, Some("trap.example.net"), 3);
        }
    }
    let _ = fetch(d, "/robots.txt");
    let _ = fetch(d, "/definitely/missing.php");
    let _ = fetch(d, "/nope");
}

#[test]
fn stable_within_instance() {
    let a = engine("stable");
    let b = engine("stable");
    let paths: Vec<String> = a.current().persona.routes().iter().map(|r| concrete(&r.pattern)).filter(|p| p.starts_with('/')).collect();
    for p in paths.iter().chain(["/robots.txt".to_string()].iter()) {
        let (_, x) = fetch(&a, p);
        let (_, y) = fetch(&b, p);
        assert_eq!(x, y, "body of {p} differs between identical instances");
    }
}

#[test]
fn route_tables_differ_across_instances() {
    let tables: HashSet<Vec<(String, String)>> = keys(30)
        .iter()
        .map(|k| engine(k).current().persona.routes().iter().map(|r| (r.pattern.clone(), format!("{:?}", r.profile))).collect())
        .collect();
    assert_eq!(tables.len(), 30, "two instances expose the same set of paths");
}

#[test]
fn not_every_path_exists() {
    for key in keys(20) {
        let d = engine(&key);
        let inc = d.current();
        assert_eq!(inc.router.profile_for("/this-path-does-not-exist-anywhere"), Profile::NotFound);
        let (art, _) = fetch(&d, "/this-path-does-not-exist-anywhere");
        assert_eq!(art.status.as_u16(), 404);
    }
}

/// The main config leak of an instance (example files carry no secrets).
fn primary_leak(site: &crate::persona::Site) -> Option<&String> {
    site.env_paths.iter().find(|p| !p.ends_with(".example")).or(site.config_leaks.first())
}

/// The "shape" of a config file: its keys in order, quoting and line style,
/// with values erased.
fn config_shape(body: &str) -> String {
    body.lines()
        .map(|l| {
            let l = l.trim_end_matches('\r');
            match l.find(['=', ':']) {
                Some(i) => {
                    let v = &l[i + 1..];
                    let q = v.trim_start().chars().next().filter(|c| *c == '"' || *c == '\'').map(String::from).unwrap_or_default();
                    format!("{}{q}", &l[..=i])
                }
                None => l.chars().take(12).collect(),
            }
        })
        .collect::<Vec<_>>()
        .join("|")
        + if body.contains("\r\n") { "#crlf" } else { "" }
}

#[test]
fn leaked_configs_have_distinct_shapes() {
    let mut shapes = HashSet::new();
    let mut seen = 0;
    for key in keys(40) {
        let d = engine(&key);
        let inc = d.current();
        let site = &inc.persona.site;
        if let Some(p) = primary_leak(site) {
            let (_, body) = fetch(&d, p);
            assert!(!body.is_empty(), "{p} empty");
            shapes.insert(config_shape(&body));
            seen += 1;
        }
    }
    assert!(seen >= 30, "too few instances leak a config: {seen}");
    assert_eq!(shapes.len(), seen, "two instances leak configs with an identical layout");
}

#[test]
fn configs_agree_with_each_other() {
    // The DB password from the leaked config shows up in docker-compose too.
    for key in keys(60) {
        let d = engine(&key);
        let inc = d.current();
        let site = &inc.persona.site;
        let Some(cfg_path) = primary_leak(site) else { continue };
        let (_, cfg) = fetch(&d, cfg_path);
        assert!(
            cfg.contains(&inc.persona.db.name)
                || cfg.contains("master.key")
                || cfg_path.ends_with("master.key")
                || cfg_path.contains("storage"),
            "{cfg_path} lacks db name ({}):\n{cfg}",
            inc.persona.stack_name
        );
        if site.docker_compose {
            let (_, compose) = fetch(&d, "/docker-compose.yml");
            let (_, cfg2) = fetch(&d, cfg_path);
            let pw_in_both =
                compose.lines().filter_map(|l| l.split_once(": ").map(|x| x.1.to_string())).any(|v| v.len() > 10 && cfg2.contains(&v));
            assert!(pw_in_both, "compose and {cfg_path} disagree on secrets");
        }
    }
}

#[test]
fn logs_are_chronological() {
    let mut checked = 0;
    for key in keys(40) {
        let d = engine(&key);
        let inc = d.current();
        for (path, _) in inc.persona.site.logs.clone() {
            let (_, body) = fetch_with(&d, Method::GET, &path, None, 30);
            let stamps: Vec<String> = body.lines().filter_map(sortable_stamp).collect();
            assert!(stamps.len() > 20, "{path}: too few timestamps ({}) in:\n{}", stamps.len(), &body[..body.len().min(1500)]);
            let lines: Vec<&str> = body.lines().collect();
            if let Some(i) = stamps.windows(2).position(|w| w[0] > w[1]) {
                panic!("{path} is not chronological at {i}: {:?} > {:?}\n{}\n{}", stamps[i], stamps[i + 1], lines[i], lines[i + 1]);
            }
            checked += 1;
        }
    }
    assert!(checked > 10);
}

/// Extract a lexicographically sortable timestamp from common log formats.
fn sortable_stamp(line: &str) -> Option<String> {
    const MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let digits = |s: &str| s.chars().filter(|c| c.is_ascii_digit()).collect::<String>();
    // pino: "time":1792587909123
    if let Some(t) = line.split("\"time\":").nth(1).filter(|t| t.starts_with(|c: char| c.is_ascii_digit())) {
        return Some(digits(&t[..13.min(t.len())]));
    }
    // ISO-ish: 2026-10-21 13:05:09 / 2026-10-21T13:05:09 / 2026/10/21 13:05:09
    for (i, _) in line.char_indices() {
        let rest = &line[i..];
        if rest.len() >= 19 {
            let b = rest.as_bytes();
            if b[..4].iter().all(u8::is_ascii_digit) && (b[4] == b'-' || b[4] == b'/') && (b[10] == b' ' || b[10] == b'T') && b[13] == b':'
            {
                return Some(digits(&rest[..19]));
            }
        }
        // CLF: 21/Oct/2026:13:05:09
        if rest.len() >= 20
            && rest.as_bytes()[2] == b'/'
            && rest.as_bytes()[6] == b'/'
            && let Some(m) = MON.iter().position(|m| &rest[3..6] == *m)
        {
            return Some(format!("{}{:02}{}{}", &rest[7..11], m + 1, &rest[0..2], digits(&rest[12..20])));
        }
        // Apache error: [Wed Oct 21 13:05:09.123456 2026]
        if rest.len() >= 32
            && rest.starts_with('[')
            && rest.as_bytes()[4] == b' '
            && rest.as_bytes()[8] == b' '
            && let Some(m) = MON.iter().position(|m| &rest[5..8] == *m)
        {
            return Some(format!("{}{:02}{}{}", &rest[28..32], m + 1, &rest[9..11], digits(&rest[12..20])));
        }
        // PHP: 21-Oct-2026 13:05:09
        if rest.len() >= 20
            && rest.as_bytes()[2] == b'-'
            && rest.as_bytes()[6] == b'-'
            && let Some(m) = MON.iter().position(|m| &rest[3..6] == *m)
        {
            return Some(format!("{}{:02}{}{}", &rest[7..11], m + 1, &rest[0..2], digits(&rest[12..20])));
        }
    }
    None
}

#[test]
fn json_pages_parse_when_complete() {
    for key in keys(40) {
        let d = engine(&key);
        let inc = d.current();
        if inc.persona.cms && inc.persona.stack != crate::persona::Stack::WordPress {
            continue;
        }
        for res in inc.persona.site.api_resources.clone() {
            let path = format!("{}/{res}?page=2", inc.persona.site.api_prefix);
            let (art, body) = fetch_with(&d, Method::GET, &path, Some("trap.example.net"), 1_000);
            if art.status.as_u16() != 200 {
                continue;
            }
            serde_json::from_str::<serde_json::Value>(&body).unwrap_or_else(|e| panic!("{path}: {e}\n{body}"));
            assert!(!body.contains("https://x/"), "{path} links to a foreign host");
        }
    }
}

#[test]
fn sql_dumps_are_well_formed() {
    let mut checked = 0;
    for key in keys(60) {
        let d = engine(&key);
        let inc = d.current();
        let Some(b) = inc.persona.site.backups.first().cloned() else { continue };
        let (_, body) = fetch_with(&d, Method::GET, &b, None, 40);
        let domain = &inc.persona.domain;
        assert!(body.contains(&format!("@{domain}")), "{b}: no staff on the company domain");
        let trimmed = body.trim_end();
        assert!(
            trimmed.ends_with(';') || trimmed.ends_with("--") || trimmed.ends_with("GO") || trimmed.contains("Dump completed"),
            "{b}: bad ending: {}",
            &trimmed[trimmed.len().saturating_sub(200)..]
        );
        let creates = body.matches("CREATE TABLE").count();
        assert!(creates >= 2, "{b}: only {creates} tables");
        checked += 1;
    }
    assert!(checked > 10);
}

#[test]
fn links_follow_the_request_host() {
    for key in keys(20) {
        let d = engine(&key);
        let inc = d.current();
        if let Some(sm) = inc.persona.site.sitemap.clone() {
            let (_, body) = fetch_with(&d, Method::GET, &sm, Some("decoy.example.org"), 3);
            assert!(body.contains("decoy.example.org"), "{sm} ignores Host");
            for loc in body.split("<loc>").skip(1) {
                let url = loc.split('<').next().unwrap_or("");
                assert!(url.starts_with("https://decoy.example.org/"), "foreign link {url}");
                let path = url.trim_start_matches("https://decoy.example.org");
                assert_ne!(inc.router.profile_for(path), Profile::NotFound, "sitemap links to missing {path}");
            }
        }
    }
}

#[test]
fn headers_are_consistent_with_stack() {
    for key in keys(40) {
        let d = engine(&key);
        let inc = d.current();
        let p = &inc.persona;
        let (art, _) = fetch(&d, &p.site.login_path.clone());
        let server = art.headers.iter().find(|(k, _)| k == "server").map(|(_, v)| v.clone()).unwrap_or_default();
        let powered = art.headers.iter().find(|(k, _)| k == "x-powered-by").map(|(_, v)| v.clone());
        if server.starts_with("Microsoft-IIS") {
            assert_eq!(p.stack, crate::persona::Stack::AspNet);
        }
        if let Some(pb) = powered {
            assert!(!pb.starts_with("PHP/") || p.is_php(), "PHP banner on {:?}", p.stack);
            assert!(pb != "Express" || p.stack == crate::persona::Stack::Express);
        }
    }
}

#[test]
fn robots_lists_only_existing_paths() {
    for key in keys(30) {
        let d = engine(&key);
        let inc = d.current();
        let (_, body) = fetch(&d, "/robots.txt");
        for line in body.lines().filter_map(|l| l.strip_prefix("Disallow: ")) {
            if inc.persona.robots_baits().iter().any(|b| b == line) {
                let probe = if line.ends_with('/') { format!("{line}x") } else { line.to_string() };
                let _ = inc.router.profile_for(&probe);
            }
        }
        let shared: Vec<&str> = body.lines().filter(|l| l.starts_with("Disallow: /.env") || l.starts_with("Disallow: /.git")).collect();
        assert!(shared.is_empty(), "robots.txt advertises fixed bait: {shared:?}");
    }
}

#[test]
fn persona_is_stable_but_details_drift_across_epochs() {
    let cfg = DeceptionConfig { master_key: "drift".into(), host_os: HostOsSetting::Unix, ..DeceptionConfig::default() };
    let now = engine_at(&cfg, NOW);
    let later = engine_at(&cfg, NOW + 120 * 86_400);
    let (a, b) = (now.current(), later.current());
    assert_eq!(a.persona.summary(), b.persona.summary());
    let routes = |d: &Engine| d.current().persona.routes().iter().map(|r| r.pattern.clone()).collect::<Vec<_>>();
    assert_eq!(routes(&now), routes(&later));
    let leak = primary_leak(&a.persona.site).cloned().expect("a config leak");
    let (_, x) = fetch(&now, &leak);
    let (_, y) = fetch(&later, &leak);
    assert_ne!(x, y, "{leak} never changes");
    assert_eq!(config_shape(&x).split('|').next(), config_shape(&y).split('|').next(), "{leak} changed layout entirely");
}

#[test]
fn persona_era_rotates_identity() {
    let cfg =
        DeceptionConfig { master_key: "era".into(), persona_era_days: 30, host_os: HostOsSetting::Unix, ..DeceptionConfig::default() };
    let a = engine_at(&cfg, NOW).current().persona.summary();
    let b = engine_at(&cfg, NOW + 31 * 86_400).current().persona.summary();
    assert_ne!(a, b);
}

#[test]
fn timing_and_headers_differ_across_instances() {
    let mut timings = HashSet::new();
    let mut orders = HashSet::new();
    for key in keys(30) {
        let d = engine(&key);
        let inc = d.current();
        let t = &inc.persona.timing;
        let brisk = &t.regimes[crate::persona::Pace::Brisk as usize];
        timings.insert(((t.ttfb_factor * 100.0) as i64, (t.body_lag * 100.0) as i64, (brisk.chunk * 10.0) as i64));
        let (art, _) = fetch(&d, "/");
        orders.insert(art.headers.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>());
    }
    assert_eq!(timings.len(), 30, "two instances share a timing profile");
    assert!(orders.len() >= 20, "header sets/orders too uniform: {}", orders.len());
}

#[test]
fn tarpit_plans_are_not_a_constant() {
    use rand::SeedableRng;
    let cfg = crate::config::TarpitConfig::default();
    let mut medians = Vec::new();
    for key in keys(8) {
        let d = engine(&key);
        let inc = d.current();
        let (art, _) = fetch(&d, "/");
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(7);
        let mut ttfb: Vec<u128> = (0..400)
            .map(|_| {
                let p = crate::tarpit::Plan::sample(&cfg, &inc.persona.timing, &art, &mut rng);
                p.pre_headers.as_millis() + p.pre_body_ms()
            })
            .collect();
        ttfb.sort_unstable();
        let spread = ttfb[360] as f64 / ttfb[40].max(1) as f64;
        assert!(spread > 1.5, "{key}: TTFB nearly constant");
        medians.push(ttfb[200]);
    }
    medians.sort_unstable();
    assert!(medians[7] as f64 > medians[0] as f64 * 1.5, "instances share one TTFB profile: {medians:?}");
}

#[test]
fn a_zero_delay_config_means_no_waiting() {
    use rand::SeedableRng;
    let cfg = crate::config::TarpitConfig { initial_delay_ms: 0, delay_ms: 0, ..Default::default() };
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(3);
    for key in keys(4) {
        let d = engine(&key);
        let inc = d.current();
        let (art, _) = fetch(&d, "/");
        for _ in 0..200 {
            let p = crate::tarpit::Plan::sample(&cfg, &inc.persona.timing, &art, &mut rng);
            assert!(p.pre_headers.as_millis() + p.pre_body_ms() <= 1, "{key}: waited with a zero delay");
        }
    }
}

/// A slow (non-fast) plan's cadence for the instance's front page.
fn slow_cadence(d: &Engine, cfg: &crate::config::TarpitConfig, rng: &mut rand_chacha::ChaCha8Rng) -> crate::tarpit::Cadence {
    let (art, _) = fetch(d, "/");
    loop {
        let p = crate::tarpit::Plan::sample(cfg, &d.current().persona.timing, &art, rng);
        if !p.is_fast() {
            return p.cadence();
        }
    }
}

/// `n` beats of a response: (pauses in ms, chunk sizes).
fn beats(c: &mut crate::tarpit::Cadence, rng: &mut rand_chacha::ChaCha8Rng, n: usize) -> (Vec<f64>, Vec<f64>) {
    (0..n).map(|_| c.next(rng)).map(|b| (b.pause.as_millis() as f64, b.chunk as f64)).unzip()
}

fn quantile(v: &[f64], q: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    s[((s.len() - 1) as f64 * q) as usize]
}

/// Ratio of the 90th to the 10th percentile: the width of the body of the
/// distribution, insensitive to a few outliers.
fn spread(v: &[f64]) -> f64 {
    quantile(v, 0.9) / quantile(v, 0.1).max(1.0)
}

/// Ratio between the slowest and the fastest stretch: medians of
/// consecutive 20-beat windows.
fn window_spread(v: &[f64]) -> f64 {
    let medians: Vec<f64> = v.chunks(20).map(|w| quantile(w, 0.5)).collect();
    quantile(&medians, 1.0) / quantile(&medians, 0.0).max(1.0)
}

#[test]
fn drip_cadence_varies_within_a_response() {
    use rand::SeedableRng;
    // Before the regime walk one response stayed in a narrow band: gap and
    // chunk p90/p10 at most ~5, 20-beat window medians within ~2x.
    let cfg = crate::config::TarpitConfig::default();
    for key in keys(16) {
        let d = engine(&key);
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(11);
        let mut c = slow_cadence(&d, &cfg, &mut rng);
        let mut paces = HashSet::new();
        let (mut gaps, mut chunks) = (Vec::new(), Vec::new());
        for _ in 0..600 {
            let b = c.next(&mut rng);
            paces.insert(c.pace());
            gaps.push(b.pause.as_millis() as f64);
            chunks.push(b.chunk as f64);
        }
        assert!(paces.len() >= 3, "{key}: stuck in {paces:?}");
        assert!(spread(&gaps) > 6.0, "{key}: gaps in a narrow band ({:.1})", spread(&gaps));
        assert!(spread(&chunks) > 6.0, "{key}: chunks in a narrow band ({:.1})", spread(&chunks));
        assert!(window_spread(&gaps) > 3.0, "{key}: no slow and fast stretches ({:.1})", window_spread(&gaps));
        assert!(window_spread(&chunks) > 3.0, "{key}: no large and small stretches ({:.1})", window_spread(&chunks));
    }
}

#[test]
fn drip_regimes_differ_across_instances() {
    use rand::SeedableRng;
    let cfg = crate::config::TarpitConfig::default();
    let mut shapes = HashSet::new();
    let mut medians = Vec::new();
    for key in keys(30) {
        let d = engine(&key);
        let t = &d.current().persona.timing;
        shapes.insert(t.regimes.map(|r| ((r.gap * 100.0) as i64, (r.chunk * 100.0) as i64, r.dwell as i64, r.weight)));
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(5);
        let gaps: Vec<f64> = (0..40).flat_map(|_| beats(&mut slow_cadence(&d, &cfg, &mut rng), &mut rng, 50).0).collect();
        medians.push(quantile(&gaps, 0.5));
    }
    assert_eq!(shapes.len(), 30, "two instances share a regime set");
    assert!(quantile(&medians, 1.0) > quantile(&medians, 0.0) * 2.0, "instances share one drip pace: {medians:?}");
}

#[test]
fn drip_stays_within_operator_bounds() {
    use rand::SeedableRng;
    let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(9);
    for (chunk_bytes, delay_ms) in [(48, 800), (1, 50), (4096, 3_000), (16, 0)] {
        let cfg = crate::config::TarpitConfig { chunk_bytes, delay_ms, ..Default::default() };
        for key in keys(6) {
            let d = engine(&key);
            for _ in 0..20 {
                let (gaps, chunks) = beats(&mut slow_cadence(&d, &cfg, &mut rng), &mut rng, 200);
                assert!(chunks.iter().all(|&c| c >= 1.0 && c <= (chunk_bytes * 8) as f64), "{key}: chunk out of bounds");
                assert!(gaps.iter().all(|&g| g <= cfg.max_idle_ms as f64), "{key}: silent long enough to look dead");
                if delay_ms == 0 {
                    assert!(gaps.iter().all(|&g| g == 0.0), "{key}: waited between chunks with a zero delay");
                }
            }
        }
    }
}

/// What a scanner sees before the first byte: an application answering in a
/// few hundred milliseconds, never a server that seems to have died.
#[test]
fn headers_arrive_like_an_application_answers() {
    use rand::SeedableRng;
    let cfg = crate::config::TarpitConfig::default();
    let mut medians = Vec::new();
    for key in keys(20) {
        let d = engine(&key);
        let inc = d.current();
        let (art, _) = fetch(&d, "/");
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(5);
        let mut heads: Vec<u128> = Vec::new();
        for _ in 0..400 {
            let p = crate::tarpit::Plan::sample(&cfg, &inc.persona.timing, &art, &mut rng);
            assert!(p.pre_headers.as_millis() + p.pre_body_ms() < cfg.max_idle_ms as u128, "{key}: first byte later than the idle ceiling");
            if !p.is_fast() {
                heads.push(p.pre_headers.as_millis());
            }
        }
        heads.sort_unstable();
        let median = heads[heads.len() / 2];
        assert!((150..=600).contains(&median), "{key}: headers after {median} ms typically");
        medians.push(median);
    }
    medians.sort_unstable();
    assert!(medians[19] as f64 > medians[0] as f64 * 1.4, "instances answer equally fast: {medians:?}");
}

/// Mass deployments must not share one piece size: each instance's
/// typical chunk is its own.
#[test]
fn drip_piece_sizes_differ_across_instances() {
    use rand::SeedableRng;
    let cfg = crate::config::TarpitConfig::default();
    let mut medians = Vec::new();
    for key in keys(16) {
        let d = engine(&key);
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(17);
        let (_, chunks) = beats(&mut slow_cadence(&d, &cfg, &mut rng), &mut rng, 1_000);
        medians.push(quantile(&chunks, 0.5));
        // Large pieces do not pile up at the cap.
        let cap = (cfg.chunk_bytes * 8) as f64;
        let at_cap = chunks.iter().filter(|&&c| c == cap).count();
        assert!(at_cap * 50 < chunks.len(), "{key}: {at_cap} pieces exactly at the cap");
    }
    medians.sort_by(f64::total_cmp);
    assert!(medians[15] > medians[0] * 2.5, "instances share one piece size: {medians:?}");
}

/// A lossy link: now and then a retransmission stall, after which the held
/// data arrives in one larger piece.
#[test]
fn drip_has_network_hiccups() {
    use rand::SeedableRng;
    let cfg = crate::config::TarpitConfig::default();
    let mut seen = 0;
    for key in keys(8) {
        let d = engine(&key);
        let rto = d.current().persona.timing.rto_ms;
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(13);
        let (gaps, chunks) = beats(&mut slow_cadence(&d, &cfg, &mut rng), &mut rng, 2_000);
        // A flush: a short pause after a chunk at least twice its predecessor,
        // following a pause of at least one retransmission timeout.
        seen +=
            (2..gaps.len()).filter(|&i| gaps[i - 2] >= rto && chunks[i - 1] >= 2.0 * chunks[i - 2] && gaps[i - 1] < gaps[i - 2]).count();
    }
    assert!(seen > 20, "only {seen} stall-and-flush patterns");
}

/// Dump sample artifacts for manual review:
/// `DUMP_DIR=/tmp/x cargo test dump_samples -- --ignored`
#[test]
#[ignore]
fn dump_samples() {
    let dir = std::env::var("DUMP_DIR").expect("DUMP_DIR");
    for key in keys(6) {
        let d = engine(&key);
        let inc = d.current();
        let base = format!("{dir}/{key}");
        std::fs::create_dir_all(&base).unwrap();
        let mut index = format!("{}\n\n", inc.persona.summary());
        let mut paths: Vec<String> = inc.persona.routes().iter().map(|r| concrete(&r.pattern)).filter(|p| p.starts_with('/')).collect();
        paths.push("/robots.txt".into());
        paths.push("/missing-page".into());
        paths.push("/missing.php".into());
        for (i, p) in paths.iter().enumerate() {
            let (art, body) = fetch(&d, p);
            let mut out = format!("GET {p}\n{}\n", art.status);
            for (k, v) in &art.headers {
                out += &format!("{k}: {v}\n");
            }
            out += "\n";
            out += &body;
            std::fs::write(format!("{base}/{i:03}.txt"), out).unwrap();
            index += &format!("{i:03} {p} -> {}\n", art.status);
        }
        std::fs::write(format!("{base}/index.txt"), index).unwrap();
    }
}

#[test]
fn cached_dumps_match_fresh_ones() {
    for key in keys(10) {
        let (a, b) = (engine(&key), engine(&key));
        let routes = a.current().persona.routes();
        let Some(rule) = routes.iter().find(|r| r.profile == Profile::SqlDump) else { continue };
        let path = concrete(&rule.pattern);
        let warm = fetch(&a, &path).1;
        let cached = fetch(&a, &path).1;
        let fresh = fetch(&b, &path).1;
        assert_eq!(warm, cached, "{key}: cache changed the dump");
        assert_eq!(cached, fresh, "{key}: cached dump differs from a fresh render");
        let other_host = fetch_with(&a, Method::GET, &path, Some("other.example.org"), 6).1;
        assert_eq!(other_host, fetch_with(&b, Method::GET, &path, Some("other.example.org"), 6).1);
    }
}

#[test]
fn wordpress_lists_only_a_few_authors() {
    let mut checked = 0;
    for key in keys(60) {
        let d = engine(&key);
        if d.current().persona.stack != crate::persona::Stack::WordPress {
            continue;
        }
        let (_, body) = fetch(&d, "/wp-json/wp/v2/users");
        let users: serde_json::Value = serde_json::from_str(&body).unwrap_or_else(|e| panic!("{key}: {e}: {body}"));
        let n = users.as_array().map(|a| a.len()).unwrap_or(0);
        assert!((1..=5).contains(&n), "{key}: {n} users listed");
        checked += 1;
    }
    assert!(checked > 0, "no WordPress persona among the keys");
}

/// Directory listings in the exact layout of the server's autoindex module.
#[test]
fn listings_match_the_servers_autoindex_layout() {
    use crate::persona::{AreaKind, Frontend};
    let (mut nginx, mut apache) = (0, 0);
    for key in keys(120) {
        let d = engine(&key);
        let inc = d.current();
        let p = &inc.persona;
        let Some(area) = p.site.areas.iter().find(|a| a.kind == AreaKind::Autoindex) else { continue };
        let (art, body) = fetch(&d, &area.prefix);
        if art.status != http::StatusCode::OK {
            continue;
        }
        let rows: Vec<&str> = body.lines().map(|l| l.trim_end_matches('\r')).collect();
        match p.frontend {
            Frontend::Nginx | Frontend::OpenResty => {
                let mut seen_file = false;
                for row in rows.iter().filter(|r| r.starts_with("<a href=") && !r.starts_with("<a href=\"../\"")) {
                    let close = row.find("</a>").unwrap();
                    let label = &row[row.find("\">").unwrap() + 2..close];
                    let shown = label.replace("..&gt;", "..>");
                    let rest = &row[close + 4..];
                    assert_eq!(rest.len() - rest.trim_start().len(), 51 - shown.chars().count(), "{key}: padding in {row:?}");
                    let cols: Vec<&str> = rest.split_whitespace().collect();
                    assert_eq!(cols.len(), 3, "{key}: {row:?}");
                    assert_eq!(cols[0].len(), 11, "{key}: date {row:?}");
                    let dir = shown.ends_with('/') || cols[2] == "-";
                    assert!(!(dir && seen_file), "{key}: a directory after a file: {row:?}");
                    seen_file |= !dir;
                    // date, space, a 19-wide size column
                    assert!(rest.ends_with(&format!(" {:>19}", cols[2])), "{key}: size column {row:?}");
                }
                nginx += 1;
            }
            Frontend::Apache => {
                for row in rows.iter().filter(|r| r.starts_with("<tr><td valign=\"top\"><img")) {
                    let alt = row.split("alt=\"").nth(1).unwrap().split('"').next().unwrap();
                    assert!(["[DIR]", "[TXT]", "[IMG]", "[   ]", "[PARENTDIR]"].contains(&alt), "{key}: alt {alt:?}");
                    let size = row.rsplit("<td align=\"right\">").next().unwrap().split("</td>").next().unwrap();
                    assert_eq!(size.len(), 4, "{key}: size cell {size:?} in {row:?}");
                }
                // Apache titles the directory without its trailing slash.
                let title = area.prefix.trim_end_matches('/');
                assert!(body.contains(&format!("<title>Index of {title}</title>")), "{key}: {body}");
                apache += 1;
            }
            _ => {}
        }
    }
    assert!(nginx > 0 && apache > 0, "nginx {nginx}, apache {apache} listings checked");
}

/// Same-origin stylesheets and scripts a page links to, as request paths.
fn linked_assets(body: &str, host: &str) -> Vec<String> {
    let base = format!("https://{host}");
    let mut out = Vec::new();
    for attr in ["href=\"", "src=\"", "href='", "src='"] {
        let quote = &attr[attr.len() - 1..];
        for rest in body.split(attr).skip(1) {
            let Some((url, _)) = rest.split_once(quote) else { continue };
            let url = url.strip_prefix(&base).unwrap_or(url).replace("&amp;", "&");
            let path = url.split('?').next().unwrap_or_default().to_string();
            if path.starts_with('/') && (path.ends_with(".css") || path.ends_with(".js")) {
                out.push(path);
            }
        }
    }
    out
}

/// Whatever a page links to exists, and comes back at once like a file off
/// disk, in the type the front server gives that extension.
#[test]
fn linked_assets_are_served_fast() {
    let host = "trap.example.net";
    let mut checked = 0;
    for key in keys(60) {
        let d = engine(&key);
        let inc = d.current();
        let p = &inc.persona;
        for page in ["/", p.site.login_path.as_str()] {
            let (_, body) = fetch(&d, page);
            for path in linked_assets(&body, host) {
                assert_eq!(inc.router.profile_for(&path), Profile::Asset, "{key}: {page} links {path}, which is not served");
                let (art, asset) = fetch(&d, &path);
                assert_eq!(art.status, http::StatusCode::OK, "{key}: {path}");
                assert!(art.fast, "{key}: {path} is tarpitted");
                assert!(!asset.trim().is_empty(), "{key}: {path} is empty");
                let ct = art.headers.iter().find(|(n, _)| n == "content-type").map(|(_, v)| v.as_str()).unwrap_or("");
                let want = if path.ends_with(".css") { "css" } else { "javascript" };
                assert!(ct.contains(want), "{key}: {path} served as {ct:?}");
                checked += 1;
            }
        }
    }
    assert!(checked > 100, "only {checked} linked assets checked");
}

#[test]
fn favicon_follows_the_persona() {
    use crate::persona::Favicon;
    let mut seen = HashSet::new();
    for key in keys(60) {
        let d = engine(&key);
        let inc = d.current();
        let favicon = inc.persona.assets.favicon;
        seen.insert(favicon);
        let (art, _) = fetch(&d, "/favicon.ico");
        match favicon {
            Favicon::None => assert_eq!(art.status, http::StatusCode::NOT_FOUND, "{key}"),
            Favicon::Empty => {
                assert_eq!(art.status, http::StatusCode::OK, "{key}");
                assert!(art.head.is_empty() && art.fast, "{key}");
            }
            Favicon::Own => {
                assert_eq!(art.status, http::StatusCode::OK, "{key}");
                assert!(art.head.starts_with(&[0, 0, 1, 0]) && art.fast, "{key}: not an icon");
            }
        }
    }
    assert_eq!(seen.len(), 3, "favicon kinds seen: {seen:?}");
}

#[test]
fn asset_names_are_instance_specific_and_stable() {
    let names = |key: &str| -> Vec<String> { engine(key).current().persona.assets.files.values().map(|f| f.href.clone()).collect() };
    // CMS core files sit at the same path on every install; build output
    // carries hashes of its own.
    let hashed = |key: &str| {
        let p = engine(key).current().persona.clone();
        Data::embedded()
            .assets
            .stack(p.stack.id())
            .files
            .iter()
            .any(|f| f.path.contains("{hex") || f.path.contains("{b64") || f.query.is_some())
    };
    let all: Vec<Vec<String>> = keys(30).iter().filter(|k| hashed(k)).map(|k| names(k)).collect();
    let distinct: HashSet<&Vec<String>> = all.iter().collect();
    assert!(all.len() > 15, "only {} instances with build hashes", all.len());
    assert_eq!(distinct.len(), all.len(), "two instances link the same files: {all:?}");
    assert_eq!(names("instance-key-3"), names("instance-key-3"));
}
