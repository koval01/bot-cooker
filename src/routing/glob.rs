//! Minimal glob matching for route patterns.

/// Minimal glob: `*` matches any (possibly empty) run of characters. Everything
/// else is literal. Both inputs are expected lowercased.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    // Iterative two-pointer wildcard match (no regex dependency).
    let p = pattern.as_bytes();
    let t = text.as_bytes();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = pi;
            mark = ti;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("/.env", "/.env"));
        assert!(glob_match("*.sql", "/db/backup.sql"));
        assert!(glob_match("/.git/*", "/.git/config"));
        assert!(glob_match("*backup*", "/old-backup-2026.zip"));
        assert!(glob_match("/a/*/c", "/a/b/c"));
        assert!(glob_match("*", ""));
        assert!(!glob_match("/.env", "/.environment"));
        assert!(!glob_match("*.sql", "/index.html"));
        assert!(!glob_match("/a/*/c", "/a/b/d"));
    }
}
