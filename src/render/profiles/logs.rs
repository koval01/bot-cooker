//! Logs: a window of the world's traffic (`world::traffic`) written in the
//! format of whichever software would write that file.

use minijinja::context;

use crate::catalog::to_crlf;
use crate::persona::{AccessFormat, Frontend, LogKind};
use crate::render::artifact::StreamKind;
use crate::render::doc::Doc;
use crate::render::php;
use crate::render::scope::{Scope, ser};
use crate::render::static_origin;
use crate::world::traffic;

fn kind(scope: &Scope) -> LogKind {
    let p = scope.persona();
    p.site
        .logs
        .iter()
        .find(|(path, _)| path.eq_ignore_ascii_case(&scope.req.path))
        .map(|(_, k)| *k)
        .unwrap_or(if scope.req.path.contains("error") { LogKind::Error } else { LogKind::Access })
}

fn template(scope: &Scope, kind: LogKind) -> String {
    let p = scope.persona();
    match kind {
        LogKind::Access => format!("logs/access/{}.log.j2", p.web.access_format.id()),
        LogKind::Error if p.frontend == Frontend::Apache => "logs/error/apache.log.j2".into(),
        LogKind::Error => "logs/error/nginx.log.j2".into(),
        LogKind::App => {
            let own = format!("logs/app/{}.log.j2", p.stack.id());
            scope.pick(&[own.clone(), "logs/app/wordpress.log.j2".into()]).unwrap_or(own)
        }
    }
}

fn lines(scope: &Scope, kind: LogKind, seq: u64) -> String {
    let events = traffic::events(&scope.world, &scope.req.path, seq);
    let php = if kind == LogKind::App { php::logged(scope) } else { Vec::new() };
    let body = scope.render_or_empty(&template(scope, kind), context! { events => ser(&events), php => ser(&php) });
    if scope.persona().frontend == Frontend::Iis { to_crlf(&body) } else { body }
}

pub fn render(scope: &Scope) -> Doc {
    let kind = kind(scope);
    let tl = traffic::timeline(&scope.world, &scope.req.path);
    let mut head = String::new();
    if kind == LogKind::Access && scope.persona().web.access_format == AccessFormat::Iis {
        head += &to_crlf(&scope.render_or_empty("logs/access/iis_header.log.j2", context! { start => tl.start }));
    }
    head += &lines(scope, kind, 0);
    Doc::new(static_origin(scope, "log"), Some("text/plain"), head).streamed(StreamKind::Log(kind), tl.blocks.saturating_sub(1), Vec::new())
}

pub fn block(scope: &Scope, kind: LogKind, seq: u64) -> Vec<u8> {
    lines(scope, kind, seq + 1).into_bytes()
}
