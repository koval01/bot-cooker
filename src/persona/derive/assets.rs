//! The static files a persona's pages link to, and the look they share.

use std::collections::BTreeMap;

use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::tech::Tech;
use crate::data::{AssetSpec, Data, assets_slots};
use crate::persona::model::{AssetFile, Assets, Brand, IconShape};
use crate::util::{self, B64URL, pick, pick_str, weighted_key};

pub fn derive(rng: &mut ChaCha8Rng, data: &'static Data, tech: &Tech) -> Assets {
    let spec = data.assets.stack(tech.stack.id());
    let favicon = weighted_key(rng, &spec.favicon);
    let picks: BTreeMap<String, String> =
        data.assets.picks.iter().map(|(name, list)| (name.clone(), pick_str(rng, list).to_string())).collect();
    let major = tech.versions.framework.major;
    let files = spec
        .files
        .iter()
        .map(|f| {
            let file = file(rng, f, major, &picks);
            (f.name.clone(), file)
        })
        .collect();
    Assets { files, favicon, brand: brand(rng, data), picks }
}

fn file(rng: &mut ChaCha8Rng, spec: &'static AssetSpec, major: u32, picks: &BTreeMap<String, String>) -> AssetFile {
    let pattern = match &spec.older {
        Some(o) if major < o.below => &o.path,
        _ => &spec.path,
    };
    let path = fill(rng, pattern, picks);
    let href = match &spec.query {
        Some(q) => format!("{path}?{}", fill(rng, q, picks)),
        None => path.clone(),
    };
    AssetFile { path, href, body: spec.body.as_str() }
}

/// `{hex:8}` -> 8 hex digits, `{b64:8}` -> 8 base64url characters,
/// `{pick:list}` -> the persona's pick, `{semver}` -> a release number.
fn fill(rng: &mut ChaCha8Rng, pattern: &str, picks: &BTreeMap<String, String>) -> String {
    let mut out = pattern.to_string();
    for slot in assets_slots(pattern) {
        let value = match slot.split_once(':') {
            Some(("hex", n)) => util::hex(rng, n.parse::<usize>().unwrap_or(8).div_ceil(2))[..n.parse().unwrap_or(8)].to_string(),
            Some(("b64", n)) => util::token(rng, B64URL, n.parse().unwrap_or(8)),
            Some(("pick", list)) => picks.get(list).cloned().unwrap_or_default(),
            _ => format!("{}.{}.{}", rng.random_range(1..4), rng.random_range(0..10), rng.random_range(0..12)),
        };
        out = out.replacen(&format!("{{{slot}}}"), &value, 1);
    }
    out
}

fn brand(rng: &mut ChaCha8Rng, data: &'static Data) -> Brand {
    let hue = rng.random_range(0.0..360.0);
    let accent_hue = hue + pick(rng, &[30.0, 150.0, 180.0, 210.0, -30.0]);
    let dark = rng.random_bool(0.15);
    Brand {
        primary: hsl(hue, rng.random_range(0.45..0.8), rng.random_range(0.3..0.48)),
        accent: hsl(accent_hue, rng.random_range(0.5..0.85), rng.random_range(0.42..0.58)),
        ink: if dark {
            hsl(hue, 0.1, rng.random_range(0.86..0.94))
        } else {
            hsl(hue, rng.random_range(0.05..0.2), rng.random_range(0.1..0.22))
        },
        paper: if dark {
            hsl(hue, 0.12, rng.random_range(0.07..0.14))
        } else {
            hsl(hue, rng.random_range(0.0..0.3), rng.random_range(0.96..1.0))
        },
        font: pick_str(rng, &data.assets.fonts),
        radius: pick(rng, &[0, 2, 3, 4, 6, 8, 12]),
        width: pick(rng, &[960, 1040, 1100, 1140, 1200, 1280]),
        icon: pick(rng, IconShape::ALL),
    }
}

/// `#rrggbb` for a hue in degrees, saturation and lightness in 0..1.
fn hsl(h: f64, s: f64, l: f64) -> String {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let byte = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsl_hits_the_primaries() {
        assert_eq!(hsl(0.0, 1.0, 0.5), "#ff0000");
        assert_eq!(hsl(120.0, 1.0, 0.5), "#00ff00");
        assert_eq!(hsl(240.0, 1.0, 0.5), "#0000ff");
        assert_eq!(hsl(-120.0, 1.0, 0.5), "#0000ff");
        assert_eq!(hsl(0.0, 0.0, 1.0), "#ffffff");
    }

    #[test]
    fn placeholders_are_filled() {
        let mut rng = util::keyed_rng(&[7; 32], &[b"t"]);
        let picks = BTreeMap::from([("theme".to_string(), "astra".to_string())]);
        let out = fill(&mut rng, "/t/{pick:theme}/app-{hex:12}.{b64:8}.css", &picks);
        let (_, rest) = out.split_once("/t/astra/app-").unwrap();
        let (hex, rest) = rest.split_once('.').unwrap();
        assert_eq!(hex.len(), 12);
        assert!(hex.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        assert_eq!(rest.len(), "12345678.css".len());
    }
}
