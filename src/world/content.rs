//! Site content: numbered posts, and what sits in directories.

use rand::RngExt;
use serde::Serialize;

use super::World;
use crate::persona::capitalize;
use crate::util::{self, Tm, parse_date, pick};

#[derive(Clone, Debug, Serialize)]
pub struct Post {
    pub n: u64,
    pub slug: String,
    pub title: String,
    pub time: i64,
}

/// Post `n`, stable for the persona; one post every few days since launch.
pub fn post(w: &World, n: u64) -> Post {
    let p = w.persona;
    let mut r = util::keyed_rng(&p.mac(&[b"post"]), &[&n.to_le_bytes()]);
    // A short line of the persona's own prose makes the headline.
    let words = r.random_range(3..9);
    let sentence = p.sentence(&mut r, words);
    let title = capitalize(sentence.trim_end_matches(['.', '!', '?']));
    let mut slug = slugify(&title);
    if r.random_bool(0.3) {
        slug = format!("{slug}-{n}");
    }
    let mut t = util::keyed_rng(&p.mac(&[b"post-time"]), &[&n.to_le_bytes()]);
    Post { n, slug, title, time: w.founded() + n as i64 * 3 * 86_400 + t.random_range(0..2 * 86_400) }
}

/// Number of the newest post as of the current epoch.
pub fn newest(w: &World) -> u64 {
    1 + ((w.epochs.log_anchor - w.founded()).max(0) / 86_400 / 3) as u64
}

/// `Our New Export, Finally` -> `our-new-export-finally`
pub fn slugify(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub mtime: i64,
    pub size: u64,
}

/// Contents of a browsable directory in a content area, sorted by name like
/// a real autoindex. Files use extensions that route to a generator.
pub fn listing(w: &World, dir: &str) -> Vec<Entry> {
    let p = w.persona;
    let mut r = w.cfg_rng("autoindex", dir);
    let depth = dir.trim_matches('/').split('/').count();
    let anchor = w.epochs.cfg_anchor;
    let mut out: Vec<Entry> = Vec::new();
    for _ in 0..r.random_range(3..30) {
        let is_dir = r.random_bool(if depth > 5 { 0.3 } else { 0.55 });
        let word = pick(&mut r, &p.site.vocabulary);
        // A date or year in the name bounds the modification time: nothing
        // is written before the day it is named after.
        let oldest = anchor - 800 * 86_400;
        let (base, since) = match r.random_range(0..4) {
            0 => (word.to_string(), oldest),
            1 => {
                let year = Tm::at(anchor - r.random_range(0..900) * 86_400, 0).year;
                (format!("{word}-{year}"), parse_date(&format!("{year}-01-01")).unwrap_or(oldest).max(oldest))
            }
            2 => (format!("{}_{}", word.replace('-', "_"), r.random_range(1..40)), oldest),
            _ => {
                let day = anchor - r.random_range(1..700) * 86_400;
                let day = day - day.rem_euclid(86_400);
                (Tm::at(day, 0).date(), day)
            }
        };
        let name = if is_dir {
            base
        } else {
            match pick(&mut r, &["sql", "log", "json", "xml", "bak", "sql", "log"]) {
                "bak" => format!("{base}.env.bak"),
                ext => format!("{base}.{ext}"),
            }
        };
        if out.iter().any(|e| e.name == name) {
            continue;
        }
        out.push(Entry {
            size: if is_dir { 0 } else { r.random_range(900..40_000_000) },
            mtime: since + r.random_range(0..(anchor - since).max(1)),
            dir: is_dir,
            name,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The persona's real leftovers (backups, logs) inside `dir`.
pub fn leftovers(w: &World, dir: &str) -> Vec<Entry> {
    let p = w.persona;
    let dir = dir.to_ascii_lowercase();
    let mut out: Vec<Entry> = p
        .site
        .backups
        .iter()
        .chain(p.site.logs.iter().map(|(f, _)| f))
        .filter(|f| f.to_ascii_lowercase().starts_with(&dir) && !f[dir.len()..].contains('/'))
        .map(|f| Entry {
            name: f[dir.len()..].to_string(),
            dir: false,
            mtime: if f.contains("log") {
                w.epochs.log_anchor - (file_size(w, f) % 600) as i64
            } else {
                w.epochs.cfg_anchor - 3_600 - (file_size(w, f) % (40 * 86_400)) as i64
            },
            size: file_size(w, f),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The on-disk size a large streamed file claims (listing and ETag agree).
pub fn file_size(w: &World, path: &str) -> u64 {
    let m = w.persona.mac(&[b"size", path.as_bytes()]);
    u64::from_le_bytes(m[..8].try_into().unwrap_or([0; 8])) % 400_000_000 + 4_000_000
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;

    #[test]
    fn dated_folders_are_not_older_than_their_names() {
        for n in 0..30 {
            let p = persona(n);
            let w = World::new(&p, epochs());
            for dir in ["/files/", "/uploads/2025/", "/media/a/b/"] {
                for e in listing(&w, dir) {
                    assert!(e.mtime <= w.epochs.cfg_anchor, "{}: in the future", e.name);
                    let named = parse_date(&e.name).or_else(|| {
                        let year = e.name.rsplit('-').next().filter(|y| y.len() == 4)?;
                        parse_date(&format!("{year}-01-01"))
                    });
                    if let Some(day) = named {
                        assert!(e.mtime >= day, "{} modified before its date", e.name);
                    }
                }
            }
        }
    }

    #[test]
    fn posts_are_dated_in_order_and_stable() {
        let p = persona(9);
        let w = World::new(&p, epochs());
        assert_eq!(post(&w, 5).slug, post(&w, 5).slug);
        assert!(post(&w, 5).time < post(&w, 6).time);
        assert!(post(&w, newest(&w)).time <= w.epochs.log_anchor + 3 * 86_400);
    }

    #[test]
    fn slugs() {
        assert_eq!(slugify("Our New Export, Finally!"), "our-new-export-finally");
        assert_eq!(slugify("  --a  b-- "), "a-b");
    }

    #[test]
    fn listings_are_sorted_unique_and_stable() {
        let p = persona(10);
        let w = World::new(&p, epochs());
        let a = listing(&w, "/files/");
        let names: Vec<&str> = a.iter().map(|e| e.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(names, sorted);
        assert_eq!(names, listing(&w, "/files/").iter().map(|e| e.name.as_str()).collect::<Vec<_>>());
    }
}
