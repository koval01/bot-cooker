//! User-Agent strings for the instance's visitors.
//!
//! No finished UA string is stored anywhere: shapes come from data, versions
//! are computed for the simulated date (so a log from March shows the Chrome
//! of March), visitors lag behind the newest release by a realistic amount,
//! and every instance re-weights the mix of browsers, crawlers and tools.

use rand::{Rng, RngExt};

use super::World;
use crate::data::AgentShape;
use crate::persona::fill::{Vars, fill};
use crate::util::weighted;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Population {
    Browser,
    Crawler,
    Tool,
}

/// A UA for a visitor seen at time `t`.
pub fn agent(w: &World, rng: &mut impl Rng, t: i64) -> String {
    let population = population(w, rng);
    agent_of(w, rng, t, population)
}

pub fn agent_of(w: &World, rng: &mut impl Rng, t: i64, population: Population) -> String {
    let data = &w.data().agents;
    let (label, shapes) = match population {
        Population::Browser => ("browsers", &data.browsers),
        Population::Crawler => ("crawlers", &data.crawlers),
        Population::Tool => ("tools", &data.tools),
    };
    let weights = instance_weights(w, label, shapes);
    let shape = &shapes[weighted(rng, &weights)];
    fill(&shape.ua, &versions(w, rng, t), rng)
}

fn population(w: &World, rng: &mut impl Rng) -> Population {
    let mix = &w.data().agents.mix;
    let mut fixed = w.fixed_rng("agent-mix");
    let mut skew = |name: &str| (*mix.get(name).unwrap_or(&0) as f64 * fixed.random_range(0.5..1.6)) as u32;
    let options = [(Population::Browser, skew("browsers")), (Population::Crawler, skew("crawlers")), (Population::Tool, skew("tools"))];
    weighted(rng, &options)
}

/// The data weights, perturbed per instance; a few shapes never show up at
/// all on a given instance.
fn instance_weights(w: &World, label: &str, shapes: &[AgentShape]) -> Vec<(usize, u32)> {
    let mut fixed = w.fixed_rng(&format!("agents:{label}"));
    let mut out: Vec<(usize, u32)> = shapes
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let gone = fixed.random_bool(0.15);
            (i, if gone { 0 } else { (s.weight as f64 * fixed.random_range(0.3..1.7)).ceil() as u32 })
        })
        .collect();
    if out.iter().all(|(_, w)| *w == 0) {
        out[0].1 = 1;
    }
    out
}

/// Version placeholders for date `t`.
fn versions(w: &World, rng: &mut impl Rng, t: i64) -> Vars<'static> {
    let data = &w.data().agents;
    let lag_table: Vec<(u32, u32)> = data.lag.iter().map(|(k, v)| (*k, *v)).collect();
    let mut lag = || weighted(rng, &lag_table);
    let major = |name: &str| data.cadences.get(name).map(|c| c.major_at(t)).unwrap_or(0);
    let chrome = major("chrome").saturating_sub(lag());
    let firefox = major("firefox").saturating_sub(lag());
    let samsung = major("samsung").saturating_sub(lag().min(2));
    let mut stepped = |product: &str, max_lag: usize| {
        let (idx, steps) = data.steps_at(product, t);
        let back = (lag() as usize).min(max_lag).min(idx);
        steps.get(idx - back).map(|(_, v)| v.clone()).unwrap_or_default()
    };
    let safari = stepped("safari", 1);
    let ios = stepped("ios", 2);
    let curl = stepped("curl", 4);
    let requests = stepped("requests", 3);
    let wget = stepped("wget", 2);
    Vars::default()
        .with("chrome", chrome.to_string())
        .with("chrome_old", chrome.saturating_sub(rng.random_range(3..25)).to_string())
        .with("chrome_build", (6099 + (chrome.saturating_sub(120)) * 90 + rng.random_range(0..60)).to_string())
        .with("chrome_patch", rng.random_range(30..220).to_string())
        .with("edge_build", (2210 + (chrome.saturating_sub(120)) * 64 + rng.random_range(0..40)).to_string())
        .with("edge_patch", rng.random_range(10..120).to_string())
        .with("firefox", firefox.to_string())
        .with("samsung", samsung.to_string())
        .with("safari", safari)
        .with("safari_minor", rng.random_range(0..6).to_string())
        .with("ios", ios)
        .with("curl", curl)
        .with("requests", requests)
        .with("wget", wget)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::super::testing::*;
    use super::*;
    use crate::util::{keyed_rng, parse_date};

    #[test]
    fn versions_follow_the_simulated_date() {
        let p = persona(1);
        let w = World::new(&p, epochs());
        let chrome = |date: &str| {
            let t = parse_date(date).unwrap();
            let mut rng = keyed_rng(b"ua", &[date.as_bytes()]);
            (0..200)
                .filter_map(|_| {
                    let ua = agent_of(&w, &mut rng, t, Population::Browser);
                    ua.split("Chrome/").nth(1).and_then(|v| v.split('.').next()?.parse::<u32>().ok())
                })
                .max()
                .unwrap()
        };
        assert!(chrome("2026-10-01") > chrome("2025-01-01"));
        assert!(chrome("2026-10-01") <= 130 + (parse_date("2026-10-01").unwrap() - parse_date("2024-10-15").unwrap()) as u32 / 86_400 / 31);
    }

    #[test]
    fn no_placeholder_survives() {
        for i in 0..10 {
            let p = persona(i);
            let w = World::new(&p, epochs());
            let mut rng = keyed_rng(b"ua", &[&i.to_le_bytes()]);
            for _ in 0..300 {
                let ua = agent(&w, &mut rng, NOW);
                assert!(!ua.contains('{') && !ua.contains('}'), "{ua}");
            }
        }
    }

    #[test]
    fn instances_see_different_mixes() {
        let sets: Vec<HashSet<String>> = (0..2)
            .map(|i| {
                let p = persona(10 + i);
                let w = World::new(&p, epochs());
                let mut rng = keyed_rng(b"same", &[]);
                (0..400).map(|_| agent(&w, &mut rng, NOW)).collect()
            })
            .collect();
        let shared = sets[0].intersection(&sets[1]).count();
        assert!(shared * 2 < sets[0].len().min(sets[1].len()), "instances share most of their UAs: {shared}");
    }
}
