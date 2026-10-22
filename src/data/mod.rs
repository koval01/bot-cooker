//! Typed view of the data files (`templates/data/*.yaml`): what personas are
//! drawn from and what the instance's records look like.
//!
//! The data is loaded once per process and leaked into a `&'static`
//! reference: personas are re-derived from it (persona eras), and handing out
//! `&'static str` keeps the derived model cheap to copy around.

mod agents;
mod assets;
mod env;
mod facts;
mod php;
mod probes;
mod schema;
mod words;
mod world;

pub use agents::{AgentData, AgentShape};
pub use assets::{AssetData, AssetSpec, StackAssets, slots as assets_slots};
pub use env::{EnvFile, EnvGroup, EnvVar, NoteData};
pub use facts::*;
pub use php::{PhpErrorData, PhpStack, series as php_series};
pub use probes::ProbeData;
pub use schema::*;
pub use words::*;
pub use world::WorldData;

use std::collections::BTreeMap;
use std::sync::OnceLock;

use anyhow::Context;

use crate::catalog::Catalog;
use crate::persona::{Frontend, HostOs, Stack};

/// Relative weights keyed by a choice.
pub type Weights<K> = BTreeMap<K, u32>;

#[derive(Debug)]
pub struct Data {
    pub words: Words,
    pub stacks: BTreeMap<Stack, StackFacts>,
    pub servers: BTreeMap<Frontend, ServerFacts>,
    pub infra: Infra,
    pub world: WorldData,
    pub agents: AgentData,
    pub assets: AssetData,
    pub names: BTreeMap<String, NameSet>,
    pub probes: ProbeData,
    pub synonyms: Synonyms,
    /// `.env` layouts by stack id, plus `common` shared groups.
    pub env: BTreeMap<String, EnvFile>,
    pub notes: NoteData,
    pub config_files: ConfigFiles,
    /// What PHP prints into pages, per stack.
    pub php_errors: PhpErrorData,
    /// Database schemas by stack id (`default` for the rest).
    pub schemas: BTreeMap<String, SchemaFile>,
    /// Corpus sentences for the text model, by file stem.
    pub corpus: BTreeMap<String, String>,
}

impl Data {
    /// Load and validate the data files from a catalog, once per process.
    pub fn load(catalog: &Catalog) -> anyhow::Result<&'static Data> {
        let data = Data {
            words: catalog.data("data/words.yaml")?,
            stacks: catalog.data("data/stacks.yaml")?,
            servers: catalog.data("data/servers.yaml")?,
            infra: catalog.data("data/infra.yaml")?,
            world: catalog.data("data/world.yaml")?,
            agents: catalog.data("data/agents.yaml")?,
            assets: catalog.data("data/assets.yaml")?,
            names: catalog.data("data/names.yaml")?,
            probes: catalog.data("data/probes.yaml")?,
            synonyms: catalog.data("data/synonyms.yaml")?,
            env: catalog.data_dir("data/env/")?,
            notes: catalog.data("data/notes.yaml")?,
            config_files: catalog.data("data/config_files.yaml")?,
            php_errors: catalog.data("data/php_errors.yaml")?,
            schemas: catalog.data("data/schemas.yaml")?,
            corpus: catalog.texts("data/corpus/"),
        };
        data.validate()?;
        Ok(Box::leak(Box::new(data)))
    }

    /// The data compiled into the binary (tests and `--check`).
    pub fn embedded() -> &'static Data {
        static DATA: OnceLock<&'static Data> = OnceLock::new();
        DATA.get_or_init(|| {
            let catalog = Catalog::load(None).expect("embedded catalog compiles");
            Data::load(&catalog).expect("embedded persona data is valid")
        })
    }

    pub fn stack(&self, s: Stack) -> &StackFacts {
        &self.stacks[&s]
    }

    pub fn server(&self, f: Frontend) -> &ServerFacts {
        &self.servers[&f]
    }

    /// Every lookup the derivation does by key must succeed, and every list
    /// it picks from must be non-empty.
    fn validate(&self) -> anyhow::Result<()> {
        for s in Stack::ALL {
            let facts = self.stacks.get(s).with_context(|| format!("stacks.yaml lacks {s:?}"))?;
            anyhow::ensure!(
                !facts.versions.is_empty() && !facts.login_paths.is_empty() && !facts.api_prefixes.is_empty(),
                "{s:?}: empty list"
            );
            anyhow::ensure!(self.infra.runtimes.contains_key(&facts.family), "{s:?}: no runtimes for {:?}", facts.family);
            for f in facts.frontends.keys() {
                anyhow::ensure!(self.servers.contains_key(f), "{s:?}: unknown frontend {f:?}");
            }
            for d in facts.databases.keys() {
                anyhow::ensure!(self.infra.databases.contains_key(d), "{s:?}: unknown database {d:?}");
            }
        }
        for (f, facts) in &self.servers {
            anyhow::ensure!(
                facts.versions.contains_key("default") || facts.distros.keys().all(|d| facts.versions.contains_key(d.id())),
                "{f:?}: missing versions"
            );
        }
        for host in HostOs::ALL {
            let served =
                self.stacks.values().any(|s| s.frontends.keys().any(|f| self.servers[f].distros.keys().any(|d| d.host_os() == *host)));
            anyhow::ensure!(served, "no stack runs on a {host:?} host");
        }
        anyhow::ensure!(!self.words.industries.is_empty() && !self.infra.timezones.is_empty(), "empty word lists");
        self.agents.validate()?;
        self.assets.validate()?;
        for tz in &self.infra.timezones {
            for locale in tz.locales.keys() {
                let set = self.names.get(locale).with_context(|| format!("{}: unknown locale {locale}", tz.name))?;
                anyhow::ensure!(!set.first.is_empty() && !set.last.is_empty(), "locale {locale} has no names");
            }
        }
        anyhow::ensure!(!self.corpus.is_empty(), "data/corpus/ is empty");
        self.validate_versions()?;
        self.probes.validate()?;
        self.php_errors.validate()?;
        Ok(())
    }
}

