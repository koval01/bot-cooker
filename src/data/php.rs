//! `templates/data/php_errors.yaml`: what PHP prints when errors reach the
//! page, and where in each stack's code they come from.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::persona::{PhpDisplay, PhpLevel, Stack};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhpErrorData {
    /// Wording by kind, then by the first PHP release (`8.0`) using it.
    pub messages: BTreeMap<String, BTreeMap<String, Message>>,
    pub labels: BTreeMap<PhpLevel, String>,
    pub null_args: Vec<NullArg>,
    pub allocations: Vec<u64>,
    pub stacks: BTreeMap<Stack, PhpStack>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub level: PhpLevel,
    pub text: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NullArg {
    #[serde(rename = "fn")]
    pub function: String,
    pub n: u32,
    pub param: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhpStack {
    /// Only this framework major lets PHP print raw errors.
    pub framework_major: Option<u32>,
    pub modes: Vec<Mode>,
    /// Whether the stack's `blank` scripts load the faulty code too.
    pub blank_boots: bool,
    /// Slot values by name (`key`, `var`, `prop`).
    pub words: BTreeMap<String, Vec<String>>,
    pub sources: Vec<Source>,
    pub core: Option<Core>,
    #[serde(default)]
    pub deprecated: Vec<Deprecated>,
    #[serde(default)]
    pub fatal: Vec<Source>,
    #[serde(default)]
    pub fatal_share: f64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mode {
    pub setting: String,
    pub display: PhpDisplay,
    /// What error_reporting lets through.
    pub levels: Vec<PhpLevel>,
    pub weight: u32,
}

/// A file of the application that misbehaves, relative to its root.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub file: String,
    pub class: Option<String>,
    /// Text domain of the plugin the file belongs to.
    pub domain: Option<String>,
    /// Length of the file.
    pub lines: u32,
    #[serde(default)]
    pub place: crate::persona::PhpPlace,
}

/// Notices the application raises itself, from one line that moves between
/// releases.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Core {
    pub file: String,
    /// By framework release series (`major.minor`).
    pub lines: BTreeMap<String, u32>,
    /// Kinds and the first release series that raises them.
    pub kinds: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deprecated {
    #[serde(rename = "fn")]
    pub function: String,
    pub since: String,
    #[serde(rename = "use")]
    pub replacement: String,
}

/// `major.minor` as a comparable pair.
pub fn series(text: &str) -> (u32, u32) {
    let mut parts = text.split('.').map(|p| p.parse().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

impl PhpErrorData {
    /// The wording of `kind` on PHP `php` (`major.minor`), if it happens there.
    pub fn message(&self, kind: &str, php: (u32, u32)) -> Option<&Message> {
        self.messages.get(kind)?.iter().filter(|(since, _)| series(since) <= php).max_by_key(|(since, _)| series(since)).map(|(_, m)| m)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        for (kind, by_php) in &self.messages {
            anyhow::ensure!(!by_php.is_empty(), "php_errors.yaml: {kind} has no wording");
            for m in by_php.values() {
                anyhow::ensure!(self.labels.contains_key(&m.level), "php_errors.yaml: no label for {:?}", m.level);
            }
        }
        anyhow::ensure!(!self.null_args.is_empty() && !self.allocations.is_empty(), "php_errors.yaml: empty list");
        for (stack, s) in &self.stacks {
            anyhow::ensure!(!s.sources.is_empty() && !s.modes.is_empty(), "php_errors.yaml {stack:?}: no sources or modes");
            for slot in ["key", "var", "prop"] {
                anyhow::ensure!(s.words.get(slot).is_some_and(|w| !w.is_empty()), "php_errors.yaml {stack:?}: no {slot} words");
            }
            if let Some(core) = &s.core {
                anyhow::ensure!(!core.lines.is_empty(), "php_errors.yaml {stack:?}: no core lines");
                for kind in core.kinds.keys() {
                    anyhow::ensure!(self.messages.contains_key(kind), "php_errors.yaml {stack:?}: unknown kind {kind}");
                }
                anyhow::ensure!(!s.deprecated.is_empty(), "php_errors.yaml {stack:?}: core without deprecated functions");
            }
            anyhow::ensure!(s.fatal_share == 0.0 || !s.fatal.is_empty(), "php_errors.yaml {stack:?}: fatal share without places");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Data;

    #[test]
    fn wording_follows_the_php_release() {
        let d = &Data::embedded().php_errors;
        let text = |kind: &str, php| d.message(kind, php).map(|m| (m.level, m.text.as_str()));
        assert_eq!(text("undefined_key", (7, 4)), Some((PhpLevel::Notice, "Undefined index: {key}")));
        assert_eq!(text("undefined_key", (8, 4)), Some((PhpLevel::Warning, "Undefined array key \"{key}\"")));
        assert_eq!(text("offset_on_null", (8, 2)).map(|t| t.1), Some("Trying to access array offset on value of type null"));
        assert_eq!(text("offset_on_null", (8, 3)).map(|t| t.1), Some("Trying to access array offset on null"));
        assert_eq!(text("dynamic_property", (8, 1)), None);
        assert!(text("dynamic_property", (8, 5)).is_some());
    }
}
