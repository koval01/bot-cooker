//! Instance persona: the imaginary organisation and system an instance
//! pretends to be.
//!
//! A scanner that hits several bot-cooker hosts must not see the same "shape"
//! twice. So every instance derives, from its master key alone, one coherent
//! identity — company, domain, staff, tech stack, front server, versions,
//! database, formatting habits, timing temperament — and every artifact is a
//! projection of that identity. Two keys give two unrelated-looking systems;
//! one key gives the same system for as long as the persona era lasts (forever
//! by default). Small details (patch versions, secrets, log tails) still drift
//! with the shorter content epochs, see [`Ver::at`] and `Ctx::secret`.

mod derive;
pub mod fill;
mod model;
pub mod routes;
mod version;

pub use model::*;
pub use version::{Ver, valid_series};

use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use crate::data::Data;
use crate::util;

impl Persona {
    /// Derive the persona for `master_key` during persona era `era`, for a
    /// trap running on `host`.
    pub fn derive(master_key: &[u8; 32], era: u64, host: HostOs, data: &'static Data) -> Self {
        derive::derive(master_key, era, host, data)
    }

    /// A deterministic RNG scoped to this persona and a label.
    pub fn rng(&self, label: &str) -> ChaCha8Rng {
        util::keyed_rng(&self.seed, &[label.as_bytes()])
    }

    /// Raw keyed bytes scoped to this persona.
    pub fn mac(&self, parts: &[&[u8]]) -> [u8; 32] {
        util::hmac(&self.seed, parts)
    }

    /// An address inside the instance's private network.
    pub fn internal_ip(&self, host: u8) -> String {
        format!("{}.{}.{}.{}", self.net.0, self.net.1, self.net.2, host.max(2))
    }

    pub fn admin(&self) -> &Person {
        &self.staff[0]
    }

    /// A person's name in this company's locale mix (staff, customers).
    pub fn person_name(&self, rng: &mut impl rand::Rng) -> (&'static str, &'static str) {
        derive::name(rng, self.data, &self.name_mix)
    }

    /// A sentence of the persona's own prose, slots filled from its vocabulary.
    pub fn sentence(&self, rng: &mut impl rand::Rng, max_words: usize) -> String {
        let raw = self.text.sentence(rng, max_words);
        let voiced = self.voice.speak(&raw, rng, 0.12);
        self.fill_slots(&voiced, rng)
    }

    pub fn paragraph(&self, rng: &mut impl rand::Rng, sentences: usize) -> String {
        (0..sentences).map(|_| self.sentence(rng, 40)).collect::<Vec<_>>().join(" ")
    }

    fn fill_slots(&self, raw: &str, rng: &mut impl rand::Rng) -> String {
        let vocab = &self.site.vocabulary;
        let topic = vocab[rng.random_range(0..vocab.len())].replace('-', " ");
        let topic2 = vocab[rng.random_range(0..vocab.len())].replace('-', " ");
        let vars = fill::Vars::default()
            .with("topic", topic)
            .with("topic2", topic2)
            .with("company", self.company.clone())
            .with("product", self.app_name.clone())
            .with("resource", self.site.api_resources[rng.random_range(0..self.site.api_resources.len())].replace('_', " "));
        fill::fill(raw, &vars, rng)
    }

    pub fn is_php(&self) -> bool {
        self.family == Family::Php
    }

    pub fn summary(&self) -> String {
        format!("{} ({}) — {} on {}, {}", self.company, self.domain, self.stack_name, self.frontend.id(), self.db.engine.id())
    }

    /// Line ending for files this persona writes.
    pub fn eol(&self) -> &'static str {
        if self.style.crlf { "\r\n" } else { "\n" }
    }

    /// Name a field in the persona's (stack's) naming convention.
    pub fn field(&self, snake: &str) -> String {
        convert_case(snake, self.style.naming)
    }
}

impl Person {
    pub fn display(&self) -> String {
        format!("{} {}", capitalize(self.first), capitalize(self.last))
    }
}

pub fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub fn convert_case(snake: &str, naming: Naming) -> String {
    match naming {
        Naming::Snake => snake.to_string(),
        Naming::Camel | Naming::Pascal => {
            let mut out = String::new();
            for (i, part) in snake.split('_').enumerate() {
                if i == 0 && naming == Naming::Camel {
                    out.push_str(part);
                } else {
                    out.push_str(&capitalize(part));
                }
            }
            out
        }
    }
}

