//! `templates/data/agents.yaml`: release cadences and UA shapes.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::util::parse_date;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentData {
    pub cadences: BTreeMap<String, Cadence>,
    pub steps: BTreeMap<String, Vec<(String, String)>>,
    pub lag: BTreeMap<u32, u32>,
    pub browsers: Vec<AgentShape>,
    pub crawlers: Vec<AgentShape>,
    pub tools: Vec<AgentShape>,
    pub mix: BTreeMap<String, u32>,
}

/// A product that ships a new major every `days` days.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cadence {
    pub base: u32,
    pub since: String,
    pub days: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentShape {
    pub weight: u32,
    pub ua: String,
}

impl Cadence {
    /// Newest major version on date `t` (unix seconds).
    pub fn major_at(&self, t: i64) -> u32 {
        let since = parse_date(&self.since).unwrap_or(0);
        let elapsed = (t - since).max(0) / 86_400;
        self.base + (elapsed / self.days.max(1) as i64) as u32
    }
}

impl AgentData {
    /// Index of the newest step released by `t`, and all steps of `product`.
    pub fn steps_at(&self, product: &str, t: i64) -> (usize, &[(String, String)]) {
        let steps = self.steps.get(product).map(Vec::as_slice).unwrap_or(&[]);
        let idx = steps.iter().rposition(|(date, _)| parse_date(date).is_some_and(|d| d <= t)).unwrap_or(0);
        (idx, steps)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        for (name, steps) in &self.steps {
            anyhow::ensure!(!steps.is_empty(), "agents: {name} has no releases");
            for (date, _) in steps {
                anyhow::ensure!(parse_date(date).is_some(), "agents: bad date {date:?} for {name}");
            }
        }
        for (name, c) in &self.cadences {
            anyhow::ensure!(parse_date(&c.since).is_some(), "agents: bad date for {name}");
        }
        anyhow::ensure!(!self.browsers.is_empty() && !self.crawlers.is_empty() && !self.tools.is_empty(), "agents: empty population");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cadence_counts_releases() {
        let c = Cadence { base: 120, since: "2023-12-05".into(), days: 28 };
        assert_eq!(c.major_at(parse_date("2023-12-05").unwrap()), 120);
        assert_eq!(c.major_at(parse_date("2024-01-02").unwrap()), 121);
        assert_eq!(c.major_at(parse_date("2020-01-01").unwrap()), 120, "never before base");
    }
}
