//! Request routing: pick a deception profile for a path, and decide whether a
//! client is on the (opt-in) allowlist.

mod allow;
mod glob;

pub use allow::AllowList;
pub use glob::glob_match;

use crate::config::{Profile, RouteRule};

/// Compiled routing table.
pub struct Router {
    rules: Vec<CompiledRule>,
}

struct CompiledRule {
    pattern: String,
    profile: Profile,
}

impl Router {
    pub fn new(rules: &[RouteRule]) -> Self {
        let rules = rules.iter().map(|r| CompiledRule { pattern: r.pattern.to_ascii_lowercase(), profile: r.profile }).collect();
        Self { rules }
    }

    /// Choose a profile for a request path. Paths that don't exist on this
    /// instance get the realistic 404 of its stack.
    pub fn profile_for(&self, path: &str) -> Profile {
        let p = path.to_ascii_lowercase();
        for rule in &self.rules {
            if glob_match(&rule.pattern, &p) {
                return rule.profile;
            }
        }
        Profile::NotFound
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(pattern: &str, profile: Profile) -> RouteRule {
        RouteRule { pattern: pattern.into(), profile }
    }

    #[test]
    fn first_match_wins_and_unknown_is_not_found() {
        let r = Router::new(&[rule("/.env", Profile::Env), rule("/.*", Profile::Forbidden), rule("/API/*", Profile::Json)]);
        assert_eq!(r.profile_for("/.env"), Profile::Env);
        assert_eq!(r.profile_for("/.htaccess"), Profile::Forbidden);
        assert_eq!(r.profile_for("/api/users"), Profile::Json, "patterns are case-insensitive");
        assert_eq!(r.profile_for("/nope"), Profile::NotFound);
        assert_eq!(r.len(), 3);
    }
}
