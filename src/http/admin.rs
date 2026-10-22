//! The token-gated JSON stats endpoint.

use http::HeaderMap;

use crate::stats::Stats;

/// Token from `X-Admin-Token` or `?token=`; an empty expected token never matches.
pub fn authorized(want: &str, headers: &HeaderMap, query: Option<&str>) -> bool {
    if want.is_empty() {
        return false;
    }
    let header_ok = headers.get("x-admin-token").and_then(|v| v.to_str().ok()).is_some_and(|h| same(h, want));
    let query_ok = query.is_some_and(|q| q.split('&').any(|pair| pair.strip_prefix("token=").is_some_and(|t| same(t, want))));
    header_ok || query_ok
}

/// Compare without stopping at the first differing byte, so response timing
/// says nothing about how much of a guessed token was right.
fn same(given: &str, want: &str) -> bool {
    let (a, b) = (given.as_bytes(), want.as_bytes());
    let mut diff = a.len() ^ b.len();
    for (i, &w) in b.iter().enumerate() {
        diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ w);
    }
    diff == 0
}

/// The stats document served at `admin_path`.
pub fn stats_json(stats: &Stats) -> String {
    let top: Vec<serde_json::Value> = stats
        .top_ips(20)
        .into_iter()
        .map(|(ip, rec)| {
            serde_json::json!({
                "ip": ip.to_string(),
                "requests": rec.requests,
                "bytes": rec.bytes,
                "first_seen": rec.first_seen,
                "last_seen": rec.last_seen,
            })
        })
        .collect();
    let body = serde_json::json!({ "stats": stats.snapshot(), "top_ips": top });
    serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_via_header_or_query() {
        let mut h = HeaderMap::new();
        assert!(!authorized("s3cret", &h, None));
        assert!(authorized("s3cret", &h, Some("a=1&token=s3cret")));
        assert!(!authorized("s3cret", &h, Some("token=s3cre")));
        h.insert("x-admin-token", "s3cret".parse().unwrap());
        assert!(authorized("s3cret", &h, None));
        assert!(!authorized("", &h, Some("token=")));
        assert!(same("abc", "abc"));
        assert!(!same("abd", "abc") && !same("ab", "abc") && !same("abcd", "abc") && !same("", "abc"));
    }
}
