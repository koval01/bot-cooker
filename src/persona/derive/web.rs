//! What the HTTP layer says about itself: banner, X-Powered-By, cookies,
//! request ids, hardening headers, dotfile policy, access log format.

use rand::RngExt;
use rand::seq::SliceRandom;
use rand_chacha::ChaCha8Rng;

use super::identity::Identity;
use super::tech::Tech;
use crate::data::Data;
use crate::persona::fill::{Vars, fill};
use crate::persona::model::{DotPolicy, Web};
use crate::util::{chance, pick_str, weighted_key, weighted_str};

pub fn derive(rng: &mut ChaCha8Rng, data: &'static Data, tech: &Tech, id: &Identity, dotfiles: DotPolicy) -> Web {
    let facts = data.stack(tech.stack);
    let server = data.server(tech.frontend);
    let infra = &data.infra;

    let banner_vars = Vars::default()
        .with("version", tech.versions.server.initial())
        .with("flavour", server.flavours.get(&tech.distro).cloned().unwrap_or_default());
    let server_banner = match &server.banner_hidden {
        Some(hidden) if chance(rng, server.hide_p) => hidden.clone(),
        _ => fill(&server.banner, &banner_vars, rng),
    };

    let powered_by = facts.powered_by.as_ref().and_then(|pb| {
        let applies = pb.frontend.is_none_or(|f| f == tech.frontend);
        (applies && chance(rng, pb.p)).then(|| fill(&pb.value, &Vars::default().with("runtime", tech.versions.runtime.initial()), rng))
    });

    let vars = Vars::default().with("short", id.short());
    let session_cookie = fill(weighted_str(rng, &facts.session_cookies), &vars, rng);
    let csrf_cookie = facts.csrf_cookie.as_ref().filter(|_| chance(rng, facts.csrf_cookie_p)).map(|c| fill(c, &vars, rng));

    let request_id_header = match facts.request_id.as_deref() {
        Some("none") => None,
        Some(name) => Some(name),
        None => Some(weighted_str(rng, &infra.request_ids)).filter(|n| *n != "none"),
    };

    let mut security_headers: Vec<(&'static str, String)> = Vec::new();
    for h in &infra.security_headers {
        if chance(rng, infra.security_header_p) {
            security_headers.push((h.name.as_str(), pick_str(rng, &h.values).to_string()));
        }
    }
    // Framework defaults win over whatever the operator may have added.
    for (name, value) in &facts.security_headers {
        security_headers.retain(|(n, _)| n != name);
        security_headers.push((name.as_str(), value.clone()));
    }
    security_headers.shuffle(rng);

    Web {
        server_banner: Some(server_banner),
        powered_by,
        session_cookie,
        csrf_cookie,
        request_id_header,
        security_headers,
        dotfiles,
        access_format: weighted_key(rng, &server.access_formats),
        order_salt: rng.random(),
    }
}
