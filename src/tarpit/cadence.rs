//! The drip's cadence within one response: a walk between the instance's
//! delivery regimes (brisk, steady, sluggish, stalled), each with its own
//! log-normal gap and chunk size, so a client timing the chunks sees
//! stretches of different behaviour instead of one narrow band. On top of
//! that the path itself hiccups now and then, as a lossy link does: a
//! retransmission stall, then what was held back arrives in one go.

use std::time::Duration;

use rand::{Rng, RngExt};

use super::plan::idle_ceiling;
use crate::config::TarpitConfig;
use crate::persona::{Pace, Regime, Timing};
use crate::util::{chance, lognormal, weighted};

/// Longest single pause between two chunks, whatever the config says.
const MAX_PAUSE_MS: f64 = 120_000.0;
/// A pause that would exceed the instance's idle ceiling lands somewhere in
/// this share of it instead: long, but never the same ceiling every time.
const OVER_IDLE: std::ops::Range<f64> = 0.55..0.98;
/// Largest chunk, as a multiple of the operator's `chunk_bytes`.
const MAX_CHUNK_FACTOR: f64 = 8.0;
/// A chunk that would pass that size lands somewhere in this share of it,
/// so no instance piles its large pieces up at one recognisable size.
const OVER_CHUNK: std::ops::Range<f64> = 0.55..0.98;
/// Per-request chunk scale spread, relative to the pace spread.
const CHUNK_SPREAD_SHARE: f64 = 0.5;
/// Retransmission timeouts a hiccup doubles through (1 = one timeout).
const BACKOFFS: std::ops::RangeInclusive<u32> = 1..=3;
/// Chunks' worth of data a hiccup holds back and then flushes together.
const HELD_BACK: std::ops::RangeInclusive<u32> = 2..=5;
/// The pause after a flush, relative to a normal one: the link catches up.
const AFTER_FLUSH: f64 = 0.1;

/// One step of the drip: how much to send, then how long to wait.
#[derive(Clone, Copy, Debug)]
pub struct Beat {
    pub chunk: usize,
    pub pause: Duration,
}

/// The cadence of one response, already scaled to this request.
#[derive(Clone, Debug)]
pub struct Cadence {
    regimes: [Regime; Pace::ALL.len()],
    tick_ms: f64,
    chunk_bytes: f64,
    chunk_max: f64,
    sigma_scale: f64,
    burst_after_stall: f64,
    /// Longest silence allowed, in milliseconds.
    idle_ms: f64,
    hiccup_prob: f64,
    rto_ms: f64,
    /// Chunks held back by the last hiccup, flushed with the next beat.
    held: u32,
    pace: Pace,
    /// Chunks left before the current regime ends.
    left: u32,
}

impl Cadence {
    /// Scale the instance's regimes to the operator's config and draw this
    /// request's overall pace and starting regime.
    pub(super) fn sample(cfg: &TarpitConfig, t: &Timing, rng: &mut impl Rng) -> Cadence {
        let chunk_bytes = cfg.chunk_bytes.max(1) as f64;
        let chunk_scale = lognormal(rng, 1.0, t.request_sigma * CHUNK_SPREAD_SHARE);
        let mut c = Cadence {
            regimes: t.regimes,
            tick_ms: cfg.delay_ms as f64 * t.tick_factor * lognormal(rng, 1.0, t.request_sigma),
            chunk_bytes: chunk_bytes * t.chunk_scale * chunk_scale,
            chunk_max: (chunk_bytes * MAX_CHUNK_FACTOR).max(1.0),
            sigma_scale: 0.5 + cfg.delay_jitter.clamp(0.0, 1.0),
            burst_after_stall: t.burst_after_stall,
            idle_ms: idle_ceiling(cfg, t).min(MAX_PAUSE_MS),
            // No waiting configured, no waiting invented.
            hiccup_prob: if cfg.delay_ms == 0 { 0.0 } else { t.hiccup_prob },
            rto_ms: t.rto_ms,
            held: 0,
            pace: Pace::Steady,
            left: 0,
        };
        // The time-to-first-byte was the wait; a response opens moving.
        let start = c.pick(rng, |p| p != Pace::Stalled);
        c.enter(rng, start);
        c
    }

    pub fn next(&mut self, rng: &mut impl Rng) -> Beat {
        if self.left == 0 {
            let next = if self.pace == Pace::Stalled && chance(rng, self.burst_after_stall) {
                Pace::Brisk
            } else {
                let from = self.pace;
                self.pick(rng, |p| p != from)
            };
            self.enter(rng, next);
        }
        self.left -= 1;
        let r = &self.regimes[self.pace as usize];
        let mut chunk = lognormal(rng, self.chunk_bytes * r.chunk, r.chunk_sigma);
        let mut pause = lognormal(rng, self.tick_ms * r.gap, r.gap_sigma * self.sigma_scale).clamp(0.0, MAX_PAUSE_MS);
        if self.held > 0 {
            // The retransmission got through: the held-back data flushes.
            chunk *= f64::from(self.held + 1);
            pause *= AFTER_FLUSH;
            self.held = 0;
        } else if chance(rng, self.hiccup_prob) {
            let backoffs = rng.random_range(BACKOFFS);
            pause += self.rto_ms * f64::from((1u32 << backoffs) - 1) * rng.random_range(0.9..1.2);
            self.held = rng.random_range(HELD_BACK);
        }
        if pause > self.idle_ms {
            pause = self.idle_ms * rng.random_range(OVER_IDLE);
        }
        if chunk > self.chunk_max {
            chunk = self.chunk_max * rng.random_range(OVER_CHUNK);
        }
        let chunk = chunk.round().max(1.0);
        Beat { chunk: chunk as usize, pause: Duration::from_millis(pause as u64) }
    }

    #[cfg(test)]
    pub fn pace(&self) -> Pace {
        self.pace
    }

    /// A regime by the instance's weights, among those `allowed`.
    fn pick(&self, rng: &mut impl Rng, allowed: impl Fn(Pace) -> bool) -> Pace {
        let weights = Pace::ALL.map(|p| (p, if allowed(p) { self.regimes[p as usize].weight.max(1) } else { 0 }));
        weighted(rng, &weights)
    }

    /// Enter `pace` for a geometric number of chunks around its dwell.
    fn enter(&mut self, rng: &mut impl Rng, pace: Pace) {
        let dwell = self.regimes[pace as usize].dwell.max(1.0);
        let u: f64 = rng.random_range(f64::EPSILON..1.0);
        self.pace = pace;
        self.left = (-dwell * u.ln()).ceil().clamp(1.0, u32::MAX as f64) as u32;
    }
}
