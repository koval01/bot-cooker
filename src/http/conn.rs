//! One client connection: read requests the way the front server would,
//! answer protocol errors as that server does, keep the connection alive by
//! its rules, and hand everything else to the handler.

use std::net::SocketAddr;
use std::time::Duration;

use http::{Method, StatusCode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{AppState, Stream, handler, request};
use crate::data::WireFacts;
use crate::engine::{Incarnation, RequestInfo};
use crate::tarpit::{self, Exchange, Outcome};
use crate::util::Tm;
use crate::wire::{self, Persist, ReadError, RequestHead};

/// Request body bytes we read and discard before giving up on keep-alive.
const BODY_DRAIN_LIMIT: u64 = 8 * 1024 * 1024;

pub async fn connection(app: AppState, mut sock: impl Stream, peer: SocketAddr) {
    let mut buf = Vec::new();
    let mut served: u32 = 0;
    loop {
        let inc = app.engine.current();
        let wire = &inc.persona.data.server(inc.persona.frontend).wire;
        let wait = if served == 0 { wire.header_timeout_secs } else { wire.keepalive_secs };
        let head = match tokio::time::timeout(Duration::from_secs(wait), wire::read_head(&mut sock, &mut buf, wire)).await {
            Ok(Ok(head)) => head,
            // Idle keep-alive, a silent client or a reset: just close.
            Ok(Err(ReadError::Closed | ReadError::Io)) | Err(_) => return,
            Ok(Err(e)) => {
                let status = match e {
                    ReadError::UriTooLong => wire.uri_too_long,
                    ReadError::HeadTooLarge => wire.head_too_large,
                    ReadError::Version => 505,
                    _ => 400,
                };
                refuse(&app, &inc, &mut sock, peer, None, status).await;
                return;
            }
        };
        served += 1;
        if let Some(status) = protocol_error(&head, wire) {
            refuse(&app, &inc, &mut sock, peer, Some(&head), status).await;
            return;
        }
        let body_read = match drain_body(&mut sock, &mut buf, &head, wire).await {
            Ok(read) => read,
            Err(Some(status)) => {
                refuse(&app, &inc, &mut sock, peer, Some(&head), status).await;
                return;
            }
            Err(None) => return,
        };
        let keep = body_read && head.wants_keep_alive() && served < wire.keepalive_max;
        let persist = if keep { Persist::KeepAlive { left: wire.keepalive_max - served } } else { Persist::Close };
        let outcome = handler::handle(&app, &inc, peer, &head, persist, &mut sock).await;
        if outcome != Outcome::Complete || !keep {
            let _ = sock.shutdown().await;
            return;
        }
    }
}

/// What the server itself rejects before routing.
fn protocol_error(head: &RequestHead, wire: &WireFacts) -> Option<u16> {
    if head.minor >= 1 && !head.headers.contains_key(http::header::HOST) {
        return Some(400);
    }
    let target = head.path_and_query();
    let path = target.split('?').next().unwrap_or_default();
    let star = head.method == Method::OPTIONS && head.target == "*";
    if !star && !path.starts_with('/') {
        return Some(400);
    }
    if !wire.cleans_paths && escapes_root(path) {
        return Some(400);
    }
    if head.content_length().is_some_and(|l| l.is_err()) {
        return Some(400);
    }
    None
}

/// `/../x` and its percent-encoded forms climb above the document root;
/// nginx, Apache and IIS refuse that outright.
fn escapes_root(path: &str) -> bool {
    let decoded = path.replace("%2e", ".").replace("%2E", ".").replace("%2f", "/").replace("%2F", "/").replace('\\', "/");
    let mut depth: i32 = 0;
    for seg in decoded.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            _ => depth += 1,
        }
    }
    false
}

/// Read and discard the request body. `Ok(false)`: the body was not fully
/// read, so the connection cannot be reused. `Err(Some(status))`: the server
/// refuses it; `Err(None)`: the client went away.
async fn drain_body(sock: &mut impl Stream, buf: &mut Vec<u8>, head: &RequestHead, wire: &WireFacts) -> Result<bool, Option<u16>> {
    if head.chunked() {
        // Not worth decoding: answer, then close.
        buf.clear();
        return Ok(false);
    }
    let Some(Ok(len)) = head.content_length() else { return Ok(true) };
    if len > wire.max_body {
        return Err(Some(413));
    }
    let from_buf = (len as usize).min(buf.len());
    buf.drain(..from_buf);
    let mut left = len - from_buf as u64;
    if left > BODY_DRAIN_LIMIT {
        return Ok(false);
    }
    let mut chunk = [0u8; 8192];
    while left > 0 {
        let want = (left as usize).min(chunk.len());
        match tokio::time::timeout(Duration::from_secs(wire.header_timeout_secs), sock.read(&mut chunk[..want])).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => return Err(None),
            Ok(Ok(n)) => left -= n as u64,
        }
    }
    Ok(true)
}

/// The server's own error page, then close.
async fn refuse(app: &AppState, inc: &Incarnation, sock: &mut impl Stream, peer: SocketAddr, head: Option<&RequestHead>, status: u16) {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST);
    let method = head.map(|h| h.method.clone()).unwrap_or(Method::GET);
    let host = head.and_then(|h| request::host(&h.headers, peer));
    let https = head.is_some_and(|h| request::is_https(&h.headers, peer));
    let info = RequestInfo { method: &method, path: "/", query: None, host: host.as_deref(), https };
    let art = app.engine.server_error(inc, status, &info);
    let wire = &inc.persona.data.server(inc.persona.frontend).wire;
    let ex = Exchange {
        wire,
        persist: Persist::Close,
        http10: head.is_some_and(|h| h.minor == 0),
        head_only: method == Method::HEAD,
        date: Tm::now().http(),
        proxied: request::from_proxy(peer),
    };
    let ip = head.map(|h| request::client_ip(&h.headers, peer)).unwrap_or(peer.ip());
    let _guard = app.stats.open(ip);
    tarpit::instant(sock, &ex, art).await;
    linger(sock).await;
}

/// Close the way nginx's lingering close does: stop writing, then read and
/// drop what the client is still sending for a short while, so the answer is
/// not lost to a reset when unread data hits a closed socket.
async fn linger(sock: &mut impl Stream) {
    let _ = sock.shutdown().await;
    let mut sink = [0u8; 8192];
    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        let mut total = 0usize;
        while let Ok(n) = sock.read(&mut sink).await {
            total += n;
            if n == 0 || total > 4 * 1024 * 1024 {
                break;
            }
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn climbing_above_the_root_is_caught() {
        assert!(escapes_root("/../etc/passwd"));
        assert!(escapes_root("/a/../../b"));
        assert!(escapes_root("/%2e%2e/%2e%2e/etc/passwd"));
        assert!(escapes_root("/..%2fetc"));
        assert!(!escapes_root("/a/b/../c"));
        assert!(!escapes_root("/.env"));
        assert!(!escapes_root("/a..b/"));
    }
}
