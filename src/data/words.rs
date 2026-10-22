//! `words.yaml`, `names.yaml`, `synonyms.yaml`: vocabulary.

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Words {
    pub name_heads: Vec<String>,
    pub name_tails: Vec<String>,
    pub tlds: Vec<String>,
    pub internal_host_words: Vec<String>,
    pub commit_verbs: Vec<String>,
    pub commit_objects: Vec<String>,
    pub commit_tails: Vec<String>,
    pub commit_types: Vec<String>,
    pub filler: Vec<String>,
    pub industries: Vec<Industry>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Industry {
    pub id: String,
    pub suffixes: Vec<String>,
    pub resources: Vec<String>,
    pub vocabulary: Vec<String>,
    pub sells_online: bool,
}

/// Interchangeable words (see `text::Voice`).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Synonyms {
    pub groups: Vec<Vec<String>>,
    #[serde(default)]
    pub markers: Vec<String>,
}

/// First and last names of one locale.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NameSet {
    pub first: Vec<String>,
    pub last: Vec<String>,
}
