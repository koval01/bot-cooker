//! The tarpit response: a slow body whose timing has no single signature,
//! written straight to the socket in the front server's wire format.
//!
//! Timeline of a trapped request:
//!   1. part of the time-to-first-byte is spent before the status line, the
//!      rest before the first body byte (the split is per instance)
//!   2. chunks walk between delivery regimes (brisk, steady, sluggish,
//!      stalled), each with its own log-normal gap and chunk size
//!   3. streamed artifacts keep generating blocks until they are complete
//!   4. a clean end sends the closing `tail`; caps end it the same way; a
//!      planned abort cuts it before the body, inside the head, or later
//!   5. now and then a request is simply answered fast, like a warm cache
//!
//! All the per-instance knobs come from the persona's [`Timing`]; the
//! operator's `tarpit` config sets the overall scale and hard limits.
//!
//! [`Timing`]: crate::persona::Timing

mod cadence;
mod plan;

pub use cadence::{Beat, Cadence};
pub use plan::Plan;

use std::time::{Duration, Instant};

use rand_chacha::ChaCha8Rng;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use self::plan::Abort;
use crate::config::TarpitConfig;
use crate::data::WireFacts;
use crate::engine::{Engine, Incarnation};
use crate::render::Artifact;
use crate::stats::ConnGuard;
use crate::wire::{self, Framing, Head, Persist};

/// How a response ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Complete and well-framed: the connection may serve another request.
    Complete,
    /// Cut short on purpose, by a cap, or the client went away.
    Cut,
}

/// What the connection knows about how to frame this response.
pub struct Exchange<'a> {
    pub wire: &'a WireFacts,
    pub persist: Persist,
    pub http10: bool,
    pub head_only: bool,
    pub date: String,
    /// A reverse proxy in front re-frames the body itself and holds back a
    /// chunk until all of it has arrived, so behind one every piece goes as
    /// its own chunk to keep the trickle flowing.
    pub proxied: bool,
}

impl Exchange<'_> {
    /// The body framing a GET for this artifact would get; HEAD mirrors it.
    fn framing(&self, art: &Artifact) -> Framing {
        if art.fixed_length {
            Framing::Length((art.head.len() + art.tail.len()) as u64)
        } else if self.http10 {
            Framing::Close
        } else {
            Framing::Chunked
        }
    }

    fn head(&self, art: &Artifact, framing: Framing) -> Vec<u8> {
        // A close-delimited body ends with the connection.
        let persist = if framing == Framing::Close { Persist::Close } else { self.persist };
        let head = Head { status: art.status, headers: &art.headers, framing, persist, date: &self.date, http10: self.http10 };
        wire::encode(self.wire, &head)
    }
}

/// Where the body comes from: the operator's limits and the engine that
/// renders stream blocks for this incarnation.
pub struct Source<'a> {
    pub cfg: &'a TarpitConfig,
    pub engine: &'a Engine,
    pub inc: &'a Incarnation,
}

/// Answer at once (robots.txt, the concurrency cap, server errors).
pub async fn instant(io: &mut (impl AsyncWrite + Unpin), ex: &Exchange<'_>, art: Artifact) -> Outcome {
    let framing = ex.framing(&art);
    let mut out = ex.head(&art, framing);
    if !ex.head_only {
        let mut body = art.head;
        body.extend_from_slice(&art.tail);
        match framing {
            Framing::Chunked => {
                if !body.is_empty() {
                    out.extend(wire::chunk(&body));
                }
                out.extend_from_slice(wire::LAST_CHUNK);
            }
            _ => out.extend(body),
        }
    }
    let ok = io.write_all(&out).await.is_ok() && io.flush().await.is_ok();
    if ok && framing != Framing::Close { Outcome::Complete } else { Outcome::Cut }
}

/// The slow answer. `plan.pre_headers` has already been waited.
pub async fn respond(
    io: &mut (impl AsyncWrite + Unpin),
    ex: &Exchange<'_>,
    src: &Source<'_>,
    art: Artifact,
    plan: Plan,
    guard: &ConnGuard,
) -> Outcome {
    let framing = ex.framing(&art);
    if io.write_all(&ex.head(&art, framing)).await.is_err() {
        return Outcome::Cut;
    }
    if ex.head_only {
        return if io.flush().await.is_ok() { Outcome::Complete } else { Outcome::Cut };
    }
    let frame_bytes = if ex.proxied { 0 } else { plan.frame_bytes.max(1) };
    let mut body = Body { io, framing, guard, frame_bytes, frame_left: 0 };
    match drip(&mut body, src, art, &plan).await {
        Ok(true) => {
            if body.finish().await.is_ok() && framing != Framing::Close {
                Outcome::Complete
            } else {
                Outcome::Cut
            }
        }
        // An abort ends without the closing chunk: a truncated body.
        Ok(false) | Err(_) => Outcome::Cut,
    }
}

/// The body on the wire. In a chunked body the HTTP chunks are the size the
/// stack flushes (its buffer, or what is ready); only the delivery of their
/// bytes is slow, as on a poor link. Chunk sizes therefore look like any
/// other server's, not like the trickle.
struct Body<'a, W> {
    io: &'a mut W,
    framing: Framing,
    guard: &'a ConnGuard,
    /// `0`: one chunk per piece (behind a proxy).
    frame_bytes: usize,
    /// Bytes still owed to the HTTP chunk in progress.
    frame_left: usize,
}

