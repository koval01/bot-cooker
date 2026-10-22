//! Every profile, for many personas and methods, renders without template
//! errors (strict undefined catches any context mismatch).

use std::sync::OnceLock;

use http::{Method, StatusCode};

use super::profiles::{api, config, errors, logs, pages, site, sql};
use super::*;
use crate::catalog::Catalog;
use crate::config::Profile;
use crate::data::Data;
use crate::persona::Persona;
use crate::world::{Epochs, World};

const NOW: i64 = 1_792_587_909;

fn catalog() -> &'static Catalog {
    static C: OnceLock<Catalog> = OnceLock::new();
    C.get_or_init(|| Catalog::load(None).expect("catalog"))
}

fn persona(n: u32) -> Persona {
    Persona::derive(&crate::util::hmac(b"render", &[&n.to_le_bytes()]), 0, crate::persona::HostOs::Unix, Data::embedded())
}

fn epochs() -> Epochs {
    Epochs { cfg: 2_964, log: 62_249, cfg_anchor: NOW - 3 * 86_400, log_anchor: NOW - 3_600, now: NOW }
}

fn scope<'a>(p: &'a Persona, method: &str, path: &str) -> Scope<'a> {
    let (path, query) = path.split_once('?').map(|(p, q)| (p, Some(q.to_string()))).unwrap_or((path, None));
    let req = Req { method: method.into(), host: "trap.example.net".into(), scheme: "https", path: path.into(), query };
    Scope::new(World::new(p, epochs()), req, catalog())
}

/// Render through a strict path: template errors fail the test instead of
/// degrading to an empty body.
fn strict(scope: &Scope, name: &str) {
    scope.render(name, minijinja::context! {}).unwrap_or_else(|e| panic!("{name}: {e:#}"));
}

#[test]
fn server_pages_render_for_every_frontend() {
    for i in 0..60 {
        let p = persona(i);
        let s = scope(&p, "GET", "/missing");
        for status in [StatusCode::NOT_FOUND, StatusCode::FORBIDDEN, StatusCode::METHOD_NOT_ALLOWED, StatusCode::NOT_IMPLEMENTED] {
            let d = errors::server_error(&s, status, "FOO");
            assert_eq!(d.status, status);
        }
        let r = errors::server_redirect(&s, "https://trap.example.net/files/");
        assert_eq!(r.status, StatusCode::MOVED_PERMANENTLY);
        assert!(r.extra_headers.iter().any(|(k, v)| k == "location" && v.ends_with("/files/")));
    }
}

#[test]
fn stack_pages_render_strictly() {
    for i in 0..80 {
        let p = persona(i);
        let s = scope(&p, "GET", &p.site.login_path.clone());
        let ctx = |status: u16| minijinja::context! { status => status, reason => "Not Found", method => "GET", failed => true, action => "/login", location => "/x", allow => "GET", post => false };
        for file in ["error.html.j2", "login.html.j2", "redirect.html.j2", "options.txt.j2"] {
            let name = s.stack_template(file);
            s.render(&name, ctx(404)).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        }
        strict(&s, "pages/phpinfo.html.j2");
    }
}

#[test]
fn profiles_produce_documents() {
    for i in 0..60 {
        let p = persona(i);
        let login = p.site.login_path.clone();
        for (method, path) in [("GET", login.as_str()), ("POST", login.as_str()), ("GET", "/"), ("GET", "/nope"), ("GET", "/x.php")] {
            let s = scope(&p, method, path);
            let doc = match path {
                "/" => site::render(&s),
                "/nope" | "/x.php" => errors::not_found(&s),
                _ => pages::login(&s, method == "POST"),
            };
            if doc.status == StatusCode::OK {
                assert!(!doc.head.is_empty(), "{} {path}: empty", p.stack_name);
            }
            let art = finish(&s, doc, false);
            assert!(art.headers.iter().all(|(k, _)| k == &k.to_ascii_lowercase()));
        }
        let s = scope(&p, "GET", "/server-status");
        if p.site.server_status {
            assert!(!pages::server_status(&s).head.is_empty());
        }
        assert!(!pages::joomla_manifest(&s).head.is_empty());
        assert!(!pages::drupal_changelog(&s).head.is_empty());
        for path in ["/xmlrpc.php", "/up", "/health", "/wp-config.php"] {
            let s = scope(&p, "POST", path);
            let _ = pages::blank(&s, true);
            let _ = pages::blank(&s, false);
        }
        for dir in p.leak_dirs() {
            let s = scope(&p, "GET", &dir);
            assert_eq!(site::render(&s).status, StatusCode::OK);
        }
        let s = scope(&p, "GET", "/robots.txt");
        let robots = String::from_utf8(pages::robots(&s).head).unwrap();
        assert!(robots.contains("User-agent: *"));
        for area in &p.site.areas {
            let s = scope(&p, "GET", &area.prefix);
            assert_eq!(site::render(&s).status, StatusCode::OK);
        }
    }
}

