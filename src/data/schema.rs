//! `schemas.yaml` and `config_files.yaml`: database and config layouts.

use serde::Deserialize;

/// One stack's tables (see the header of schemas.yaml).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaFile {
    #[serde(default)]
    pub naming: bool,
    #[serde(default)]
    pub cms: bool,
    pub extends: Option<String>,
    pub tables: Vec<TableSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableSpec {
    pub name: String,
    /// `[name, type, nullable, src?]`
    pub columns: Vec<Vec<serde_norway::Value>>,
    pub rows: Option<RowsSpec>,
    pub generate: Option<GenerateSpec>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum RowsSpec {
    Kind(String),
    Literal(Vec<Vec<serde_norway::Value>>),
}

/// `count`: `"a..b"` (persona-fixed pick) or `staff`; `row`: templates with `i`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerateSpec {
    pub count: String,
    pub row: Vec<String>,
}

/// File name -> template for leaked configs.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFiles {
    pub files: Vec<ConfigFile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    #[serde(rename = "match")]
    pub pattern: String,
    pub template: String,
}
