//! A "leaky" JSON API in the stack's conventions: the envelope and the
//! special endpoints are templates (`api/`), records come from the world and
//! are named in the persona's convention. Lists stream item by item and end
//! with a closing tail, so a complete page always parses.

use http::StatusCode;
use minijinja::{Value, context};
use serde_json::{Map, Value as Json, json};

use crate::persona::{Envelope, Stack, convert_case, short_slug};
use crate::render::artifact::StreamKind;
use crate::render::doc::{Doc, Origin};
use crate::render::scope::{Scope, ser};
use crate::util::{self, Tm};
use crate::world::people;
use crate::world::records::{self, Value as Cell};

const PER_BLOCK: u64 = 4;
const MARKER: &str = "\u{1}items\u{1}";

fn app_json(body: String) -> Doc {
    Doc::new(Origin::App { session: false }, Some("application/json"), body)
}

pub fn render(scope: &Scope) -> Doc {
    let p = scope.persona();
    let path = scope.req.path.trim_end_matches('/').to_ascii_lowercase();
    let prefix = p.site.api_prefix.to_ascii_lowercase();
    let special = |name: &str| pretty(scope, &scope.render_or_empty(name, context! {}));

    if let Some(rest) = path.strip_prefix("/actuator") {
        let (name, ok) = match rest.trim_matches('/') {
            "" => ("api/actuator/index.json.j2", true),
            "health" => ("api/actuator/health.json.j2", true),
            "info" => ("api/actuator/info.json.j2", true),
            "env" => ("api/actuator/env.json.j2", true),
            _ => ("api/actuator/404.json.j2", false),
        };
        let doc = Doc::new(Origin::App { session: false }, Some("application/vnd.spring-boot.actuator.v3+json"), special(name));
        return if ok { doc } else { doc.status(StatusCode::NOT_FOUND) };
    }
    match path.as_str() {
        "/_ignition/health-check" => return app_json(special("api/ignition.json.j2")),
        "/health" => return app_json(special("api/health.json.j2")),
        "/graphql" => return app_json(special("api/graphql.json.j2")).status(StatusCode::BAD_REQUEST),
        "/wp-json" => return app_json(special("api/wp-index.json.j2")),
        _ if path.starts_with("/swagger") => {
            let mut paths = Map::new();
            for r in &p.site.api_resources {
                paths.insert(
                    format!("{}/{r}", p.site.api_prefix),
                    json!({"get": {"tags": [crate::persona::capitalize(r)], "responses": {"200": {"description": "OK"}}}}),
                );
            }
            let body = scope.render_or_empty("api/swagger.json.j2", context! { swagger_paths => ser(&paths) });
            return app_json(pretty(scope, &body));
        }
        _ => {}
    }
    if path.ends_with(".json") && !path.starts_with(&prefix) {
        // A JSON file in a content area: somebody's export.
        return list(scope, "users", 1, Envelope::BareArray);
    }

    let rest = path.strip_prefix(&prefix).unwrap_or(&path).trim_start_matches('/');
    let mut seg = rest.split('/');
    let resource = seg.next().unwrap_or("");
    let id = seg.next().and_then(|s| s.parse::<u64>().ok());
    let known = p.site.api_resources.iter().find(|r| r.eq_ignore_ascii_case(resource)).copied();
    match (known, id) {
        (Some(res), Some(id)) if id >= 1 && id <= count(scope, res) => {
            let it = item(scope, res, id);
            let doc = if p.site.envelope == Envelope::DataLinksMeta { json!({ "data": it }) } else { it };
            app_json(to_text(scope, &doc))
        }
        (Some(res), None) => {
            if res == "users" && p.stack != Stack::WordPress && rand::RngExt::random_bool(&mut scope.world.fixed_rng("auth-wall"), 0.35) {
                return unauthorized(scope);
            }
            list(scope, res, scope.req.page(), p.site.envelope)
        }
        (None, None) if resource.is_empty() => {
            let mut endpoints = Map::new();
            for r in &p.site.api_resources {
                endpoints.insert(r.to_string(), Json::String(scope.url(&format!("{}/{r}", p.site.api_prefix))));
            }
            app_json(pretty(scope, &scope.render_or_empty("api/index.json.j2", context! { endpoints => ser(&endpoints) })))
        }
        _ => not_found(scope),
    }
}

fn count(scope: &Scope, res: &str) -> u64 {
    match res {
        // WordPress lists only people with published posts: a few staff
        // authors, never the customer base.
        "users" if scope.persona().stack == Stack::WordPress => authors(scope),
        "users" => people::count(&scope.world),
        _ => records::count(&scope.world, res),
    }
}

fn authors(scope: &Scope) -> u64 {
    let staff = scope.persona().staff.len().max(1) as u64;
    rand::RngExt::random_range(&mut scope.world.fixed_rng("wp-authors"), 1..=staff.min(5))
}

