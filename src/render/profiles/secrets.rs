//! Credential files: `.htpasswd`, AWS profile, SSH private key.

use minijinja::context;
use rand::RngExt;

use super::config::shared;
use crate::render::doc::Doc;
use crate::render::scope::{Scope, ser};
use crate::render::static_origin;
use crate::util::{self, BCRYPT};

pub fn render(scope: &Scope) -> Doc {
    let p = scope.persona();
    let path = scope.req.path.to_ascii_lowercase();
    let (name, extra) = if path.contains("htpasswd") {
        let mut r = scope.world.cfg_rng("htpasswd", "");
        let bcrypt = scope.world.fixed_rng("htpasswd-style").random_bool(0.4);
        let n = r.random_range(1..=p.staff.len().clamp(1, 4));
        let users: Vec<_> = p
            .staff
            .iter()
            .take(n)
            .map(|s| {
                let hash = if bcrypt {
                    format!("$2y$05${}", util::token(&mut r, BCRYPT, 53))
                } else {
                    format!("$apr1${}${}", util::token(&mut r, BCRYPT, 8), util::token(&mut r, BCRYPT, 22))
                };
                context! { login => s.login.clone(), hash => hash }
            })
            .collect();
        ("secrets/htpasswd.j2", context! { users => users })
    } else if path.contains("credentials") || path.contains(".aws") {
        ("secrets/aws_credentials.j2", context! { cfg => ser(&shared(scope)) })
    } else {
        ("secrets/id_rsa.j2", context! {})
    };
    Doc::new(static_origin(scope, "secret"), Some("text/plain"), scope.render_or_empty(name, extra))
}
