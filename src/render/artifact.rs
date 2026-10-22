//! What rendering hands to the tarpit.

use http::StatusCode;

use super::scope::Req;
use crate::persona::LogKind;
use crate::world::Epochs;

/// A rendered response, ready for the tarpit.
pub struct Artifact {
    pub status: StatusCode,
    /// Ordered response headers (`date` is added by the HTTP stack).
    pub headers: Vec<(String, String)>,
    pub head: Vec<u8>,
    /// Continuation blocks streamed after `head`, if any.
    pub stream: Option<Continuation>,
    /// Bytes that close the document cleanly (`]}`, `</urlset>`, footer of a
    /// dump). Sent when the stream finishes or hits a cap, never on abort.
    pub tail: Vec<u8>,
    /// Advertise an exact `Content-Length` (finite, static-file style bodies).
    pub fixed_length: bool,
    /// Serve without tarpitting (robots.txt is bait that must be read).
    pub fast: bool,
}

/// Everything needed to regenerate block `seq` of a streamed body, frozen at
/// request time so an epoch rollover mid-stream cannot tear the document.
#[derive(Clone, Debug)]
pub struct Continuation {
    pub kind: StreamKind,
    pub blocks: u64,
    pub req: Req,
    pub epochs: Epochs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamKind {
    Log(LogKind),
    Sql,
    Json,
    Sitemap,
}
