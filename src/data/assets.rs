//! `templates/data/assets.yaml`: the static files each stack's pages link to.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::Weights;
use crate::persona::Favicon;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetData {
    pub picks: BTreeMap<String, Vec<String>>,
    /// By stack id; `default` for stacks without an entry.
    pub stacks: BTreeMap<String, StackAssets>,
    pub fonts: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackAssets {
    pub favicon: Weights<Favicon>,
    pub files: Vec<AssetSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetSpec {
    pub name: String,
    pub path: String,
    pub query: Option<String>,
    pub older: Option<Older>,
    pub body: String,
}

/// The path older framework releases used instead.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Older {
    pub below: u32,
    pub path: String,
}

impl AssetData {
    pub fn stack(&self, id: &str) -> &StackAssets {
        self.stacks.get(id).unwrap_or(&self.stacks["default"])
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.stacks.contains_key("default"), "assets: no default entry");
        anyhow::ensure!(!self.fonts.is_empty(), "assets: no fonts");
        for (stack, a) in &self.stacks {
            anyhow::ensure!(!a.favicon.is_empty(), "assets {stack}: no favicon weights");
            anyhow::ensure!(!a.files.is_empty(), "assets {stack}: no files");
            for f in &a.files {
                let paths = std::iter::once(&f.path).chain(f.older.as_ref().map(|o| &o.path));
                for text in paths.chain(f.query.as_ref()) {
                    for slot in slots(text) {
                        let ok = match slot.split_once(':') {
                            Some(("hex" | "b64", n)) => n.parse::<usize>().is_ok_and(|n| n > 0),
                            Some(("pick", list)) => self.picks.get(list).is_some_and(|l| !l.is_empty()),
                            _ => slot == "semver",
                        };
                        anyhow::ensure!(ok, "assets {stack}/{}: bad placeholder {{{slot}}}", f.name);
                    }
                }
            }
        }
        Ok(())
    }
}

/// Placeholders in a path pattern: `/a-{hex:8}.css` -> `hex:8`.
pub fn slots(pattern: &str) -> impl Iterator<Item = &str> {
    pattern.split('{').skip(1).filter_map(|s| s.split_once('}').map(|(slot, _)| slot))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_are_extracted() {
        assert_eq!(slots("/a-{hex:8}.{b64:4}.css").collect::<Vec<_>>(), ["hex:8", "b64:4"]);
        assert_eq!(slots("/plain.css").count(), 0);
    }
}
