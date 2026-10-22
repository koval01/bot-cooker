//! Traffic the instance "received": a chronological stream of requests from
//! a small, skewed pool of visitors, hitting paths that exist (and the usual
//! scanner noise), with statuses consistent with the site.

use rand::seq::IndexedRandom;
use rand::{Rng, RngExt};
use serde::Serialize;

use super::{World, agents};
use crate::data::ProbeData;
use crate::persona::Favicon;
use crate::persona::fill::{Vars, fill};
use crate::util::{self, chance, pick_str, weighted};

/// A log file's time window, cut into equal blocks so block `k` can be
/// generated on its own and timestamps still only go forward.
#[derive(Clone, Copy, Debug)]
pub struct Timeline {
    pub start: i64,
    /// Seconds covered by one block.
    pub span: i64,
    pub lines: usize,
    pub blocks: u64,
}

pub fn timeline(w: &World, path: &str) -> Timeline {
    let mut r = w.persona.rng(&format!("log-timeline:{path}"));
    let window = r.random_range(1..12) * 86_400 + r.random_range(0..86_400);
    let lines = r.random_range(12..40);
    let gap = r.random_range(4..90);
    let span = lines as i64 * gap;
    Timeline { start: w.epochs.log_anchor - window, span, lines, blocks: (window / span).max(1) as u64 }
}

#[derive(Clone, Debug, Serialize)]
pub struct Visitor {
    pub ip: String,
    pub agent: String,
}

/// Visitors of the current log epoch. Agents are drawn for the middle of
/// the window, so the versions match the dates in the log.
pub fn visitors(w: &World, around: i64) -> Vec<Visitor> {
    let mut r = w.log_rng("visitors", "");
    (0..r.random_range(12..40)).map(|_| Visitor { ip: util::public_ipv4(&mut r), agent: agents::agent(w, &mut r, around) }).collect()
}

/// A path that exists on this persona and the status it usually returns.
#[derive(Clone, Debug, Serialize)]
pub struct KnownPath {
    pub path: String,
    pub status: u16,
}

pub fn known_paths(w: &World) -> Vec<KnownPath> {
    let p = w.persona;
    let ok = |path: String| KnownPath { path, status: 200 };
    let favicon = KnownPath { path: "/favicon.ico".into(), status: if p.assets.favicon == Favicon::None { 404 } else { 200 } };
    let mut out = vec![ok("/".into()), favicon, ok("/robots.txt".into()), ok(p.site.login_path.clone())];
    for a in &p.site.areas {
        out.push(ok(a.prefix.clone()));
        out.extend(p.site.vocabulary.iter().take(5).map(|v| ok(format!("{}{v}/", a.prefix))));
    }
    if !p.cms {
        for res in &p.site.api_resources {
            let status = if w.fixed_rng(res).random_bool(0.5) { 200 } else { 401 };
            out.push(KnownPath { path: format!("{}/{res}", p.site.api_prefix), status });
        }
    }
    // The files pages link to, under the names the pages use.
    out.extend(p.assets.files.values().map(|f| ok(f.path.clone())));
    out.extend(p.site.sitemap.clone().map(ok));
    out
}

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub t: i64,
    pub micros: u32,
    pub ip: String,
    pub agent: String,
    pub method: &'static str,
    pub path: String,
    pub status: u16,
    pub bytes: u32,
    pub ms: u32,
    pub referer: Option<String>,
    /// Scanner noise rather than a page of the site.
    pub probe: bool,
    pub pid: u32,
    pub conn: u32,
    /// A free selector templates use to vary messages per line.
    pub pick: u32,
}