impl Data {
    /// Every version entry must name real-looking releases (see
    /// `persona::version` for the forms).
    fn validate_versions(&self) -> anyhow::Result<()> {
        let mut lists: Vec<(String, &Vec<String>)> = Vec::new();
        for (s, f) in &self.stacks {
            lists.push((format!("stacks.yaml {s:?}"), &f.versions));
        }
        for (f, facts) in &self.servers {
            for (distro, list) in &facts.versions {
                lists.push((format!("servers.yaml {f:?} {distro}"), list));
            }
        }
        for (family, list) in &self.infra.runtimes {
            lists.push((format!("infra.yaml runtimes {family:?}"), list));
        }
        for (engine, db) in &self.infra.databases {
            lists.push((format!("infra.yaml databases {engine:?}"), &db.versions));
        }
        for (place, list) in lists {
            anyhow::ensure!(!list.is_empty(), "{place}: no versions");
            for v in list {
                anyhow::ensure!(crate::persona::valid_series(v), "{place}: bad version series {v:?}");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_data_is_complete() {
        let d = Data::embedded();
        assert_eq!(d.stacks.len(), Stack::ALL.len());
        assert!(d.names["de"].first.len() > 20);
        assert!(d.corpus.len() >= 3);
        assert!(d.stack(Stack::WordPress).cms);
        assert_eq!(d.stack(Stack::AspNet).family, crate::persona::Family::Dotnet);
    }

    #[test]
    fn validation_rejects_missing_stack() {
        let mut d = Data {
            words: Words::default(),
            stacks: BTreeMap::new(),
            servers: BTreeMap::new(),
            infra: Infra::default(),
            world: WorldData::default(),
            agents: AgentData::default(),
            assets: AssetData::default(),
            names: BTreeMap::new(),
            probes: ProbeData::default(),
            synonyms: Synonyms::default(),
            env: BTreeMap::new(),
            notes: NoteData::default(),
            config_files: ConfigFiles::default(),
            php_errors: PhpErrorData::default(),
            schemas: BTreeMap::new(),
            corpus: BTreeMap::new(),
        };
        assert!(d.validate().is_err());
        d.words.industries.clear();
        assert!(d.validate().is_err());
    }
}
