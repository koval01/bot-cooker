//! Reading a request head off the socket with the front server's limits.

use http::{HeaderMap, HeaderName, HeaderValue, Method};
use tokio::io::{AsyncRead, AsyncReadExt};

use crate::data::WireFacts;

/// Headers a request may carry before the parser gives up; real servers
/// allow about this many (nginx ~100 with default buffers).
const MAX_HEADERS: usize = 128;

#[derive(Debug)]
pub struct RequestHead {
    pub method: Method,
    /// The request-target exactly as sent.
    pub target: String,
    /// `1` for HTTP/1.1, `0` for HTTP/1.0.
    pub minor: u8,
    pub headers: HeaderMap,
}

impl RequestHead {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).and_then(|v| v.to_str().ok())
    }

    /// Whether the client asked to keep the connection open.
    pub fn wants_keep_alive(&self) -> bool {
        let tokens = self.header("connection").unwrap_or_default().to_ascii_lowercase();
        if self.minor >= 1 { !tokens.split(',').any(|t| t.trim() == "close") } else { tokens.split(',').any(|t| t.trim() == "keep-alive") }
    }

    /// The path part of an origin-form or absolute-form target.
    pub fn path_and_query(&self) -> &str {
        let t = self.target.as_str();
        match t.find("://") {
            Some(i) => t[i + 3..].find('/').map(|j| &t[i + 3 + j..]).unwrap_or("/"),
            None => t,
        }
    }

    pub fn content_length(&self) -> Option<Result<u64, ()>> {
        self.header("content-length").map(|v| v.trim().parse().map_err(|_| ()))
    }

    pub fn chunked(&self) -> bool {
        self.header("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"))
    }
}

/// Why no request could be read; the first three end the connection quietly,
/// the rest get the server's own error page.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadError {
    Closed,
    Io,
    Malformed,
    UriTooLong,
    HeadTooLarge,
    /// A version other than HTTP/1.x.
    Version,
}

/// Read and parse one request head. `buf` keeps bytes that arrived after it
/// (a pipelined request or the start of a body).
pub async fn read_head(io: &mut (impl AsyncRead + Unpin), buf: &mut Vec<u8>, w: &WireFacts) -> Result<RequestHead, ReadError> {
    loop {
        // Clients may send blank lines before a request; servers skip them.
        let skip = buf.iter().take_while(|b| matches!(b, b'\r' | b'\n')).count();
        buf.drain(..skip);
        if let Some(head) = parse_head(buf, w)? {
            return Ok(head);
        }
        let mut chunk = [0u8; 4096];
        let n = io.read(&mut chunk).await.map_err(|_| ReadError::Io)?;
        if n == 0 {
            return Err(if buf.is_empty() { ReadError::Closed } else { ReadError::Malformed });
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Parse one request head from the start of `buf` under the server's limits:
/// `Ok(None)` when more bytes are needed. A parsed head is removed from
/// `buf`, leaving whatever followed it.
pub fn parse_head(buf: &mut Vec<u8>, w: &WireFacts) -> Result<Option<RequestHead>, ReadError> {
    let line_end = buf.windows(2).position(|p| p == b"\r\n");
    if line_end.unwrap_or(buf.len()) > w.max_request_line {
        return Err(ReadError::UriTooLong);
    }
    let mut slots = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut req = httparse::Request::new(&mut slots);
    let consumed = match req.parse(buf) {
        Ok(httparse::Status::Complete(n)) => n,
        Ok(httparse::Status::Partial) => {
            return if buf.len() > w.max_head { Err(ReadError::HeadTooLarge) } else { Ok(None) };
        }
        Err(httparse::Error::Version) => {
            // `HTTP/2.0` and friends parse as a version error; garbage too.
            let line = &buf[..line_end.unwrap_or(buf.len())];
            return Err(if line.windows(5).any(|p| p == b"HTTP/") { ReadError::Version } else { ReadError::Malformed });
        }
        Err(httparse::Error::TooManyHeaders) => return Err(ReadError::HeadTooLarge),
        Err(_) => return Err(ReadError::Malformed),
    };
    if consumed > w.max_head {
        return Err(ReadError::HeadTooLarge);
    }
    let method = req.method.and_then(|m| Method::from_bytes(m.as_bytes()).ok()).ok_or(ReadError::Malformed)?;
    let target = req.path.ok_or(ReadError::Malformed)?.to_string();
    let minor = req.version.ok_or(ReadError::Malformed)?;
    let mut headers = HeaderMap::new();
    for h in req.headers.iter() {
        let name = HeaderName::from_bytes(h.name.as_bytes()).map_err(|_| ReadError::Malformed)?;
        let value = HeaderValue::from_bytes(h.value).map_err(|_| ReadError::Malformed)?;
        headers.append(name, value);
    }
    buf.drain(..consumed);
    Ok(Some(RequestHead { method, target, minor, headers }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Data;
    use crate::persona::Frontend;

    fn nginx() -> &'static WireFacts {
        &Data::embedded().server(Frontend::Nginx).wire
    }

    async fn parse(raw: &[u8]) -> (Result<RequestHead, ReadError>, Vec<u8>) {
        let mut io = raw;
        let mut buf = Vec::new();
        let r = read_head(&mut io, &mut buf, nginx()).await;
        (r, buf)
    }

    #[tokio::test]
    async fn parses_a_request_and_keeps_what_follows() {
        let (r, rest) = parse(b"\r\nGET /a?b=1 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\nGET /next").await;
        let h = r.unwrap();
        assert_eq!((h.method.as_str(), h.target.as_str(), h.minor), ("GET", "/a?b=1", 1));
        assert!(!h.wants_keep_alive());
        assert_eq!(rest, b"GET /next");
    }

    #[tokio::test]
    async fn absolute_targets_yield_their_path() {
        let h = parse(b"GET http://example.com/x/y?z HTTP/1.1\r\nHost: example.com\r\n\r\n").await.0.unwrap();
        assert_eq!(h.path_and_query(), "/x/y?z");
        let h = parse(b"GET http://example.com HTTP/1.0\r\n\r\n").await.0.unwrap();
        assert_eq!(h.path_and_query(), "/");
        assert!(!h.wants_keep_alive());
    }

    #[tokio::test]
    async fn bad_input_maps_to_the_server_answers() {
        assert_eq!(parse(b"\x16\x03\x01\x02\x00\x01\x00\x01\xfc\x03\x03").await.0.unwrap_err(), ReadError::Malformed);
        assert_eq!(parse(b"GET / HTTP/2.0\r\n\r\n").await.0.unwrap_err(), ReadError::Version);
        assert_eq!(parse(b"").await.0.unwrap_err(), ReadError::Closed);
        assert_eq!(parse(b"GET / HTTP/1.1\r\nHost").await.0.unwrap_err(), ReadError::Malformed);
        let long = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(9000));
        assert_eq!(parse(long.as_bytes()).await.0.unwrap_err(), ReadError::UriTooLong);
        let big = format!("GET / HTTP/1.1\r\n{}\r\n", "X-A: b\r\n".repeat(5000));
        assert_eq!(parse(big.as_bytes()).await.0.unwrap_err(), ReadError::HeadTooLarge);
    }
}
