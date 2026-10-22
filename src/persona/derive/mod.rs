//! Deriving a persona from a master key. Each submodule owns one facet and
//! draws from its own keyed RNG stream, so adding a draw in one facet never
//! reshuffles the others.

mod assets;
mod identity;

pub use identity::name;
mod infra;
mod php;
mod site;
mod style;
mod tech;
mod web;

use std::sync::Arc;

use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::model::{DotPolicy, HostOs, Persona};
use crate::data::Data;
use crate::text::{self, TextModel};
use crate::util::{self, chance};

/// Derive the persona for `master_key` during persona era `era`, on `host`.
pub fn derive(master_key: &[u8; 32], era: u64, host: HostOs, data: &'static Data) -> Persona {
    let seed = util::hmac(master_key, &[b"persona", &era.to_le_bytes()]);
    let stream = |facet: &str| -> ChaCha8Rng { util::keyed_rng(&seed, &[b"facet", facet.as_bytes()]) };

    let id = identity::derive(&mut stream("identity"), data);
    let tech = tech::derive(&mut stream("tech"), data, host);
    let infra = infra::derive(&mut stream("infra"), data, &id, &tech);
    let name_mix = identity::name_mix(&mut stream("names"), data, infra.tz);
    let staff = identity::staff(&mut stream("staff"), data, &name_mix, &id.domain);
    let text = text_model(&mut stream("text"), data);
    let mut voice_rng = stream("voice");
    let voice = Arc::new(text::Voice::new(&data.synonyms.groups, &mut voice_rng).with_markers(&data.synonyms.markers, &mut voice_rng));
    let style = style::style(&mut stream("style"), data.stack(tech.stack));
    let secrets = style::secrets(&mut stream("secrets"));
    let timing = style::timing(&mut stream("timing"));
    let dotfiles = if chance(&mut stream("dotfiles"), 0.2) { DotPolicy::Deny } else { DotPolicy::Serve };
    let web = web::derive(&mut stream("web"), data, &tech, &id, dotfiles);
    let site = site::derive(&mut stream("site"), data, &tech, &id, &infra.db, dotfiles);
    let assets = assets::derive(&mut stream("assets"), data, &tech);
    let php_errors = php::derive(&mut stream("php_errors"), data, &tech);

    Persona {
        seed,
        data,
        company: id.company,
        slug: id.slug,
        domain: id.domain,
        industry: id.industry,
        stack: tech.stack,
        stack_name: data.stack(tech.stack).name.as_str(),
        family: data.stack(tech.stack).family,
        cms: data.stack(tech.stack).cms,
        frontend: tech.frontend,
        distro: tech.distro,
        versions: tech.versions,
        db: infra.db,
        cache: infra.cache,
        mail: infra.mail,
        cloud: infra.cloud,
        region: infra.region,
        net: infra.net,
        hostname: infra.hostname,
        app_name: infra.app_name,
        app_env: infra.app_env,
        tz_offset: infra.tz_offset,
        tz_name: infra.tz_name,
        staff,
        name_mix,
        text,
        voice,
        style,
        secrets,
        timing,
        web,
        site,
        assets,
        php_errors,
    }
}

/// The persona's prose model: trained on roughly two thirds of the corpus,
/// a different two thirds for every persona.
fn text_model(rng: &mut ChaCha8Rng, data: &'static Data) -> Arc<TextModel> {
    let keep = rng.random_range(0.45..0.75);
    let sentences: Vec<&str> = data.corpus.values().flat_map(|t| text::sentences(t)).filter(|_| rng.random_bool(keep)).collect();
    Arc::new(TextModel::train(sentences))
}