fn ts(scope: &Scope, t: i64) -> Json {
    let tm = Tm::at(t, 0);
    let millis = (t % 997) as u32;
    Json::String(match scope.persona().stack.id() {
        "laravel" => format!("{}T{}.000000Z", tm.date(), tm.time()),
        "spring_boot" | "asp_net" | "wordpress" => format!("{}T{}", tm.date(), tm.time()),
        _ => format!("{}T{}.{millis:03}Z", tm.date(), tm.time()),
    })
}

fn cell(scope: &Scope, v: &Cell) -> Json {
    match v {
        Cell::Null => Json::Null,
        Cell::Int(i) => json!(i),
        // DRF renders decimals as strings.
        Cell::Num(n) if scope.persona().stack == Stack::Django => Json::String(n.clone()),
        Cell::Num(n) => n.parse::<f64>().map(|f| json!(f)).unwrap_or(Json::Null),
        Cell::Str(s) => Json::String(s.clone()),
        Cell::Bool(b) => Json::Bool(*b),
        Cell::Ts(t) => ts(scope, *t),
    }
}

/// One record as the API would serialize it.
fn item(scope: &Scope, res: &str, id: u64) -> Json {
    let p = scope.persona();
    let w = &scope.world;
    let name = |snake: &str| convert_case(snake, p.style.naming);
    let mut m = Map::new();
    if res == "users" {
        let u = people::user(w, id);
        if p.stack == Stack::WordPress {
            let avatar = util::hex(&mut util::keyed_rng(&p.mac(&[b"gravatar"]), &[&id.to_le_bytes()]), 16);
            let urls: Map<String, Json> = [24, 48, 96]
                .iter()
                .map(|s| (s.to_string(), Json::String(format!("https://secure.gravatar.com/avatar/{avatar}?s={s}&d=mm&r=g"))))
                .collect();
            return json!({"id": u.id, "name": u.name, "url": "", "description": "", "link": scope.url(&format!("/author/{}/", u.login)), "slug": u.login, "avatar_urls": urls, "meta": []});
        }
        m.insert(name("id"), json!(u.id));
        m.insert(name("name"), json!(u.name));
        m.insert(name("email"), json!(u.email));
        m.insert(name("role"), json!(u.role));
        m.insert(name("is_active"), json!(u.active));
        let mut leak = util::keyed_rng(&p.mac(&[b"api-leak"]), &[&id.to_le_bytes()]);
        if rand::RngExt::random_bool(&mut leak, 0.5) {
            m.insert(name("api_token"), json!(format!("{}_{}", short_slug(&p.slug), util::token(&mut leak, util::ALNUM, 32))));
        }
        m.insert(name("last_login_at"), u.last_login.map(|t| ts(scope, t)).unwrap_or(Json::Null));
        m.insert(name("created_at"), ts(scope, u.created));
        m.insert(name("updated_at"), ts(scope, u.updated));
        return Json::Object(m);
    }
    for c in records::record(w, res, id) {
        m.insert(name(c.name), cell(scope, &c.value));
    }
    Json::Object(m)
}

/// The persona's JSON formatting: compact, or pretty with its indent.
fn to_text(scope: &Scope, v: &Json) -> String {
    match scope.persona().style.json_pretty {
        None => v.to_string(),
        Some(n) => {
            let mut out = Vec::new();
            let indent = " ".repeat(n);
            let fmt = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
            let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
            serde::Serialize::serialize(v, &mut ser).ok();
            String::from_utf8(out).unwrap_or_default()
        }
    }
}

/// Re-format JSON produced by a template in the persona's style.
fn pretty(scope: &Scope, text: &str) -> String {
    serde_json::from_str::<Json>(text).map(|v| to_text(scope, &v)).unwrap_or_else(|_| text.to_string())
}

/// Indent a pretty-printed value so it sits at nesting `level`.
fn nest(text: &str, indent: usize, level: usize) -> String {
    let pad = " ".repeat(indent * level);
    text.lines().enumerate().map(|(i, l)| if i == 0 { l.to_string() } else { format!("{pad}{l}") }).collect::<Vec<_>>().join("\n")
}

fn envelope_ctx(scope: &Scope, res: &str, page: u64, per: u64, total: u64) -> Value {
    let pages = total.div_ceil(per).max(1);
    let first = (page - 1) * per + 1;
    let on_page = if first > total { 0 } else { (total - first + 1).min(per) };
    let base = scope.url(&scope.req.path);
    let link_base = base.clone();
    let naming = scope.persona().style.naming;
    context! {
        resource => res, page => page, per_page => per, total => total, pages => pages, first => first, on_page => on_page, base => base,
        link => Value::from_function(move |n: i64| if n >= 1 && n as u64 <= pages { Value::from(format!("{link_base}?page={n}")) } else { Value::from(()) }),
        field => Value::from_function(move |s: String| convert_case(&s, naming)),
    }
}

