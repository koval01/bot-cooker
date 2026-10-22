//! HTTP/1.1 on the wire, written the way the persona's front server writes
//! it. A generic HTTP stack gives every instance the same header case and
//! order, the same status line for HTTP/1.0 clients and the same bare answer
//! to garbage, whatever the persona claims to run; this layer takes all of
//! that from `servers.yaml` instead.

mod request;
mod response;

pub use request::{ReadError, RequestHead, parse_head, read_head};
pub use response::{Framing, Head, LAST_CHUNK, Persist, cased, chunk, encode};
