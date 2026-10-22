//! `templates/data/world.yaml`: the shapes of users and business records.

use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldData {
    pub email_domains: Vec<String>,
    pub staff_roles: Vec<String>,
    pub customer_roles: Vec<String>,
    pub currencies: Vec<String>,
    pub phone_codes: Vec<u16>,
    pub api_key_names: Vec<String>,
    pub title_words: Vec<String>,
    pub record_shapes: Vec<RecordShape>,
    pub default_shape: RecordShape,
    pub statuses: Vec<Statuses>,
    pub default_statuses: Vec<String>,
}

/// Which fields a group of resources has.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordShape {
    pub resources: Vec<String>,
    pub owner: bool,
    pub fields: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Statuses {
    pub resources: Vec<String>,
    pub values: Vec<String>,
}

impl WorldData {
    pub fn shape(&self, resource: &str) -> &RecordShape {
        self.record_shapes.iter().find(|s| s.resources.iter().any(|r| r == resource)).unwrap_or(&self.default_shape)
    }

    pub fn statuses(&self, resource: &str) -> &[String] {
        self.statuses
            .iter()
            .find(|s| s.resources.iter().any(|r| r == resource))
            .map(|s| s.values.as_slice())
            .unwrap_or(&self.default_statuses)
    }
}