#[test]
fn methods_follow_the_data() {
    for i in 0..60 {
        let p = persona(i);
        let s = scope(&p, "FOO", "/");
        let server = p.data.server(p.frontend);
        match gate(&s, Profile::Labyrinth, &Method::from_bytes(b"FOO").unwrap()) {
            Gate::Answer(d) => assert_eq!(d.status.as_u16(), server.unknown_method),
            _ => panic!("unknown verbs never reach the app"),
        }
        let s = scope(&p, "OPTIONS", "/.env");
        match gate(&s, Profile::Env, &Method::OPTIONS) {
            Gate::Answer(d) if server.static_options == "405" => assert_eq!(d.status, StatusCode::METHOD_NOT_ALLOWED),
            Gate::Answer(d) => assert!(d.extra_headers.iter().any(|(k, _)| k == "allow")),
            _ => panic!("static OPTIONS is answered by the server"),
        }
        assert!(matches!(gate(&s, Profile::Env, &Method::GET), Gate::Render));
    }
}

#[test]
fn static_headers_follow_the_server() {
    for i in 0..40 {
        let p = persona(i);
        let s = scope(&p, "GET", "/robots.txt");
        let art = finish(&s, pages::robots(&s), true);
        let names: Vec<&str> = art.headers.iter().map(|(k, _)| k.as_str()).collect();
        assert!(names.contains(&"etag") && names.contains(&"last-modified"), "{names:?}");
        let etag = &art.headers.iter().find(|(k, _)| k == "etag").unwrap().1;
        assert!(etag.starts_with('"') && etag.len() > 4, "{etag}");
    }
}

#[test]
fn configs_render_for_every_leak() {
    for i in 0..80 {
        let p = persona(i);
        let mut paths: Vec<String> = p.site.env_paths.iter().chain(&p.site.config_leaks).cloned().collect();
        paths.push("/docker-compose.yml".into());
        paths.push("/files/backup.env.bak".into());
        for path in paths {
            let s = scope(&p, "GET", &path);
            let body = String::from_utf8(config::render(&s).head).unwrap();
            assert!(!body.trim().is_empty(), "{} {path}: empty", p.stack_name);
            assert!(!body.contains("{{ ") && !body.contains("{% "), "{path}: unrendered template\n{body}");
        }
    }
}

#[test]
fn dumps_are_complete_in_every_dialect() {
    let mut dialects = std::collections::HashSet::new();
    for i in 0..80 {
        let p = persona(i);
        let s = scope(&p, "GET", "/backup/db.sql");
        let doc = sql::render(&s);
        let (_, blocks) = doc.stream.expect("dumps stream their users");
        let mut body = String::from_utf8(doc.head).unwrap();
        for seq in 0..blocks.min(3) {
            body += &String::from_utf8(sql::block(&s, seq)).unwrap();
        }
        body += &String::from_utf8(doc.tail).unwrap();
        assert!(body.matches("CREATE TABLE").count() >= 2, "{}: {body}", p.stack_name);
        assert!(body.contains(&format!("@{}", p.domain)), "no staff email");
        let end = body.trim_end();
        assert!(
            end.ends_with("--") || end.ends_with("GO") || end.contains("Dump completed"),
            "bad ending: {}",
            &end[end.len().saturating_sub(120)..]
        );
        dialects.insert(p.db.engine);
    }
    assert!(dialects.len() >= 3);
}