impl<W: AsyncWrite + Unpin> Body<'_, W> {
    /// Send `data`, the next bytes of a buffer holding `ready` bytes from
    /// here on (what an HTTP chunk opened now may announce).
    async fn send(&mut self, data: &[u8], ready: usize) -> std::io::Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        if self.framing != Framing::Chunked {
            self.io.write_all(data).await?;
        } else if self.frame_bytes == 0 {
            self.io.write_all(&wire::chunk(data)).await?;
        } else {
            let (mut rest, mut ready) = (data, ready.max(data.len()));
            while !rest.is_empty() {
                let mut out = Vec::new();
                if self.frame_left == 0 {
                    self.frame_left = ready.min(self.frame_bytes);
                    out.extend(format!("{:x}\r\n", self.frame_left).into_bytes());
                }
                let n = rest.len().min(self.frame_left);
                out.extend_from_slice(&rest[..n]);
                self.frame_left -= n;
                if self.frame_left == 0 {
                    out.extend_from_slice(b"\r\n");
                }
                self.io.write_all(&out).await?;
                rest = &rest[n..];
                ready -= n;
            }
        }
        self.io.flush().await?;
        self.guard.fed(data.len() as u64);
        Ok(())
    }

    async fn finish(&mut self) -> std::io::Result<()> {
        if self.framing == Framing::Chunked {
            self.io.write_all(wire::LAST_CHUNK).await?;
        }
        self.io.flush().await
    }
}

/// Feed the body slowly. `Ok(true)`: the document was closed cleanly;
/// `Ok(false)`: a planned abort cut it.
async fn drip<W: AsyncWrite + Unpin>(body: &mut Body<'_, W>, src: &Source<'_>, art: Artifact, plan: &Plan) -> std::io::Result<bool> {
    let cfg = src.cfg;
    let mut rng = rand::make_rng::<ChaCha8Rng>();
    let deadline = Instant::now() + Duration::from_secs(cfg.max_seconds);
    let byte_cap = if plan.fast { cfg.max_bytes.min(256 * 1024) } else { cfg.max_bytes };
    let mut cadence = plan.cadence();

    tokio::time::sleep(plan.pre_body).await;
    let abort_at = match plan.abort {
        Abort::BeforeBody => return Ok(false),
        Abort::AtByte(n) => n,
        Abort::Never => u64::MAX,
    };

    // Pieces of the body, generated lazily: head, stream blocks, tail.
    let blocks = art.stream.as_ref().map(|c| c.blocks).unwrap_or(0);
    let mut seq: u64 = 0;
    let mut pending: Vec<u8> = art.head;
    let mut pos = 0usize;
    let mut sent: u64 = 0;
    let mut capped = false;
    loop {
        if !capped && (Instant::now() >= deadline || sent >= byte_cap) {
            // Out of budget: finish the block in hand (so the document can
            // still be closed cleanly) and generate nothing more.
            capped = true;
        }
        if pos >= pending.len() {
            match &art.stream {
                Some(c) if seq < blocks && !capped => {
                    pending = src.engine.continue_stream(&src.inc.persona, c, seq);
                    pos = 0;
                    seq += 1;
                    continue;
                }
                _ => break,
            }
        }
        let beat = if plan.fast || capped { None } else { Some(cadence.next(&mut rng)) };
        let n = beat.map_or(pending.len() - pos, |b| b.chunk.min(pending.len() - pos));
        let piece = &pending[pos..pos + n];
        if sent + piece.len() as u64 >= abort_at {
            let keep = (abort_at.saturating_sub(sent) as usize).min(piece.len());
            body.send(&piece[..keep], pending.len() - pos).await?;
            return Ok(false);
        }
        body.send(piece, pending.len() - pos).await?;
        pos += n;
        sent += n as u64;
        if let Some(b) = beat {
            tokio::time::sleep(b.pause).await;
        }
    }
    // A finished or capped document still ends well-formed.
    body.send(&art.tail, art.tail.len()).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::Stats;

    /// Chunk sizes declared in a chunked body.
    fn declared(raw: &[u8]) -> Vec<usize> {
        let mut data = raw;
        let mut out = Vec::new();
        while let Some(eol) = data.windows(2).position(|w| w == b"\r\n") {
            let size = usize::from_str_radix(std::str::from_utf8(&data[..eol]).unwrap(), 16).unwrap();
            out.push(size);
            data = &data[eol + 2 + size + 2..];
        }
        out
    }

    async fn framed(frame_bytes: usize, pieces: &[usize], total: usize) -> Vec<usize> {
        let stats = Stats::new(4);
        let guard = stats.open("192.0.2.1".parse().unwrap());
        let mut out: Vec<u8> = Vec::new();
        let mut body = Body { io: &mut out, framing: Framing::Chunked, guard: &guard, frame_bytes, frame_left: 0 };
        let data = vec![b'x'; total];
        let mut pos = 0;
        for &n in pieces.iter().cycle() {
            if pos >= total {
                break;
            }
            let n = n.min(total - pos);
            body.send(&data[pos..pos + n], total - pos).await.unwrap();
            pos += n;
        }
        declared(&out)
    }

    #[tokio::test]
    async fn direct_clients_see_buffer_sized_chunks() {
        // 10 000 bytes in pieces of 7-90: chunks of the stack's buffer size.
        assert_eq!(framed(4096, &[7, 48, 90, 13], 10_000).await, [4096, 4096, 1808]);
    }

    #[tokio::test]
    async fn behind_a_proxy_every_piece_is_a_chunk() {
        assert_eq!(framed(0, &[7, 48, 90], 145).await, [7, 48, 90]);
    }
}
