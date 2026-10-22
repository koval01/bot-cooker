//! A raw HTTP/1.1 client for the tests: bytes in, bytes out, so the tests
//! see exactly what a scanner sees on the wire.

#![allow(dead_code)]

/// A parsed response.
pub struct Reply {
    pub status: u16,
    /// The status line and header block as sent.
    pub head: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Header names exactly as written, in order.
    pub fn names(&self) -> Vec<&str> {
        self.headers.iter().map(|(k, _)| k.as_str()).collect()
    }
}

/// A request with a Host header and `Connection: close`.
pub fn request(method: &str, path: &str, host: &str, extra: &[(&str, &str)]) -> Vec<u8> {
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    for (k, v) in extra {
        req += &format!("{k}: {v}\r\n");
    }
    req += "\r\n";
    req.into_bytes()
}

/// Parse the first response in `raw` (identity, chunked or close-delimited).
pub fn parse(raw: &[u8], head_only: bool) -> Reply {
    let split =
        raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or_else(|| panic!("no complete head in {:?}", String::from_utf8_lossy(raw)));
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap().split(' ').nth(1).unwrap().parse().unwrap();
    let headers: Vec<(String, String)> =
        lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.to_string(), v.trim().to_string())).collect();
    let rest = &raw[split + 4..];
    let chunked = headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("transfer-encoding") && v.eq_ignore_ascii_case("chunked"));
    let length = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse::<usize>().ok());
    let body = if head_only {
        Vec::new()
    } else if chunked {
        dechunk(rest)
    } else if let Some(n) = length {
        rest[..n.min(rest.len())].to_vec()
    } else {
        rest.to_vec()
    };
    Reply { status, head, headers, body }
}

/// Decode as many complete chunks as arrived (a capped stream may end early).
pub fn dechunk(mut data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(eol) = data.windows(2).position(|w| w == b"\r\n") {
        let size = usize::from_str_radix(String::from_utf8_lossy(&data[..eol]).split(';').next().unwrap().trim(), 16).unwrap_or(0);
        data = &data[eol + 2..];
        if size == 0 || data.len() < size {
            out.extend_from_slice(&data[..size.min(data.len())]);
            break;
        }
        out.extend_from_slice(&data[..size]);
        data = data.get(size + 2..).unwrap_or_default();
    }
    out
}

/// Whether a chunked body ended with its closing chunk.
pub fn chunked_complete(raw: &[u8]) -> bool {
    raw.ends_with(b"0\r\n\r\n")
}
