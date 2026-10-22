//! Writing a response head the way the persona's front server writes it.

use http::StatusCode;

use crate::data::{HeaderCase, KeepAliveStyle, WireFacts};

/// How the body is delimited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Framing {
    Length(u64),
    Chunked,
    /// Until the connection closes (HTTP/1.0 clients, streamed bodies).
    Close,
}

/// The connection state the head announces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Persist {
    /// Kept open; `left` more requests allowed on it.
    KeepAlive {
        left: u32,
    },
    Close,
}

pub struct Head<'a> {
    pub status: StatusCode,
    /// Lowercase names, in the order the application produced them.
    pub headers: &'a [(String, String)],
    pub framing: Framing,
    pub persist: Persist,
    /// `Date` in IMF-fixdate form.
    pub date: &'a str,
    /// The request was HTTP/1.0.
    pub http10: bool,
}

/// Status line and headers, ready to write.
pub fn encode(w: &WireFacts, head: &Head) -> Vec<u8> {
    let mut all: Vec<(String, String)> = head.headers.iter().filter(|(n, _)| !is_hop(n)).cloned().collect();
    all.push(("date".into(), head.date.to_string()));
    match head.framing {
        Framing::Length(n) => all.push(("content-length".into(), n.to_string())),
        Framing::Chunked => all.push(("transfer-encoding".into(), "chunked".into())),
        Framing::Close => {}
    }
    match (w.keep_alive, head.persist) {
        (_, Persist::Close) => all.push(("connection".into(), "close".into())),
        (KeepAliveStyle::Explicit, Persist::KeepAlive { .. }) => all.push(("connection".into(), "keep-alive".into())),
        (KeepAliveStyle::Counted, Persist::KeepAlive { left }) => {
            all.push(("keep-alive".into(), format!("timeout={}, max={left}", w.keepalive_secs)));
            all.push(("connection".into(), "Keep-Alive".into()));
        }
        (KeepAliveStyle::Implicit, Persist::KeepAlive { .. }) => {}
    }
    let ordered = order(w, all);

    let version = if head.http10 && w.echo_version { "HTTP/1.0" } else { "HTTP/1.1" };
    let reason = w.reasons.get(&head.status.as_u16()).map(String::as_str).or(head.status.canonical_reason()).unwrap_or("Unknown");
    let mut out = format!("{version} {} {reason}\r\n", head.status.as_u16());
    for (name, value) in ordered {
        out.push_str(&cased(&name, w.case));
        out.push_str(": ");
        out.push_str(&value);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    out.into_bytes()
}

/// Headers the wire layer owns; an application never sets them itself.
fn is_hop(name: &str) -> bool {
    matches!(name, "date" | "content-length" | "transfer-encoding" | "connection" | "keep-alive")
}

fn order(w: &WireFacts, mut all: Vec<(String, String)>) -> Vec<(String, String)> {
    if w.sorted {
        all.sort_by(|a, b| a.0.cmp(&b.0));
        return all;
    }
    let take = |all: &mut Vec<(String, String)>, names: &[String]| -> Vec<(String, String)> {
        let mut out = Vec::new();
        for name in names {
            let (hit, rest): (Vec<_>, Vec<_>) = std::mem::take(all).into_iter().partition(|(n, _)| n == name);
            out.extend(hit);
            *all = rest;
        }
        out
    };
    let mut out = take(&mut all, &w.first);
    let last = take(&mut all, &w.last);
    out.extend(all);
    out.extend(last);
    out
}

/// Names whose usual spelling is not plain title case.
const SPELLINGS: &[&str] = &[
    "ETag",
    "WWW-Authenticate",
    "X-XSS-Protection",
    "X-UA-Compatible",
    "X-AspNet-Version",
    "X-AspNetMvc-Version",
    "X-DNS-Prefetch-Control",
    "Content-MD5",
    "P3P",
    "DNT",
    "TE",
];

/// A header name in the server's spelling.
pub fn cased(name: &str, case: HeaderCase) -> String {
    match case {
        HeaderCase::Lower if !matches!(name, "connection" | "keep-alive") => name.to_ascii_lowercase(),
        HeaderCase::Title | HeaderCase::Lower => {
            SPELLINGS.iter().find(|s| s.eq_ignore_ascii_case(name)).map(|s| s.to_string()).unwrap_or_else(|| canonical(name))
        }
        HeaderCase::Go => canonical(name),
    }
}

/// `x-powered-by` → `X-Powered-By` (Go's `CanonicalMIMEHeaderKey`).
fn canonical(name: &str) -> String {
    let mut up = true;
    name.chars()
        .map(|c| {
            let out = if up { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() };
            up = c == '-';
            out
        })
        .collect()
}

/// One chunk of a chunked body.
pub fn chunk(data: &[u8]) -> Vec<u8> {
    let mut out = format!("{:x}\r\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\r\n");
    out
}

/// The chunk that ends a chunked body.
pub const LAST_CHUNK: &[u8] = b"0\r\n\r\n";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Data;
    use crate::persona::Frontend;

    fn wire(f: Frontend) -> &'static WireFacts {
        &Data::embedded().server(f).wire
    }

    fn head_for(f: Frontend, persist: Persist, framing: Framing, http10: bool) -> String {
        let headers = vec![
            ("server".to_string(), "srv".to_string()),
            ("content-type".to_string(), "text/html".to_string()),
            ("x-powered-by".to_string(), "PHP/8.3.1".to_string()),
            ("etag".to_string(), "\"1\"".to_string()),
        ];
        let h = Head { status: StatusCode::OK, headers: &headers, framing, persist, date: "Thu, 22 Oct 2026 09:00:00 GMT", http10 };
        String::from_utf8(encode(wire(f), &h)).unwrap()
    }

    fn names(text: &str) -> Vec<&str> {
        text.lines().skip(1).take_while(|l| !l.is_empty()).map(|l| l.split(':').next().unwrap()).collect()
    }

    #[test]
    fn nginx_order_and_spelling() {
        let t = head_for(Frontend::Nginx, Persist::KeepAlive { left: 9 }, Framing::Length(5), false);
        assert!(t.starts_with("HTTP/1.1 200 OK\r\n"));
        assert_eq!(names(&t), ["Server", "Date", "Content-Type", "Content-Length", "Connection", "X-Powered-By", "ETag"]);
        assert!(t.contains("Connection: keep-alive\r\n"));
    }

    #[test]
    fn apache_counts_keep_alive_and_puts_content_type_last() {
        let t = head_for(Frontend::Apache, Persist::KeepAlive { left: 99 }, Framing::Chunked, false);
        assert_eq!(names(&t), ["Date", "Server", "X-Powered-By", "ETag", "Transfer-Encoding", "Keep-Alive", "Connection", "Content-Type"]);
        assert!(t.contains("Keep-Alive: timeout=5, max=99\r\nConnection: Keep-Alive\r\n"));
    }

    #[test]
    fn go_sorts_canonicalises_and_echoes_http10() {
        let t = head_for(Frontend::Caddy, Persist::Close, Framing::Close, true);
        assert!(t.starts_with("HTTP/1.0 200 OK\r\n"));
        assert_eq!(names(&t), ["Connection", "Content-Type", "Date", "Etag", "Server", "X-Powered-By"]);
    }

    #[test]
    fn iis_and_litespeed() {
        let t = head_for(Frontend::Iis, Persist::KeepAlive { left: 5 }, Framing::Length(0), true);
        assert!(t.starts_with("HTTP/1.1 200 OK\r\n"), "IIS answers 1.0 clients with 1.1");
        assert_eq!(names(&t), ["Content-Type", "ETag", "Server", "X-Powered-By", "Date", "Content-Length"]);
        let t = head_for(Frontend::LiteSpeed, Persist::KeepAlive { left: 3 }, Framing::Length(1), false);
        assert_eq!(names(&t), ["Connection", "Keep-Alive", "content-type", "x-powered-by", "etag", "content-length", "date", "server"]);
    }

    #[test]
    fn nginx_names_405_its_own_way() {
        let h = Head {
            status: StatusCode::METHOD_NOT_ALLOWED,
            headers: &[],
            framing: Framing::Length(0),
            persist: Persist::Close,
            date: "d",
            http10: false,
        };
        assert!(String::from_utf8(encode(wire(Frontend::Nginx), &h)).unwrap().starts_with("HTTP/1.1 405 Not Allowed\r\n"));
    }

    #[test]
    fn spelling() {
        assert_eq!(cased("www-authenticate", HeaderCase::Title), "WWW-Authenticate");
        assert_eq!(cased("www-authenticate", HeaderCase::Go), "Www-Authenticate");
        assert_eq!(cased("x-xss-protection", HeaderCase::Go), "X-Xss-Protection");
        assert_eq!(cased("set-cookie", HeaderCase::Lower), "set-cookie");
        assert_eq!(cased("keep-alive", HeaderCase::Lower), "Keep-Alive");
        assert_eq!(chunk(b"hello"), b"5\r\nhello\r\n");
    }
}
