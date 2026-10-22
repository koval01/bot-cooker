//! One request: route it, render it, and answer it slowly (or not).

use std::net::SocketAddr;

use http::{Method, StatusCode};

use super::{AppState, Stream, admin, request};
use crate::engine::{Incarnation, RequestInfo};
use crate::render::Artifact;
use crate::tarpit::{self, Exchange, Outcome};
use crate::util::Tm;
use crate::wire::{Persist, RequestHead};

pub async fn handle(
    app: &AppState,
    inc: &Incarnation,
    peer: SocketAddr,
    head: &RequestHead,
    persist: Persist,
    sock: &mut impl Stream,
) -> Outcome {
    let (path, query) = match head.path_and_query().split_once('?') {
        Some((p, q)) => (p.to_string(), Some(q.to_string())),
        None => (head.path_and_query().to_string(), None),
    };
    let headers = &head.headers;
    let ua = headers.get(http::header::USER_AGENT).and_then(|v| v.to_str().ok());
    let ip = request::client_ip(headers, peer);
    let host = request::host(headers, peer);
    let https = request::is_https(headers, peer);
    let head_only = head.method == Method::HEAD;
    let wire = &inc.persona.data.server(inc.persona.frontend).wire;
    let ex = Exchange { wire, persist, http10: head.minor == 0, head_only, date: Tm::now().http(), proxied: request::from_proxy(peer) };

    // Admin stats; a wrong or missing token is just another probe.
    if path == app.cfg.stats.admin_path && admin::authorized(&app.admin_token, headers, query.as_deref()) {
        let art = plain(StatusCode::OK, "application/json; charset=utf-8", admin::stats_json(&app.stats).into_bytes());
        return tarpit::instant(sock, &ex, art).await;
    }

    // Opt-in allowlist: spare these clients with a fast clean 404.
    if app.allow.contains(ip, ua) {
        return tarpit::instant(sock, &ex, plain(StatusCode::NOT_FOUND, "text/html", Vec::new())).await;
    }

    let info = RequestInfo { method: &head.method, path: &path, query: query.as_deref(), host: host.as_deref(), https };
    // robots.txt is bait that must be read, so it is served fast.
    let art = if path == "/robots.txt" && (head.method == Method::GET || head_only) {
        app.engine.robots(inc, &info)
    } else {
        app.engine.render(inc, inc.router.profile_for(&path), &info)
    };

    let guard = app.stats.open(ip);
    // Protect our own box: past the concurrency cap, answer fast instead of
    // holding yet another slow slot open.
    if app.stats.active() as usize > app.cfg.limits.max_concurrent {
        return tarpit::instant(sock, &ex, art).await;
    }
    let plan = tarpit::Plan::sample(&app.cfg.tarpit, &inc.persona.timing, &art, &mut rand::make_rng::<rand_chacha::ChaCha8Rng>());
    // Part of the wait happens before the status line, like a slow backend.
    tokio::time::sleep(plan.pre_headers).await;
    // The date is the moment the head goes out.
    let ex = Exchange { date: Tm::now().http(), ..ex };
    let src = tarpit::Source { cfg: &app.cfg.tarpit, engine: &app.engine, inc };
    tarpit::respond(sock, &ex, &src, art, plan, &guard).await
}

/// A response of our own (stats, allowlisted clients): no persona headers.
fn plain(status: StatusCode, content_type: &str, body: Vec<u8>) -> Artifact {
    Artifact {
        status,
        headers: vec![("content-type".into(), content_type.into()), ("cache-control".into(), "no-store".into())],
        head: body,
        stream: None,
        tail: Vec::new(),
        fixed_length: true,
        fast: true,
    }
}
