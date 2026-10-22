//! Methods other than GET/HEAD, answered the way the persona's server and
//! framework would (`unknown_method`, `static_options`, `options`, `post`
//! in the data files).

use http::{Method, StatusCode};

use super::doc::{Doc, Origin};
use super::profiles::errors::{app_error, server_error, server_handles_404};
use super::scope::Scope;
use crate::config::Profile;

pub enum Gate {
    /// Render the profile as for GET.
    Render,
    /// Answer with this document instead.
    Answer(Doc),
    /// The API refuses (rendered by the API profile).
    ApiUnauthorized,
}

fn is_static(profile: Profile) -> bool {
    matches!(
        profile,
        Profile::Env
            | Profile::Git
            | Profile::SqlDump
            | Profile::Log
            | Profile::Secrets
            | Profile::Joomla
            | Profile::Drupal
            | Profile::Asset
    )
}

pub fn gate(scope: &Scope, profile: Profile, method: &Method) -> Gate {
    let p = scope.persona();
    let data = scope.world.data();
    let server = data.server(p.frontend);
    let facts = data.stack(p.stack);
    if method == Method::GET || method == Method::HEAD {
        return Gate::Render;
    }
    let verb = method.as_str();
    let standard = matches!(verb, "POST" | "PUT" | "DELETE" | "PATCH" | "OPTIONS" | "TRACE" | "CONNECT");
    if !standard {
        // Unknown verbs never reach the app.
        let status = StatusCode::from_u16(server.unknown_method).unwrap_or(StatusCode::METHOD_NOT_ALLOWED);
        return Gate::Answer(server_error(scope, status, verb));
    }
    if method == Method::TRACE || method == Method::CONNECT {
        return Gate::Answer(server_error(scope, StatusCode::METHOD_NOT_ALLOWED, verb));
    }
    let static_target = is_static(profile) || profile == Profile::Forbidden || (profile == Profile::NotFound && server_handles_404(scope));

    if method == Method::OPTIONS {
        if static_target {
            return Gate::Answer(match server.static_options.as_str() {
                "405" => server_error(scope, StatusCode::METHOD_NOT_ALLOWED, verb),
                allow => Doc::new(Origin::Server, Some(&server.page_type), Vec::new()).header("allow", allow),
            });
        }
        return match facts.options.as_str() {
            "page" => Gate::Render,
            "404" => Gate::Answer(app_error(scope, StatusCode::NOT_FOUND, verb)),
            "405" => Gate::Answer(app_error(scope, StatusCode::METHOD_NOT_ALLOWED, verb)),
            allow => {
                let body =
                    scope.render(&scope.stack_template("options.txt.j2"), minijinja::context! { allow => allow }).unwrap_or_default();
                Gate::Answer(Doc::new(Origin::App { session: false }, facts.error_type.as_deref(), body).header("allow", allow))
            }
        };
    }

    // POST / PUT / DELETE / PATCH
    if static_target {
        let status = if profile == Profile::Forbidden { StatusCode::FORBIDDEN } else { StatusCode::METHOD_NOT_ALLOWED };
        return Gate::Answer(server_error(scope, status, verb));
    }
    match profile {
        Profile::Login | Profile::Blank if method == Method::POST => Gate::Render,
        Profile::Json => Gate::ApiUnauthorized,
        Profile::NotFound => Gate::Answer(app_error(scope, StatusCode::NOT_FOUND, verb)),
        _ => match facts.post.as_str() {
            "page" => Gate::Render,
            code => {
                let status = code.parse::<u16>().ok().and_then(|c| StatusCode::from_u16(c).ok()).unwrap_or(StatusCode::METHOD_NOT_ALLOWED);
                let mut doc = app_error(scope, status, verb);
                if status == StatusCode::METHOD_NOT_ALLOWED {
                    doc = doc.header("allow", "GET, HEAD");
                }
                Gate::Answer(doc)
            }
        },
    }
}