/// First component of a slug (`northwind-freight` -> `northwind`).
pub fn short_slug(slug: &str) -> &str {
    slug.split('-').next().unwrap_or(slug)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn key(n: u32) -> [u8; 32] {
        util::hmac(b"test", &[&n.to_le_bytes()])
    }

    fn persona(n: u32) -> Persona {
        Persona::derive(&key(n), 0, HostOs::Unix, Data::embedded())
    }

    #[test]
    fn servers_match_the_host_os() {
        let data = Data::embedded();
        for n in 0..200 {
            let unix = Persona::derive(&key(n), 0, HostOs::Unix, data);
            assert_eq!(unix.distro.host_os(), HostOs::Unix, "{}", unix.summary());
            assert_ne!(unix.frontend, Frontend::Iis, "{}", unix.summary());
            let win = Persona::derive(&key(n), 0, HostOs::Windows, data);
            assert_eq!(win.distro, Distro::Windows, "{}", win.summary());
        }
    }

    #[test]
    fn windows_personas_still_differ() {
        let data = Data::embedded();
        let personas: Vec<Persona> = (0..40).map(|n| Persona::derive(&key(n), 0, HostOs::Windows, data)).collect();
        let domains: HashSet<_> = personas.iter().map(|p| p.domain.clone()).collect();
        let routes: HashSet<_> =
            personas.iter().map(|p| format!("{:?}", p.routes().iter().map(|r| r.pattern.clone()).collect::<Vec<_>>())).collect();
        assert_eq!(domains.len(), 40);
        assert!(routes.len() >= 30, "Windows route tables too uniform: {}", routes.len());
    }

    #[test]
    fn stable_for_same_key() {
        let (a, b) = (persona(1), persona(1));
        assert_eq!(a.summary(), b.summary());
        assert_eq!(a.db.name, b.db.name);
        assert_eq!(a.site.env_paths, b.site.env_paths);
        assert_eq!(a.web.server_banner, b.web.server_banner);
    }

    #[test]
    fn era_changes_identity() {
        let data = Data::embedded();
        assert_ne!(Persona::derive(&key(1), 0, HostOs::Unix, data).summary(), Persona::derive(&key(1), 1, HostOs::Unix, data).summary());
    }

    #[test]
    fn varies_across_keys() {
        let personas: Vec<Persona> = (0..40).map(persona).collect();
        let stacks: HashSet<_> = personas.iter().map(|p| p.stack).collect();
        let domains: HashSet<_> = personas.iter().map(|p| p.domain.clone()).collect();
        let fronts: HashSet<_> = personas.iter().map(|p| p.frontend).collect();
        assert!(stacks.len() >= 7, "stacks: {stacks:?}");
        assert!(fronts.len() >= 4);
        assert_eq!(domains.len(), personas.len());
    }

    #[test]
    fn coherent_stack_and_server() {
        let data = Data::embedded();
        for i in 0..80 {
            let p = persona(i);
            let facts = data.stack(p.stack);
            assert!(facts.frontends.contains_key(&p.frontend), "{:?} behind {:?}", p.stack, p.frontend);
            assert!(facts.databases.contains_key(&p.db.engine));
            if let Some(pb) = &p.web.powered_by {
                assert!(!pb.starts_with("PHP/") || p.is_php());
            }
            assert!(p.staff.iter().all(|s| s.email.ends_with(&p.domain)));
            assert!(!p.staff.is_empty());
        }
    }

    #[test]
    fn denied_dotfiles_still_leak_something() {
        for i in 0..200 {
            let p = persona(i);
            if p.web.dotfiles == DotPolicy::Deny {
                assert!(p.site.env_paths.is_empty() && !p.site.git);
                assert!(!p.site.config_leaks.is_empty() || !p.site.backups.is_empty());
            }
        }
    }

    #[test]
    fn naming_conversion() {
        assert_eq!(convert_case("created_at", Naming::Camel), "createdAt");
        assert_eq!(convert_case("created_at", Naming::Pascal), "CreatedAt");
        assert_eq!(short_slug("north-wind"), "north");
    }
}
