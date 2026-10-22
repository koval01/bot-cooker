//! Per-request timing, sampled from the instance's temperament and scaled
//! by the operator's `tarpit` config.

use std::time::Duration;

use rand::{Rng, RngExt};

use super::cadence::Cadence;
use crate::config::TarpitConfig;
use crate::persona::Timing;
use crate::render::Artifact;
use crate::util::{chance, lognormal, weighted};

/// Where a planned abort happens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Abort {
    Never,
    BeforeBody,
    AtByte(u64),
}

/// Per-request timing, sampled from the instance's temperament.
#[derive(Clone, Debug)]
pub struct Plan {
    pub pre_headers: Duration,
    pub(super) pre_body: Duration,
    pub(super) fast: bool,
    pub(super) abort: Abort,
    /// HTTP chunk size of a chunked body (the stack's buffer).
    pub(super) frame_bytes: usize,
    cadence: Cadence,
}

impl Plan {
    pub fn sample(cfg: &TarpitConfig, t: &Timing, art: &Artifact, rng: &mut impl Rng) -> Plan {
        let cap_ms = (cfg.max_seconds * 1000) as f64;
        let cadence = Cadence::sample(cfg, t, rng);
        let idle = idle_ceiling(cfg, t);
        // The operator's delay bounds every wait, so a zero delay means none;
        // and no wait may look like a dead server.
        let ceiling = cap_ms.min(cfg.initial_delay_ms as f64 * 6.0).min(idle);

        if art.fast || chance(rng, t.fast_prob) {
            let ttfb = lognormal(rng, 90.0, 0.6).clamp(5.0_f64.min(ceiling), 2_000.0_f64.min(ceiling));
            return Plan {
                pre_headers: Duration::from_millis((ttfb * 0.8) as u64),
                pre_body: Duration::from_millis((ttfb * 0.2) as u64),
                fast: true,
                abort: Abort::Never,
                frame_bytes: t.frame_bytes,
                cadence,
            };
        }

        // The application's response time puts the headers out; the page
        // then starts after a further lag of its own.
        let sigma = t.ttfb_sigma * (0.5 + cfg.initial_delay_jitter.clamp(0.0, 1.0));
        let ttfb = lognormal(rng, cfg.initial_delay_ms as f64 * t.ttfb_factor, sigma).clamp(30.0_f64.min(ceiling), ceiling);
        let lag = lognormal(rng, ttfb * t.body_lag, sigma).min(ceiling);

        let p_abort = (cfg.abort_probability * t.abort_factor).clamp(0.0, 0.9);
        let head = art.head.len() as u64;
        let abort = if p_abort > 0.0 && chance(rng, p_abort) {
            let [a, b, c] = t.abort_stages;
            match weighted(rng, &[(0u8, a), (1, b), (2, c)]) {
                0 => Abort::BeforeBody,
                1 if head > 1 => Abort::AtByte(rng.random_range(1..head)),
                _ if art.stream.is_some() => Abort::AtByte(head + rng.random_range(256..(cfg.max_bytes / 4).max(512))),
                _ if head > 1 => Abort::AtByte(rng.random_range(head / 2..head).max(1)),
                _ => Abort::BeforeBody,
            }
        } else {
            Abort::Never
        };

        Plan {
            pre_headers: Duration::from_millis(ttfb as u64),
            pre_body: Duration::from_millis(lag as u64),
            fast: false,
            abort,
            frame_bytes: t.frame_bytes,
            cadence,
        }
    }

    #[cfg(test)]
    pub fn pre_body_ms(&self) -> u128 {
        self.pre_body.as_millis()
    }

    #[cfg(test)]
    pub fn is_fast(&self) -> bool {
        self.fast
    }

    /// The drip cadence this response starts with.
    pub fn cadence(&self) -> Cadence {
        self.cadence.clone()
    }
}

/// The longest silence this instance allows itself, in milliseconds.
pub(super) fn idle_ceiling(cfg: &TarpitConfig, t: &Timing) -> f64 {
    cfg.max_idle_ms as f64 * t.idle_share
}
