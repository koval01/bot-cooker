//! The instance's world: everything that "happened" on the imaginary system,
//! as pure functions of the persona, an id and the current epochs. Users,
//! business records, commits, posts and traffic live here; how they look in a
//! given file format is the templates' business.

pub mod agents;
pub mod content;
pub mod icon;
pub mod people;
pub mod records;
pub mod traffic;
pub mod vcs;

use rand_chacha::ChaCha8Rng;
use serde::Serialize;

use crate::data::Data;
use crate::persona::Persona;
use crate::util;

/// Where we are in time. Anchors are the (jittered) starts of the current
/// epochs and never lie in the future.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Epochs {
    pub cfg: u64,
    pub log: u64,
    pub cfg_anchor: i64,
    pub log_anchor: i64,
    /// Wall clock; only for per-request details such as cookie expiry.
    pub now: i64,
}

#[derive(Clone, Copy)]
pub struct World<'a> {
    pub persona: &'a Persona,
    pub epochs: Epochs,
}

impl<'a> World<'a> {
    pub fn new(persona: &'a Persona, epochs: Epochs) -> Self {
        World { persona, epochs }
    }

    pub fn data(&self) -> &'static Data {
        self.persona.data
    }

    /// A secret shared by every artifact of this instance (the DB password
    /// in `.env` is the one in `docker-compose.yml`), rotating every few
    /// config epochs.
    pub fn secret(&self, name: &str) -> String {
        let s = &self.persona.secrets;
        util::token(&mut util::keyed_rng(&self.secret_key(), &[name.as_bytes(), b"default"]), s.alphabet, s.len)
    }

    /// A secret in a named alphabet (`hex`, `b64`, ...; see `util::alphabet`).
    pub fn secret_as(&self, name: &str, kind: &str, len: usize) -> String {
        let alphabet = util::alphabet(kind).unwrap_or(util::ALNUM);
        util::token(&mut util::keyed_rng(&self.secret_key(), &[name.as_bytes(), kind.as_bytes()]), alphabet, len)
    }

    /// Key all secrets of the current rotation derive from (templates reach
    /// them through the `secret` object, Rust through `secret`/`secret_as`).
    pub fn secret_key(&self) -> [u8; 32] {
        let rotation = self.epochs.cfg / self.persona.secrets.rotation.max(1);
        util::hmac(&self.persona.mac(&[b"secret"]), &[&rotation.to_le_bytes()])
    }

    /// Persona-stable RNG, independent of epochs.
    pub fn fixed_rng(&self, label: &str) -> ChaCha8Rng {
        self.persona.rng(label)
    }

    /// RNG for config-like content (changes per config epoch).
    pub fn cfg_rng(&self, label: &str, key: &str) -> ChaCha8Rng {
        util::keyed_rng(&self.persona.mac(&[b"cfg"]), &[label.as_bytes(), key.as_bytes(), &self.epochs.cfg.to_le_bytes()])
    }

    /// RNG for live content (changes per log epoch).
    pub fn log_rng(&self, label: &str, key: &str) -> ChaCha8Rng {
        util::keyed_rng(&self.persona.mac(&[b"log"]), &[label.as_bytes(), key.as_bytes(), &self.epochs.log.to_le_bytes()])
    }

    /// When the company "went live" (a few months to several years ago).
    pub fn founded(&self) -> i64 {
        use rand::RngExt;
        self.epochs.cfg_anchor - self.fixed_rng("founded").random_range(200..3000) * 86_400
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// 2026-10-21 13:05:09 UTC.
    pub const NOW: i64 = 1_792_587_909;

    pub fn persona(n: u32) -> Persona {
        Persona::derive(&util::hmac(b"world", &[&n.to_le_bytes()]), 0, crate::persona::HostOs::Unix, Data::embedded())
    }

    pub fn epochs() -> Epochs {
        Epochs { cfg: 2_964, log: 62_249, cfg_anchor: NOW - 3 * 86_400, log_anchor: NOW - 3_600, now: NOW }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn secrets_are_shared_and_rotate() {
        let p = persona(1);
        let w = World::new(&p, epochs());
        assert_eq!(w.secret("db"), w.secret("db"));
        assert_ne!(w.secret("db"), w.secret("redis"));
        let later = World::new(&p, Epochs { cfg: epochs().cfg + 7, ..epochs() });
        assert_ne!(w.secret("db"), later.secret("db"), "rotation is at most 6 epochs");
    }

    #[test]
    fn founded_is_in_the_past() {
        let p = persona(2);
        let w = World::new(&p, epochs());
        assert!(w.founded() < w.epochs.cfg_anchor - 199 * 86_400);
    }
}
