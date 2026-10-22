//! Placeholders in data-file patterns such as `"{short}_session"` or
//! `"wp{rand4}_"`. Deliberately tiny: only named substitutions, no logic —
//! anything richer belongs in a template.

use rand::{Rng, RngExt};

use crate::util::{self, LOWER_ALNUM};

/// Values a pattern may refer to by name.
#[derive(Default)]
pub struct Vars<'a> {
    pub pairs: Vec<(&'a str, String)>,
}

impl<'a> Vars<'a> {
    pub fn with(mut self, name: &'a str, value: impl Into<String>) -> Self {
        self.pairs.push((name, value.into()));
        self
    }
}

/// Replace `{name}` with a named value, and the generated placeholders
/// `{randN}` (lowercase token), `{numN}` (N-digit number), `{hexN}` (N hex
/// chars) and `{rand32hex}`. Unknown names are left as they are.
pub fn fill(pattern: &str, vars: &Vars, rng: &mut impl Rng) -> String {
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut rest = pattern;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(len) = rest[start..].find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &rest[start + 1..start + len];
        out.push_str(&expand(name, vars, rng).unwrap_or_else(|| format!("{{{name}}}")));
        rest = &rest[start + len + 1..];
    }
    out.push_str(rest);
    out
}

fn expand(name: &str, vars: &Vars, rng: &mut impl Rng) -> Option<String> {
    if let Some((_, v)) = vars.pairs.iter().find(|(n, _)| *n == name) {
        return Some(v.clone());
    }
    if name == "rand32hex" {
        return Some(util::hex(rng, 16));
    }
    let width = |prefix: &str| name.strip_prefix(prefix).and_then(|n| n.parse::<usize>().ok());
    if let Some(n) = width("rand") {
        return Some(util::token(rng, LOWER_ALNUM, n));
    }
    if let Some(n) = width("hex") {
        return Some(util::token(rng, b"0123456789abcdef", n));
    }
    if let Some(n) = width("num") {
        let lo = 10u64.pow(n.saturating_sub(1) as u32);
        return Some(if n <= 1 { rng.random_range(1..10).to_string() } else { rng.random_range(lo..lo * 10).to_string() });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::keyed_rng;

    #[test]
    fn named_and_generated_placeholders() {
        let mut rng = keyed_rng(b"f", &[]);
        let vars = Vars::default().with("short", "acme");
        assert_eq!(fill("_{short}_session", &vars, &mut rng), "_acme_session");
        let p = fill("wp{rand4}_", &vars, &mut rng);
        assert!(p.starts_with("wp") && p.ends_with('_') && p.len() == 7);
        assert_eq!(fill("web{num2}", &vars, &mut rng).len(), 5);
        assert_eq!(fill("SSESS{rand32hex}", &vars, &mut rng).len(), 37);
    }

    #[test]
    fn unknown_and_unbalanced_are_kept() {
        let mut rng = keyed_rng(b"f", &[]);
        assert_eq!(fill("{nope}-{", &Vars::default(), &mut rng), "{nope}-{");
    }
}
