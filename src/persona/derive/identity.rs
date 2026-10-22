//! Company name, domain, industry and staff.

use std::collections::HashSet;

use rand::{Rng, RngExt};
use rand_chacha::ChaCha8Rng;

use crate::data::{Data, Industry, Timezone};
use crate::persona::model::Person;
use crate::persona::{capitalize, short_slug};
use crate::util::{pick_str, weighted};

pub struct Identity {
    pub company: String,
    pub slug: String,
    pub domain: String,
    pub industry: &'static Industry,
}

impl Identity {
    pub fn short(&self) -> &str {
        short_slug(&self.slug)
    }
}

pub fn derive(rng: &mut ChaCha8Rng, data: &'static Data) -> Identity {
    let w = &data.words;
    let industry = &w.industries[rng.random_range(0..w.industries.len())];
    let base = format!("{}{}", pick_str(rng, &w.name_heads), pick_str(rng, &w.name_tails));
    let suffix = pick_str(rng, &industry.suffixes);
    let slug = match rng.random_range(0..3) {
        0 => base.clone(),
        1 => format!("{base}-{}", suffix.to_ascii_lowercase()),
        _ => format!("{base}{}", suffix.to_ascii_lowercase()),
    };
    Identity { company: format!("{} {suffix}", capitalize(&base)), domain: format!("{slug}.{}", pick_str(rng, &w.tlds)), slug, industry }
}

/// Name locales for this company: its time zone's, re-weighted, with an
/// occasional locale from elsewhere (people move).
pub fn name_mix(rng: &mut ChaCha8Rng, data: &'static Data, tz: &'static Timezone) -> Vec<(&'static str, u32)> {
    let mut mix: Vec<(&'static str, u32)> =
        tz.locales.iter().map(|(l, w)| (l.as_str(), ((*w as f64) * rng.random_range(0.6..1.4)).ceil() as u32)).collect();
    let all: Vec<&'static str> = data.names.keys().map(String::as_str).collect();
    for _ in 0..rng.random_range(0..3) {
        let extra = all[rng.random_range(0..all.len())];
        if !mix.iter().any(|(l, _)| *l == extra) {
            mix.push((extra, 1));
        }
    }
    mix
}

/// A (first, last) name drawn from a locale mix.
pub fn name(rng: &mut impl Rng, data: &'static Data, mix: &[(&'static str, u32)]) -> (&'static str, &'static str) {
    let locale = weighted(rng, mix);
    let set = &data.names[locale];
    (pick_str(rng, &set.first), pick_str(rng, &set.last))
}

/// Staff with one company-wide email convention.
pub fn staff(rng: &mut ChaCha8Rng, data: &'static Data, mix: &[(&'static str, u32)], domain: &str) -> Vec<Person> {
    let n = rng.random_range(6..24);
    let pattern = rng.random_range(0..4);
    let mut used = HashSet::new();
    let mut staff = Vec::with_capacity(n);
    // Bounded: with a tiny name list the convention may run out of unique
    // addresses; a smaller team is fine.
    for _ in 0..n * 20 {
        if staff.len() == n {
            break;
        }
        let (first, last) = name(rng, data, mix);
        let local = match pattern {
            0 => format!("{first}.{last}"),
            1 => format!("{}{last}", &first[..1]),
            2 => first.to_string(),
            _ => format!("{first}{}", &last[..1]),
        };
        if !used.insert(local.clone()) {
            continue;
        }
        let login = if pattern == 2 { first.to_string() } else { local.replace('.', "") };
        staff.push(Person { first, last, email: format!("{local}@{domain}"), login });
    }
    staff
}
