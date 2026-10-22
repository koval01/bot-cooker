//! Response headers as the persona's layers would emit them: the front
//! server's banner and static-file metadata (ETag/Last-Modified in that
//! server's own format), the framework's headers and cookies, the operator's
//! hardening headers. What each stack and server sends is data
//! (`stacks.yaml`, `servers.yaml`); the order of application headers is
//! instance-specific.

use minijinja::context;
use rand::RngExt;

use super::doc::{Doc, Origin};
use super::objects::Fresh;
use super::scope::Scope;
use crate::persona::Frontend;
use crate::util::{self, Tm};
use crate::world::content::file_size;

pub fn build(scope: &Scope, doc: &Doc) -> Vec<(String, String)> {
    let p = scope.persona();
    let data = scope.world.data();
    let server = data.server(p.frontend);
    let mut out: Vec<(String, String)> = Vec::new();
    if let Some(b) = &p.web.server_banner {
        out.push(("server".into(), b.clone()));
    }

    let content_type = match &doc.origin {
        Origin::Static { ext, .. } => server.mime.get(ext).or(server.mime_default.as_ref()).cloned(),
        _ => doc.content_type.clone(),
    };
    out.extend(content_type.map(|ct| ("content-type".to_string(), ct)));

    let body_len = doc.head.len() + doc.tail.len();
    match &doc.origin {
        Origin::Static { mtime, .. } => {
            // Streamed files still claim their full on-disk size.
            let size = if doc.stream.is_some() { file_size(&scope.world, &scope.req.path) } else { body_len as u64 };
            let salt = u64::from_le_bytes(p.mac(&[b"etag", scope.req.path.as_bytes()])[..8].try_into().unwrap_or([0; 8])) >> 4;
            let etag = scope.catalog.render_str(&server.etag, context! { mtime => *mtime, size => size, salt => salt }).unwrap_or_default();
            for name in &server.static_headers {
                let value = match name.as_str() {
                    "last-modified" => Tm::at(*mtime, 0).http(),
                    "etag" => etag.clone(),
                    "accept-ranges" => "bytes".into(),
                    _ => continue,
                };
                out.push((name.clone(), value));
            }
        }
        Origin::App { session } => out.extend(app_headers(scope, *session && doc.status.is_success(), body_len)),
        Origin::Server => {}
    }

    out.extend(doc.extra_headers.iter().cloned());

    // `add_header` (nginx) only applies to 2xx/3xx; Apache's and IIS's
    // configured headers go on everything.
    let applies = doc.status.as_u16() < 400 || matches!(p.frontend, Frontend::Apache | Frontend::Iis);
    if applies {
        for (k, v) in &p.web.security_headers {
            if !out.iter().any(|(n, _)| n == k) {
                out.push((k.to_string(), v.clone()));
            }
        }
    }
    out
}

fn app_headers(scope: &Scope, session: bool, body_len: usize) -> Vec<(String, String)> {
    let p = scope.persona();
    let facts = scope.world.data().stack(p.stack);
    let ctx = context! { rand => Fresh::value(), body_len => body_len, ..scope.base() };
    let render = |tpl: &str| scope.catalog.render_str(tpl, ctx.clone()).unwrap_or_default();

    let mut h: Vec<(String, String)> = Vec::new();
    if let Some(pb) = &p.web.powered_by {
        h.push(("x-powered-by".into(), pb.clone()));
    }
    for (name, tpl) in &facts.app_headers {
        h.push((name.clone(), render(tpl)));
    }
    if session {
        for (name, tpl) in &facts.session_headers {
            h.push((name.clone(), render(tpl)));
        }
    }
    if let Some(name) = p.web.request_id_header {
        let mut r = rand::rng();
        let id = match facts.request_id_format.as_deref() {
            Some("uuid") => util::uuid(&mut r),
            _ => {
                let len = r.random_range(12..17);
                util::hex(&mut r, len)
            }
        };
        h.push((name.into(), id));
    }
    h.retain(|(_, v)| !v.is_empty());
    // Header order is part of a fingerprint: rank names per instance.
    h.sort_by_key(|(n, _)| rank(scope, n));
    if session {
        h.extend(facts.cookies.iter().map(|tpl| ("set-cookie".to_string(), render(tpl))).filter(|(_, v)| !v.is_empty()));
    }
    h
}

fn rank(scope: &Scope, name: &str) -> u64 {
    let p = scope.persona();
    let mac = p.mac(&[b"hdr-order", &p.web.order_salt.to_le_bytes(), name.as_bytes()]);
    u64::from_le_bytes(mac[..8].try_into().unwrap_or([0; 8]))
}
