//! The team's formatting habits, secret style and timing temperament.

use std::ops::Range;

use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use crate::data::StackFacts;
use crate::persona::model::{Pace, Quote, Regime, SecretStyle, Sections, Style, Timing};
use crate::util::{self, chance, lognormal, weighted, weighted_key};

pub fn style(rng: &mut ChaCha8Rng, facts: &StackFacts) -> Style {
    Style {
        crlf: chance(rng, facts.crlf),
        env_export: chance(rng, 0.08),
        env_quote: weighted(rng, &[(Quote::Bare, 45), (Quote::WhenNeeded, 30), (Quote::Double, 18), (Quote::Single, 7)]),
        env_spaced: chance(rng, 0.05),
        sections: weighted(rng, &[(Sections::BlankLines, 6), (Sections::Comments, 3), (Sections::Dense, 1)]),
        comment_density: rng.random_range(0.0..0.35),
        trailing_newline: chance(rng, 0.85),
        indent: weighted(rng, &[("    ", 6), ("\t", 3), ("  ", 2)]),
        php_double_quotes: chance(rng, 0.2),
        php_short_arrays: chance(rng, 0.6),
        json_pretty: weighted(rng, &[(None, 6), (Some(2), 3), (Some(4), 1)]),
        naming: weighted_key(rng, &facts.naming),
    }
}

pub fn secrets(rng: &mut ChaCha8Rng) -> SecretStyle {
    SecretStyle {
        alphabet: weighted(rng, &[(util::ALNUM, 6), (util::SYMBOLIC, 2), (util::LOWER_ALNUM, 2)]),
        len: rng.random_range(14..33),
        rotation: rng.random_range(1..7),
    }
}

pub fn timing(rng: &mut ChaCha8Rng) -> Timing {
    Timing {
        // Medians of ~200-500 ms at the default 350 ms.
        ttfb_factor: rng.random_range(0.6..1.4),
        ttfb_sigma: rng.random_range(0.2..0.5),
        body_lag: rng.random_range(0.05..1.2),
        tick_factor: rng.random_range(0.4..1.8),
        request_sigma: rng.random_range(0.1..0.5),
        regimes: Pace::ALL.map(|p| regime(rng, &REGIMES[p as usize])),
        burst_after_stall: rng.random_range(0.3..0.9),
        abort_factor: rng.random_range(0.3..1.7),
        abort_stages: [rng.random_range(1..6), rng.random_range(1..6), rng.random_range(1..6)],
        fast_prob: rng.random_range(0.01..0.12),
        idle_share: rng.random_range(0.55..0.95),
        hiccup_prob: rng.random_range(0.01..0.06),
        rto_ms: rng.random_range(180.0..320.0),
        chunk_scale: lognormal(rng, 1.0, 0.45).clamp(0.4, 2.5),
        frame_bytes: util::pick(rng, &[2048, 4096, 4096, 8192, 8192, 8192, 16384, 32768]),
    }
}

/// Where each [`Pace`]'s parameters are drawn from, in `Pace::ALL` order.
/// Gaps are relative to the instance's tick, chunks to `chunk_bytes`.
struct RegimeRanges {
    gap: Range<f64>,
    gap_sigma: Range<f64>,
    chunk: Range<f64>,
    chunk_sigma: Range<f64>,
    dwell: Range<f64>,
    weight: Range<u32>,
}

const REGIMES: [RegimeRanges; Pace::ALL.len()] = [
    // Brisk
    RegimeRanges { gap: 0.05..0.3, gap_sigma: 0.2..0.6, chunk: 1.5..4.5, chunk_sigma: 0.3..0.8, dwell: 2.0..12.0, weight: 1..6 },
    // Steady
    RegimeRanges { gap: 0.6..1.4, gap_sigma: 0.1..0.4, chunk: 0.6..1.6, chunk_sigma: 0.2..0.5, dwell: 4.0..18.0, weight: 2..7 },
    // Sluggish
    RegimeRanges { gap: 2.0..6.0, gap_sigma: 0.2..0.6, chunk: 0.05..0.4, chunk_sigma: 0.3..0.8, dwell: 3.0..15.0, weight: 1..6 },
    // Stalled
    RegimeRanges { gap: 10.0..30.0, gap_sigma: 0.3..0.8, chunk: 0.02..0.15, chunk_sigma: 0.3..0.9, dwell: 1.0..3.0, weight: 1..3 },
];

fn regime(rng: &mut ChaCha8Rng, r: &RegimeRanges) -> Regime {
    Regime {
        gap: rng.random_range(r.gap.clone()),
        gap_sigma: rng.random_range(r.gap_sigma.clone()),
        chunk: rng.random_range(r.chunk.clone()),
        chunk_sigma: rng.random_range(r.chunk_sigma.clone()),
        dwell: rng.random_range(r.dwell.clone()),
        weight: rng.random_range(r.weight.clone()),
    }
}
