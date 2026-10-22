//! `templates/data/probes.yaml`: scanner request patterns and their parts.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeData {
    pub patterns: Vec<ProbePattern>,
    pub parts: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbePattern {
    pub weight: u32,
    pub path: String,
}

impl ProbeData {
    /// Placeholder names used in a pattern (`{dir}` -> `dir`).
    pub fn slots(pattern: &str) -> impl Iterator<Item = &str> {
        pattern.split('{').skip(1).filter_map(|s| s.split_once('}').map(|(name, _)| name))
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.patterns.is_empty(), "probes: no patterns");
        for p in &self.patterns {
            for slot in Self::slots(&p.path) {
                let part = self.parts.get(slot);
                anyhow::ensure!(part.is_some_and(|v| !v.is_empty()), "probes: {:?} uses unknown part {slot:?}", p.path);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_extracted() {
        assert_eq!(ProbeData::slots("/{dir}.env.{env_suffix}").collect::<Vec<_>>(), ["dir", "env_suffix"]);
        assert_eq!(ProbeData::slots("/plain").count(), 0);
    }
}
