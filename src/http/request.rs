//! What we trust from request headers: client address, Host, scheme.
//!
//! Forwarding headers (`X-Forwarded-*`, `X-Real-IP`) are only believed when
//! the connection comes from a proxy next to us: a loopback or private
//! address, as the Caddy sidecar or a proxy on the same host has. A client on
//! the internet that sends them is talking to us directly, and could otherwise
//! put any address into the statistics or any host into our links.

use std::net::{IpAddr, SocketAddr};

use http::{HeaderMap, header};

/// Whether `peer` is a reverse proxy we sit behind rather than a client.
pub fn from_proxy(peer: SocketAddr) -> bool {
    match peer.ip().to_canonical() {
        IpAddr::V4(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local(),
    }
}

/// A forwarding header, if the peer is a proxy that may set it.
fn forwarded<'a>(headers: &'a HeaderMap, peer: SocketAddr, name: &str) -> Option<&'a str> {
    if !from_proxy(peer) {
        return None;
    }
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// The client's address: the proxy's forwarding headers when we sit behind
/// one, the socket peer otherwise.
pub fn client_ip(headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
    let first_hop =
        |name: &str| forwarded(headers, peer, name).and_then(|v| v.split(',').next()).and_then(|v| v.trim().parse::<IpAddr>().ok());
    first_hop("x-forwarded-for").or_else(|| first_hop("x-real-ip")).unwrap_or_else(|| peer.ip())
}

/// The `Host` the client asked for, if it is a sane hostname (links in the
/// fake site are built from it). Anything odd falls back to the persona domain.
pub fn host(headers: &HeaderMap, peer: SocketAddr) -> Option<String> {
    let raw = forwarded(headers, peer, "x-forwarded-host").or_else(|| headers.get(header::HOST).and_then(|v| v.to_str().ok()))?;
    let host = raw.split(',').next()?.trim().to_ascii_lowercase();
    let ok = !host.is_empty()
        && host.len() <= 253
        && host.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'))
        && !host.starts_with(['.', '-', ':']);
    ok.then_some(host)
}

/// Whether the TLS-terminating proxy saw HTTPS.
pub fn is_https(headers: &HeaderMap, peer: SocketAddr) -> bool {
    forwarded(headers, peer, "x-forwarded-proto").is_some_and(|v| v.eq_ignore_ascii_case("https"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, v.parse().unwrap());
        }
        h
    }

    const PROXY: &str = "172.18.0.3:41000";
    const DIRECT: &str = "203.0.113.9:41000";

    fn at(addr: &str) -> SocketAddr {
        addr.parse().unwrap()
    }

    #[test]
    fn client_ip_comes_from_a_proxy_only() {
        let fwd = headers(&[("x-forwarded-for", "198.51.100.5, 10.0.0.1")]);
        assert_eq!(client_ip(&headers(&[]), at(PROXY)), at(PROXY).ip());
        assert_eq!(client_ip(&fwd, at(PROXY)).to_string(), "198.51.100.5");
        assert_eq!(client_ip(&fwd, at("127.0.0.1:5000")).to_string(), "198.51.100.5");
        assert_eq!(client_ip(&fwd, at("[::ffff:10.1.2.3]:5000")).to_string(), "198.51.100.5");
        assert_eq!(client_ip(&headers(&[("x-real-ip", "198.51.100.2")]), at(PROXY)).to_string(), "198.51.100.2");
        assert_eq!(client_ip(&headers(&[("x-forwarded-for", "garbage")]), at(PROXY)), at(PROXY).ip());
        // A client on the internet cannot claim another address.
        assert_eq!(client_ip(&fwd, at(DIRECT)), at(DIRECT).ip());
        assert_eq!(client_ip(&fwd, at("[2001:db8::7]:5000")).to_string(), "2001:db8::7");
    }

    #[test]
    fn host_is_sanitised() {
        let p = at(PROXY);
        assert_eq!(host(&headers(&[("host", "Shop.Example.COM:8443")]), p).as_deref(), Some("shop.example.com:8443"));
        assert_eq!(host(&headers(&[("host", "a.com"), ("x-forwarded-host", "b.com, c.com")]), p).as_deref(), Some("b.com"));
        assert_eq!(host(&headers(&[("host", "a.com"), ("x-forwarded-host", "b.com")]), at(DIRECT)).as_deref(), Some("a.com"));
        assert_eq!(host(&headers(&[("host", "evil\"<script>")]), p), None);
        assert_eq!(host(&headers(&[("host", ".dot")]), p), None);
        assert_eq!(host(&headers(&[]), p), None);
    }

    #[test]
    fn https_from_proxy() {
        assert!(is_https(&headers(&[("x-forwarded-proto", "HTTPS")]), at(PROXY)));
        assert!(!is_https(&headers(&[("x-forwarded-proto", "https")]), at(DIRECT)));
        assert!(!is_https(&headers(&[]), at(PROXY)));
    }
}