/// Events of block `seq` of the log at `path`, in time order.
pub fn events(w: &World, path: &str, seq: u64) -> Vec<Event> {
    let tl = timeline(w, path);
    let mut r = w.log_rng(&format!("log-block:{seq}"), path);
    let from = tl.start + seq as i64 * tl.span;
    // Sub-second parts are sorted together with the seconds.
    let mut times: Vec<(i64, u32)> = (0..tl.lines).map(|_| (from + r.random_range(0..tl.span), r.random_range(0..1_000_000))).collect();
    times.sort_unstable();
    let visitors = visitors(w, tl.start + (w.epochs.log_anchor - tl.start) / 2);
    let known = known_paths(w);
    let pid_base = 1000 + (w.epochs.log % 30_000) as u32;
    times
        .into_iter()
        .map(|(t, micros)| {
            // Zipf-ish: a handful of visitors make most of the requests.
            let vi = ((r.random::<f64>().powi(3)) * visitors.len() as f64) as usize;
            let v = &visitors[vi.min(visitors.len() - 1)];
            let is_probe = chance(&mut r, 0.22);
            let (method, path, status) = if is_probe {
                ("GET", probe(w, &mut r), weighted(&mut r, &[(404u16, 8), (403, 1), (301, 1)]))
            } else {
                let k = known.choose(&mut r).cloned().unwrap_or(KnownPath { path: "/".into(), status: 200 });
                let post = k.path.contains("login") && chance(&mut r, 0.3);
                let status = match (post, k.status) {
                    (true, _) => weighted(&mut r, &[(302u16, 5), (200, 3), (419, 1)]),
                    (false, 200) if chance(&mut r, 0.12) => 304,
                    (false, s) => s,
                };
                (if post { "POST" } else { "GET" }, k.path, status)
            };
            Event {
                t,
                micros,
                ip: v.ip.clone(),
                agent: v.agent.clone(),
                method,
                bytes: match status {
                    304 => 0,
                    301 | 302 => r.random_range(0..400),
                    400.. => r.random_range(150..900),
                    _ => r.random_range(400..60_000),
                },
                ms: r.random_range(2..1800),
                referer: chance(&mut r, 0.3).then(|| known.choose(&mut r).map(|k| k.path.clone()).unwrap_or_default()),
                probe: is_probe,
                path,
                status,
                pid: pid_base + r.random_range(0..4),
                conn: r.random_range(1000..900_000),
                pick: r.random(),
            }
        })
        .collect()
}

/// A scanner request path: a weighted pattern (weights skewed per instance,
/// since different scanners favour different hosts) with its parts filled.
pub fn probe(w: &World, rng: &mut impl Rng) -> String {
    let data = &w.data().probes;
    let mut fixed = w.fixed_rng("probe-mix");
    let weights: Vec<(usize, u32)> =
        data.patterns.iter().enumerate().map(|(i, p)| (i, (p.weight as f64 * fixed.random_range(0.2..1.8)).ceil() as u32)).collect();
    let pattern = &data.patterns[weighted(rng, &weights)].path;
    let mut vars = Vars::default();
    for slot in ProbeData::slots(pattern) {
        if let Some(values) = data.parts.get(slot) {
            vars = vars.with(slot, pick_str(rng, values).to_string());
        }
    }
    fill(pattern, &vars, rng)
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;

    #[test]
    fn logged_assets_are_the_ones_pages_link() {
        for n in 0..20 {
            let p = persona(n);
            let w = World::new(&p, epochs());
            let files: Vec<&str> = p.assets.files.values().map(|f| f.path.as_str()).collect();
            for k in known_paths(&w) {
                if k.path.ends_with(".css") || k.path.ends_with(".js") {
                    assert!(files.contains(&k.path.as_str()), "{} is logged but never linked", k.path);
                }
            }
        }
    }

    #[test]
    fn blocks_are_chronological_across_boundaries() {
        let p = persona(11);
        let w = World::new(&p, epochs());
        let all: Vec<(i64, u32)> = (0..20).flat_map(|seq| events(&w, "/access.log", seq)).map(|e| (e.t, e.micros)).collect();
        assert!(all.windows(2).all(|x| x[0] <= x[1]));
        assert!(all.last().unwrap().0 <= w.epochs.log_anchor);
    }

    #[test]
    fn probes_are_filled_and_varied() {
        let p = persona(13);
        let w = World::new(&p, epochs());
        let mut rng = crate::util::keyed_rng(b"probe", &[]);
        let paths: std::collections::HashSet<String> = (0..300).map(|_| probe(&w, &mut rng)).collect();
        assert!(paths.iter().all(|p| p.starts_with('/') && !p.contains('{')));
        assert!(paths.len() > 80, "only {} distinct probes", paths.len());
    }

    #[test]
    fn traffic_mixes_site_paths_and_probes() {
        let p = persona(12);
        let w = World::new(&p, epochs());
        let evs: Vec<Event> = (0..30).flat_map(|seq| events(&w, "/access.log", seq)).collect();
        let probes = evs.iter().filter(|e| e.probe).count();
        assert!(probes > 0 && probes < evs.len() / 2);
        assert!(evs.iter().filter(|e| e.probe).all(|e| e.status != 200));
        let ips: std::collections::HashSet<_> = evs.iter().map(|e| &e.ip).collect();
        assert!(ips.len() < evs.len() / 3, "visitors repeat");
    }
}