/// A page of a list, split around the items array for streaming.
fn list(scope: &Scope, res: &str, page: u64, env: Envelope) -> Doc {
    let p = scope.persona();
    let per = p.site.per_page as u64;
    let total = count(scope, res);
    let ctx = envelope_ctx(scope, res, page, per, total);
    let first = (page - 1) * per + 1;
    let on_page = if first > total { 0 } else { (total - first + 1).min(per) };
    let raw = scope
        .render_or_empty(&format!("api/envelopes/{}.json.j2", env.id()), ctx)
        .replace("\"MARKER\"", &serde_json::to_string(MARKER).unwrap_or_default());
    let text = pretty(scope, &raw);
    let marker = serde_json::to_string(MARKER).unwrap_or_default();
    let (before, after) = text.split_once(&marker).unwrap_or((&text, ""));
    if on_page == 0 {
        return app_json(format!("{before}[]{after}"));
    }
    let level = level_of(before, p.style.json_pretty);
    let close = match p.style.json_pretty {
        Some(n) => format!("\n{}]", " ".repeat(n * level)),
        None => "]".into(),
    };
    let mut doc = app_json(format!("{before}[")).streamed(StreamKind::Json, on_page.div_ceil(PER_BLOCK), format!("{close}{after}"));
    if p.stack == Stack::WordPress {
        doc = doc.header("x-wp-total", total.to_string()).header("x-wp-totalpages", total.div_ceil(per).max(1).to_string());
    }
    doc
}

/// Nesting level of the line the items array starts on.
fn level_of(before: &str, indent: Option<usize>) -> usize {
    match indent {
        Some(n) if n > 0 => before.rsplit('\n').next().map(|l| l.len() - l.trim_start().len()).unwrap_or(0) / n,
        _ => 0,
    }
}

pub fn block(scope: &Scope, seq: u64) -> Vec<u8> {
    let p = scope.persona();
    let path = scope.req.path.trim_end_matches('/').to_ascii_lowercase();
    let prefix = p.site.api_prefix.to_ascii_lowercase();
    let export = path.ends_with(".json") && !path.starts_with(&prefix);
    let rest = path.strip_prefix(&prefix).unwrap_or("").trim_start_matches('/');
    let res =
        p.site.api_resources.iter().find(|r| r.eq_ignore_ascii_case(rest.split('/').next().unwrap_or(""))).copied().unwrap_or("users");
    let (res, page) = if export { ("users", 1) } else { (res, scope.req.page()) };
    let per = p.site.per_page as u64;
    let total = count(scope, res);
    let first = (page - 1) * per + 1;
    let last = (first + per - 1).min(total);
    // Recover the nesting from the envelope, as `list` did.
    let env = if export { Envelope::BareArray } else { p.site.envelope };
    let raw = scope.render_or_empty(&format!("api/envelopes/{}.json.j2", env.id()), envelope_ctx(scope, res, page, per, total));
    let marker = serde_json::to_string(MARKER).unwrap_or_default();
    let text = pretty(scope, &raw.replace("\"MARKER\"", &marker));
    let level = level_of(text.split_once(&marker).map(|x| x.0).unwrap_or(""), p.style.json_pretty);
    let mut out = String::new();
    for i in 0..PER_BLOCK {
        let id = first + seq * PER_BLOCK + i;
        if id > last {
            break;
        }
        if id > first {
            out.push(',');
        }
        let it = to_text(scope, &item(scope, res, id));
        match p.style.json_pretty {
            Some(n) => {
                out += &format!("\n{}", " ".repeat(n * (level + 1)));
                out += &nest(&it, n, level + 1);
            }
            None => out += &it,
        }
    }
    out.into_bytes()
}

fn not_found(scope: &Scope) -> Doc {
    let body = scope.render_or_empty(&scope.stack_template("api-404.json.j2"), context! {});
    let ct = (!body.is_empty()).then_some("application/json");
    Doc::new(Origin::App { session: false }, ct, pretty(scope, &body)).status(StatusCode::NOT_FOUND)
}

pub fn unauthorized(scope: &Scope) -> Doc {
    let body = scope.render_or_empty(&scope.stack_template("api-401.json.j2"), context! {});
    let ct = (!body.is_empty()).then_some("application/json");
    let mut doc = Doc::new(Origin::App { session: false }, ct, pretty(scope, &body)).status(StatusCode::UNAUTHORIZED);
    match scope.persona().stack.id() {
        "django" => doc = doc.header("www-authenticate", "Token"),
        "spring_boot" | "asp_net" => doc = doc.header("www-authenticate", "Bearer"),
        _ => {}
    }
    doc
}
