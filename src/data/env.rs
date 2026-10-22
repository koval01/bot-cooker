//! `templates/data/env/*.yaml` (what goes into a stack's `.env`) and
//! `templates/data/notes.yaml` (comments people leave in config files).

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvFile {
    /// Ordered groups (stacks); `common.yaml` keys shared groups by name.
    #[serde(default)]
    pub groups: EnvGroups,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum EnvGroups {
    List(Vec<EnvGroup>),
    Named(BTreeMap<String, Vec<EnvVar>>),
}

impl Default for EnvGroups {
    fn default() -> Self {
        EnvGroups::List(Vec::new())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvGroup {
    pub name: String,
    #[serde(default)]
    pub vars: Vec<EnvVar>,
    /// Take the vars of a shared group from `common.yaml`.
    #[serde(rename = "use")]
    pub uses: Option<String>,
    pub when: Option<String>,
    /// Value of `{{ region_key }}` inside a shared group.
    pub region_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvVar {
    pub key: String,
    pub value: String,
    #[serde(default = "always")]
    pub p: f64,
    pub when: Option<String>,
}

fn always() -> f64 {
    1.0
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteData {
    pub patterns: Vec<NotePattern>,
    pub parts: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotePattern {
    pub weight: u32,
    pub text: String,
}

impl EnvFile {
    pub fn list(&self) -> &[EnvGroup] {
        match &self.groups {
            EnvGroups::List(l) => l,
            EnvGroups::Named(_) => &[],
        }
    }

    pub fn named(&self, name: &str) -> &[EnvVar] {
        match &self.groups {
            EnvGroups::Named(m) => m.get(name).map(Vec::as_slice).unwrap_or(&[]),
            EnvGroups::List(_) => &[],
        }
    }
}