#[test]
fn logs_render_in_every_format() {
    let mut formats = std::collections::HashSet::new();
    for i in 0..80 {
        let p = persona(i);
        let mut paths: Vec<String> = p.site.logs.iter().map(|(path, _)| path.clone()).collect();
        paths.push("/access.log".into());
        paths.push("/error.log".into());
        for path in paths {
            let s = scope(&p, "GET", &path);
            let doc = logs::render(&s);
            let mut body = String::from_utf8(doc.head).unwrap();
            if let Some((crate::render::StreamKind::Log(k), blocks)) = doc.stream {
                for seq in 0..blocks.min(2) {
                    body += &String::from_utf8(logs::block(&s, k, seq)).unwrap();
                }
            }
            assert!(body.lines().count() > 10, "{path}: {body}");
            assert!(!body.contains("{{") && !body.lines().any(|l| l.trim().is_empty()), "{path}: blank or raw lines");
            formats.insert(format!("{}{}", p.web.access_format.id(), p.stack.id()));
        }
    }
    assert!(formats.len() > 10);
}

#[test]
fn api_pages_parse_when_complete() {
    let mut envelopes = std::collections::HashSet::new();
    for i in 0..80 {
        let p = persona(i);
        let mut paths: Vec<String> = p.site.api_resources.iter().map(|r| format!("{}/{r}?page=2", p.site.api_prefix)).collect();
        paths.push(format!("{}/users/1", p.site.api_prefix));
        paths.push(format!("{}/nope", p.site.api_prefix));
        paths.push(p.site.api_prefix.clone());
        paths.extend(
            ["/actuator", "/actuator/env", "/health", "/graphql", "/wp-json", "/swagger/v1/swagger.json", "/files/export.json"]
                .map(String::from),
        );
        for path in paths {
            let s = scope(&p, "GET", &path);
            let doc = api::render(&s);
            let mut body = String::from_utf8(doc.head).unwrap();
            if let Some((_, blocks)) = doc.stream {
                for seq in 0..blocks {
                    body += &String::from_utf8(api::block(&s, seq)).unwrap();
                }
            }
            body += &String::from_utf8(doc.tail).unwrap();
            if body.is_empty() {
                continue;
            }
            serde_json::from_str::<serde_json::Value>(&body).unwrap_or_else(|e| panic!("{} {path}: {e}\n{body}", p.stack_name));
        }
        envelopes.insert(p.site.envelope);
        let s = scope(&p, "POST", &p.site.api_prefix.clone());
        assert_eq!(api::unauthorized(&s).status, StatusCode::UNAUTHORIZED);
    }
    assert!(envelopes.len() >= 5);
}

// --- PHP error leakage -----------------------------------------------------

fn scope_at<'a>(p: &'a Persona, path: &str, e: Epochs) -> Scope<'a> {
    let req = Req { method: "GET".into(), host: "trap.example.net".into(), scheme: "https", path: path.into(), query: None };
    Scope::new(World::new(p, e), req, catalog())
}

fn body_of(s: &Scope, profile: Profile) -> String {
    String::from_utf8(render(s, profile, &Method::GET, &DocCache::default()).head).unwrap()
}

/// `(label, file, line)` of every PHP error a body shows.
fn leaks(body: &str) -> Vec<(String, String, u32)> {
    body.lines()
        .filter_map(|l| {
            let (label, rest) = l.strip_prefix("<b>")?.split_once("</b>:  ")?;
            let (_, at) = rest.rsplit_once(" in <b>")?;
            let (file, line) = at.split_once("</b> on line <b>")?;
            Some((label.to_string(), file.to_string(), line.strip_suffix("</b><br />")?.parse().ok()?))
        })
        .collect()
}

fn shows_errors(p: &Persona) -> bool {
    p.php_errors.as_ref().is_some_and(|e| e.display != crate::persona::PhpDisplay::Off)
}

