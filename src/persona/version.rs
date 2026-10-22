//! Software versions that move forward with content epochs, like a box that
//! gets upgraded now and then, and only ever through releases that exist.
//!
//! A data entry is one release series:
//! - `8.3.0..8.3.35`: every patch release between the two (inclusive)
//! - `2.4.57|2.4.58|2.4.62`: exactly these releases, in order
//! - `1.24.0`: a single release, such as a distribution's package, which
//!   keeps its upstream version through security updates
//!
//! Versions are written with as many parts as the data gives (`16.4`,
//! `6.1.7.10`, `1.25.3.2`), the way each project numbers its releases.

use rand::{Rng, RngExt};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Ver {
    /// Of the release the persona started on (they do not change within a
    /// series).
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    #[serde(skip)]
    releases: Vec<String>,
    #[serde(skip)]
    start: usize,
    /// Time between upgrades, in seconds.
    #[serde(skip)]
    every: i64,
    /// Upgrades at most, within the series (0: never upgraded).
    #[serde(skip)]
    span: usize,
}

/// Upgrades count from here (2026-09-01): before it every persona shows the
/// release it started on.
const UPGRADES_FROM: i64 = 1_788_220_800;
const WEEK: i64 = 7 * 86_400;

impl Ver {
    /// Parse a series and pick where in it this persona starts; it is then
    /// upgraded every few weeks, a few times at most.
    pub fn parse(rng: &mut impl Rng, text: &str) -> Self {
        let mut v = Self::fixed(rng, text);
        v.every = rng.random_range(3..14) * WEEK;
        v.span = rng.random_range(1..5);
        v
    }

    /// A release that never changes within the persona's era: what shows in
    /// headers, which are fixed per persona.
    pub fn fixed(rng: &mut impl Rng, text: &str) -> Self {
        let releases = series(text);
        let start = rng.random_range(0..releases.len());
        let [major, minor, patch] = numbers(&releases[start]);
        Ver { major, minor, patch, releases, start, every: WEEK, span: 0 }
    }

    /// The version installed at unix time `at`.
    pub fn at(&self, at: i64) -> String {
        let steps = ((at - UPGRADES_FROM).max(0) / self.every) as usize;
        self.releases[(self.start + steps.min(self.span)).min(self.releases.len() - 1)].clone()
    }

    /// The release the persona started on.
    pub fn initial(&self) -> String {
        self.releases[self.start].clone()
    }

    /// `major.minor`.
    pub fn short(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }
}

/// All releases a series entry names, oldest first.
fn series(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.contains('|') {
        return text.split('|').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    }
    if let Some((from, to)) = text.split_once("..") {
        let (prefix, first) = from.rsplit_once('.').unwrap_or(("", from));
        let last = to.rsplit_once('.').map(|(_, l)| l).unwrap_or(to);
        if let (Ok(a), Ok(b)) = (first.parse::<u32>(), last.parse::<u32>()) {
            return (a..=b.max(a)).map(|n| if prefix.is_empty() { n.to_string() } else { format!("{prefix}.{n}") }).collect();
        }
    }
    vec![text.to_string()]
}

fn numbers(release: &str) -> [u32; 3] {
    let mut parts = release.split(['.', '+', '-']).map(|p| p.parse::<u32>().unwrap_or(0));
    [parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0)]
}

/// Whether `text` is a series entry this module understands, with every
/// release a dotted number (data validation).
pub fn valid_series(text: &str) -> bool {
    let all = series(text);
    !all.is_empty() && all.iter().all(|r| !r.is_empty() && r.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::keyed_rng;

    #[test]
    fn series_forms() {
        assert_eq!(series("8.3.0..8.3.3"), ["8.3.0", "8.3.1", "8.3.2", "8.3.3"]);
        assert_eq!(series("16.4..16.6"), ["16.4", "16.5", "16.6"]);
        assert_eq!(series("2.4.57|2.4.58|2.4.62"), ["2.4.57", "2.4.58", "2.4.62"]);
        assert_eq!(series("1.24.0"), ["1.24.0"]);
        assert!(valid_series("6.1.7.10") && valid_series("1.25.3.1|1.25.3.2") && !valid_series("8.3.x"));
    }

    #[test]
    fn parts_follow_the_data() {
        let mut rng = keyed_rng(b"v", &[]);
        let v = Ver::parse(&mut rng, "16.4");
        assert_eq!((v.major, v.minor, v.patch), (16, 4, 0));
        assert_eq!(v.at(UPGRADES_FROM + 52 * WEEK), "16.4");
        assert_eq!(v.short(), "16.4");
    }

    #[test]
    fn fixed_versions_never_move() {
        let mut rng = keyed_rng(b"f", &[]);
        let v = Ver::fixed(&mut rng, "8.3.0..8.3.35");
        assert_eq!(v.at(0), v.at(UPGRADES_FROM + 500 * WEEK));
        assert_eq!(v.at(0), v.initial());
    }

    #[test]
    fn only_real_releases_and_never_backwards() {
        for seed in 0..50u32 {
            let mut rng = keyed_rng(b"v", &[&seed.to_le_bytes()]);
            let v = Ver::parse(&mut rng, "2.4.57|2.4.58|2.4.62|2.4.63");
            let mut last = 0;
            for week in -10..200 {
                let at = v.at(UPGRADES_FROM + week * WEEK);
                let i = v.releases.iter().position(|r| *r == at).expect("a listed release");
                assert!(i >= last, "seed {seed}: {at} in week {week} went backwards");
                last = i;
            }
        }
    }
}
