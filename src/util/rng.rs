//! Keyed RNG derivation, weighted picks, token alphabets and a few
//! distributions. Every generator in the crate draws from these.

use std::fmt::Write as _;

use hmac::{Hmac, KeyInit, Mac};
use rand::{Rng, RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub const ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
pub const LOWER_ALNUM: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
pub const UPPER_ALNUM: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
pub const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
pub const B64URL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
pub const BCRYPT: &[u8] = b"./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
pub const SYMBOLIC: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789!@#%^*-_=+";

/// Token alphabets by name (used by templates and secrets alike).
pub fn alphabet(name: &str) -> Option<&'static [u8]> {
    Some(match name {
        "alnum" => ALNUM,
        "lower" => LOWER_ALNUM,
        "upper" => UPPER_ALNUM,
        "hex" => b"0123456789abcdef",
        "upper_hex" => b"0123456789ABCDEF",
        "b64" => B64,
        "b64url" => B64URL,
        "bcrypt" => BCRYPT,
        "symbolic" => SYMBOLIC,
        "salt" => b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#%^&*()-_[]{}<>~+=,.;:/?|",
        _ => return None,
    })
}

/// HMAC-SHA256 over the concatenation of `parts` (each length-prefixed so
/// `["ab","c"]` and `["a","bc"]` never collide).
pub fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac accepts any key length");
    for p in parts {
        mac.update(&(p.len() as u32).to_le_bytes());
        mac.update(p);
    }
    mac.finalize().into_bytes().into()
}

pub fn keyed_rng(key: &[u8], parts: &[&[u8]]) -> ChaCha8Rng {
    ChaCha8Rng::from_seed(hmac(key, parts))
}

pub fn pick<T: Copy>(rng: &mut impl Rng, items: &[T]) -> T {
    items[rng.random_range(0..items.len())]
}

pub fn weighted<T: Copy>(rng: &mut impl Rng, items: &[(T, u32)]) -> T {
    let total: u32 = items.iter().map(|(_, w)| *w).sum();
    let mut roll = rng.random_range(0..total.max(1));
    for (item, w) in items {
        if roll < *w {
            return *item;
        }
        roll -= w;
    }
    items[items.len() - 1].0
}

/// Weighted pick from a `key -> weight` map (deterministic iteration order).
pub fn weighted_key<K: Copy>(rng: &mut impl Rng, items: &std::collections::BTreeMap<K, u32>) -> K {
    let flat: Vec<(K, u32)> = items.iter().map(|(k, w)| (*k, *w)).collect();
    weighted(rng, &flat)
}

/// Weighted pick of a string key, borrowing from the map.
pub fn weighted_str<'a>(rng: &mut impl Rng, items: &'a std::collections::BTreeMap<String, u32>) -> &'a str {
    let flat: Vec<(&'a str, u32)> = items.iter().map(|(k, w)| (k.as_str(), *w)).collect();
    weighted(rng, &flat)
}

/// Uniform pick from a list of owned strings, borrowing the result.
pub fn pick_str<'a>(rng: &mut impl Rng, items: &'a [String]) -> &'a str {
    items[rng.random_range(0..items.len())].as_str()
}

pub fn chance(rng: &mut impl Rng, p: f64) -> bool {
    rng.random_bool(p.clamp(0.0, 1.0))
}

pub fn token(rng: &mut impl Rng, alphabet: &[u8], len: usize) -> String {
    (0..len).map(|_| alphabet[rng.random_range(0..alphabet.len())] as char).collect()
}

pub fn hex(rng: &mut impl Rng, bytes: usize) -> String {
    let mut s = String::with_capacity(bytes * 2);
    for _ in 0..bytes {
        let _ = write!(s, "{:02x}", rng.random::<u8>());
    }
    s
}

pub fn uuid(rng: &mut impl Rng) -> String {
    let h = hex(rng, 16);
    format!("{}-{}-4{}-{}{}-{}", &h[0..8], &h[8..12], &h[13..16], pick(rng, &['8', '9', 'a', 'b']), &h[17..20], &h[20..32])
}

/// Standard normal sample (Box–Muller).
pub fn normal(rng: &mut impl Rng) -> f64 {
    let u1: f64 = rng.random_range(f64::EPSILON..1.0);
    let u2: f64 = rng.random();
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

/// Log-normal sample with the given median and shape.
pub fn lognormal(rng: &mut impl Rng, median: f64, sigma: f64) -> f64 {
    median * (sigma * normal(rng)).exp()
}

/// A random public-looking IPv4 address (skips private, loopback, CGNAT,
/// multicast and documentation ranges).
pub fn public_ipv4(rng: &mut impl Rng) -> String {
    loop {
        let a: u8 = rng.random_range(1..224);
        let b: u8 = rng.random();
        let private = matches!(a, 10 | 127 | 0)
            || (a == 172 && (16..32).contains(&b))
            || (a == 192 && b == 168)
            || (a == 100 && (64..128).contains(&b))
            || (a == 169 && b == 254)
            || (a == 192 && b == 0)
            || (a == 198 && (18..20).contains(&b));
        if !private {
            return format!("{a}.{b}.{}.{}", rng.random::<u8>(), rng.random_range(1..255u8));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lognormal_median() {
        let mut rng = keyed_rng(b"k", &[b"t"]);
        let mut v: Vec<f64> = (0..2001).map(|_| lognormal(&mut rng, 100.0, 0.6)).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!((80.0..125.0).contains(&v[1000]));
    }

    #[test]
    fn keyed_rng_is_deterministic_and_separated() {
        let a = token(&mut keyed_rng(b"k", &[b"ab", b"c"]), ALNUM, 16);
        let b = token(&mut keyed_rng(b"k", &[b"ab", b"c"]), ALNUM, 16);
        let c = token(&mut keyed_rng(b"k", &[b"a", b"bc"]), ALNUM, 16);
        assert_eq!(a, b);
        assert_ne!(a, c, "length prefixing must separate parts");
    }

    #[test]
    fn weighted_respects_zero_weights() {
        let mut rng = keyed_rng(b"w", &[]);
        for _ in 0..500 {
            assert_ne!(weighted(&mut rng, &[("a", 1), ("never", 0), ("b", 3)]), "never");
        }
    }

    #[test]
    fn public_ipv4_skips_private_ranges() {
        let mut rng = keyed_rng(b"ip", &[]);
        for _ in 0..2000 {
            let ip: std::net::Ipv4Addr = public_ipv4(&mut rng).parse().unwrap();
            assert!(!ip.is_private() && !ip.is_loopback() && !ip.is_link_local(), "{ip}");
        }
    }

    #[test]
    fn uuid_shape() {
        let u = uuid(&mut keyed_rng(b"u", &[]));
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
    }
}