#[test]
fn php_errors_show_only_where_the_persona_displays_them() {
    use crate::persona::Stack;
    let (mut displaying, mut visible, mut quiet) = (0, 0, 0);
    for i in 0..300 {
        let p = persona(i);
        if p.php_errors.is_some() {
            assert!(p.stack == Stack::WordPress || (p.stack == Stack::Joomla && p.versions.framework.major == 3), "{}", p.summary());
        }
        let mut found = Vec::new();
        for (path, profile) in
            [("/", Profile::Labyrinth), (p.site.login_path.as_str(), Profile::Login), ("/blog/a-page/", Profile::Labyrinth)]
        {
            let body = body_of(&scope(&p, "GET", path), profile);
            found.extend(leaks(&body));
            assert!(!body.contains("</b>:  ") || shows_errors(&p), "{} {path}: quiet persona leaks", p.summary());
        }
        // phpinfo is a script of its own: never anything in front of it.
        assert!(body_of(&scope(&p, "GET", "/info.php"), Profile::PhpInfo).starts_with("<!DOCTYPE"));
        if shows_errors(&p) {
            displaying += 1;
            visible += usize::from(!found.is_empty());
        } else {
            quiet += 1;
        }
    }
    assert!(displaying >= 5 && quiet > displaying * 4, "{displaying} displaying, {quiet} quiet");
    assert!(visible * 10 >= displaying * 7, "only {visible} of {displaying} displaying personas show anything");
}

#[test]
fn php_errors_name_files_under_the_docroot() {
    let mut checked = 0;
    for i in 0..300 {
        let p = persona(i);
        if !shows_errors(&p) {
            continue;
        }
        let s = scope(&p, "GET", "/");
        let body = body_of(&s, Profile::Labyrinth);
        for (label, file, line) in leaks(&body) {
            assert!(file.starts_with(&format!("{}/", s.docroot())), "{file} outside {}", s.docroot());
            assert!(["Warning", "Notice", "Deprecated"].contains(&label.as_str()), "{label}");
            assert!(line > 0);
            // PHP's own framing: a line break before every message.
            assert!(body.contains(&format!("<br />\n<b>{label}</b>:  ")));
            checked += 1;
        }
        let e = p.php_errors.as_ref().unwrap();
        for (label, ..) in leaks(&body) {
            let level = match label.as_str() {
                "Warning" => crate::persona::PhpLevel::Warning,
                "Notice" => crate::persona::PhpLevel::Notice,
                _ => crate::persona::PhpLevel::Deprecated,
            };
            assert!(e.levels.contains(&level), "{label} shown under {}", e.setting);
        }
    }
    assert!(checked > 5);
}

#[test]
fn php_wording_follows_the_php_release() {
    let (mut seven, mut eight) = (0, 0);
    for i in 0..400 {
        let p = persona(i);
        if p.php_errors.is_none() {
            continue;
        }
        let s = scope(&p, "GET", "/");
        let (major, minor) = (p.versions.runtime.major, p.versions.runtime.minor);
        for e in php::logged(&s) {
            let m = e.message.as_str();
            if major == 7 {
                seven += 1;
                for php8 in [
                    "Undefined array key",
                    "Undefined variable $",
                    "Attempt to read property",
                    "foreach() argument",
                    "Passing null",
                    "dynamic property",
                ] {
                    assert!(!m.contains(php8), "PHP {major}.{minor}: {m}");
                }
            } else {
                eight += 1;
                for php7 in ["Undefined index:", "Undefined variable: ", "Trying to get property", "Invalid argument supplied"] {
                    assert!(!m.contains(php7), "PHP {major}.{minor}: {m}");
                }
                assert!(!m.contains("Passing null") || minor >= 1, "{m}");
                assert!(!m.contains("dynamic property") || minor >= 2, "{m}");
                assert!(!m.contains("offset on value of type null") || minor < 3, "PHP 8.{minor}: {m}");
                assert!(!m.contains("offset on null") || minor >= 3, "PHP 8.{minor}: {m}");
            }
            assert!(!m.contains('{'), "unfilled slot: {m}");
        }
    }
    assert!(seven > 0 && eight > 0, "{seven} PHP 7 and {eight} PHP 8 messages");
}

#[test]
fn php_errors_are_stable_per_instance_and_differ_across() {
    let mut seen = std::collections::HashSet::new();
    let (mut leaking, mut drifted) = (0, 0);
    for i in 0..300 {
        let p = persona(i);
        if !shows_errors(&p) {
            continue;
        }
        let at = |e: Epochs, path: &str| leaks(&body_of(&scope_at(&p, path, e), Profile::Labyrinth));
        let now = at(epochs(), "/");
        if now.is_empty() {
            continue;
        }
        leaking += 1;
        // The same on every request of the epoch, whatever the page.
        assert_eq!(now, at(epochs(), "/"));
        assert_eq!(now, at(Epochs { log: epochs().log + 5, ..epochs() }, "/"));
        let top = |v: &[(String, String, u32)]| {
            v.iter()
                .filter(|(_, f, _)| !f.contains("contact-form-7") && !f.contains("/components/") && !f.contains("/modules/"))
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(top(&now), top(&at(epochs(), &p.site.login_path.clone())));
        assert!(seen.insert(format!("{now:?}")), "two instances leak the same: {now:?}");
        // Some of it changes between config epochs.
        drifted += usize::from((1..6).any(|k| at(Epochs { cfg: epochs().cfg + k, ..epochs() }, "/") != now));
    }
    assert!(leaking >= 5 && drifted * 2 >= leaking, "{drifted} of {leaking} drift");
}

#[test]
fn config_files_and_phpinfo_agree_with_php_errors() {
    use crate::persona::{PhpDisplay, Stack};
    let mut checked = 0;
    for i in 0..300 {
        let p = persona(i);
        let Some(e) = &p.php_errors else { continue };
        let s = scope(&p, "GET", "/info.php");
        let info = s.render("pages/phpinfo.html.j2", minijinja::context! {}).unwrap();
        let on = "<td class=\"e\">display_errors</td><td class=\"v\">On</td><td class=\"v\">On</td>";
        assert_eq!(info.contains(on), e.display == PhpDisplay::Ini, "{}: {}", p.summary(), e.setting);
        assert!(info.contains(&format!("memory_limit</td><td class=\"v\">{}M</td>", e.memory_limit)));
        let backup = if p.stack == Stack::WordPress { "/wp-config.php.bak" } else { "/configuration.php.bak" };
        let text = String::from_utf8(config::render(&scope(&p, "GET", backup)).head).unwrap();
        let line = |needle: &str| text.lines().find(|l| l.contains(needle)).unwrap_or_default().to_string();
        match p.stack {
            Stack::WordPress => {
                let debug = line("'WP_DEBUG'").contains("true") || line("\"WP_DEBUG\"").contains("true");
                assert_eq!(debug && e.display != PhpDisplay::App, text.contains("WP_DEBUG_DISPLAY"), "{}", e.setting);
                assert_eq!(e.setting == "wp_debug", debug && e.display == PhpDisplay::App);
                assert!(e.setting != "ini" || !debug);
            }
            _ => assert!(line("error_reporting").contains(e.setting), "{}: {}", e.setting, line("error_reporting")),
        }
        checked += 1;
    }
    assert!(checked > 20);
}

#[test]
fn joomla_3_pages_can_die_of_memory_exhaustion() {
    let mut found = 0;
    for i in 0..500 {
        let p = persona(i);
        let Some(e) = &p.php_errors else { continue };
        if !shows_errors(&p) || e.fatal.is_none() {
            continue;
        }
        let limit = (u64::from(e.memory_limit) << 20).to_string();
        let dead = (0..200)
            .map(|n| body_of(&scope(&p, "GET", &format!("/articles/page-{n}/")), Profile::Labyrinth))
            .filter(|b| b.contains("<b>Fatal error</b>"))
            .collect::<Vec<_>>();
        assert!(!dead.is_empty() && dead.len() < 60, "{} of 200 pages die", dead.len());
        for body in dead {
            // Everything buffered before the fatal error is discarded.
            assert!(body.starts_with(&format!("<br />\n<b>Fatal error</b>:  Allowed memory size of {limit} bytes exhausted")), "{body}");
            assert_eq!(leaks(&body).len(), 1);
        }
        assert!(!body_of(&scope(&p, "GET", "/"), Profile::Labyrinth).contains("Fatal error"));
        found += 1;
    }
    assert!(found >= 1);
}
