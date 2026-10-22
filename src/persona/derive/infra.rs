//! Where and how the app runs: cloud, network, host, database, cache, mail.

use std::collections::BTreeMap;

use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::identity::Identity;
use super::tech::Tech;
use crate::data::{Data, Timezone};
use crate::persona::capitalize;
use crate::persona::fill::{Vars, fill};
use crate::persona::model::{Cache, Cloud, Database, DbEngine, Mail, Stack};
use crate::util::{chance, pick, pick_str, weighted_key, weighted_str};

pub struct Infra {
    pub tz: &'static Timezone,
    pub cloud: Cloud,
    pub region: &'static str,
    pub net: (u8, u8, u8),
    pub hostname: String,
    pub tz_offset: i32,
    pub tz_name: &'static str,
    pub db: Database,
    pub cache: Option<Cache>,
    pub mail: Mail,
    pub app_name: String,
    pub app_env: &'static str,
}

pub fn derive(rng: &mut ChaCha8Rng, data: &'static Data, id: &Identity, tech: &Tech) -> Infra {
    let infra = &data.infra;
    let facts = data.stack(tech.stack);
    let short = id.short().replace('-', "_");

    let mut weights: BTreeMap<Cloud, u32> = infra.clouds.iter().map(|(c, f)| (*c, f.weight)).collect();
    weights.extend(facts.clouds.iter().map(|(c, w)| (*c, *w)));
    let cloud = weighted_key(rng, &weights);
    let region = pick_str(rng, &infra.clouds[&cloud].regions);

    let net = match rng.random_range(0..10) {
        0..=5 => (10, rng.random_range(0..64), rng.random_range(0..16)),
        6..=7 => (172, rng.random_range(16..32), rng.random_range(0..16)),
        _ => (192, 168, rng.random_range(0..16)),
    };
    let internal_ip = |rng: &mut ChaCha8Rng| format!("{}.{}.{}.{}", net.0, net.1, net.2, rng.random_range(2..250));

    let hostname = if cloud == Cloud::Aws && chance(rng, 0.3) {
        format!("ip-{}-{}-{}-{}", net.0, net.1, net.2, rng.random_range(4..250))
    } else {
        let pattern = pick_str(rng, &infra.hostnames);
        fill(pattern, &Vars::default().with("short", id.short()), rng)
    };

    let tz = &infra.timezones[rng.random_range(0..infra.timezones.len())];

    let db_vars = |rng: &mut ChaCha8Rng| {
        let ip = internal_ip(rng);
        let host_word = pick_str(rng, &data.words.internal_host_words).to_string();
        let alias = pick_str(rng, &infra.databases[&tech.engine].aliases).to_string();
        Vars::default().with("short", short.clone()).with("internal_ip", ip).with("host_word", host_word).with("alias", alias)
    };
    let vars = db_vars(rng);
    let name_pattern = pick_str(rng, facts.db_names.as_deref().unwrap_or(&infra.db_names));
    let user_pattern = pick_str(rng, facts.db_users.as_deref().unwrap_or(&infra.db_users));
    let host_pattern = if tech.engine == DbEngine::MsSql && chance(rng, 0.4) { "localhost" } else { pick_str(rng, &infra.db_hosts) };
    let prefix = if facts.table_prefixes.is_empty() { "" } else { weighted_str(rng, &facts.table_prefixes) };
    let db = Database {
        engine: tech.engine,
        name: fill(name_pattern, &vars, rng),
        user: fill(user_pattern, &vars, rng).chars().take(16).collect(),
        host: fill(host_pattern, &vars, rng),
        port: pick(rng, &infra.databases[&tech.engine].ports),
        table_prefix: fill(prefix, &vars, rng),
        extended_insert: chance(rng, 0.8),
    };

    let cache = (tech.stack != Stack::WordPress || chance(rng, 0.3)).then(|| {
        let redis = !chance(rng, 0.15);
        let vars = db_vars(rng);
        Cache {
            redis,
            host: fill(pick_str(rng, &infra.cache_hosts), &vars, rng),
            port: if redis { 6379 } else { 11211 },
            has_password: chance(rng, 0.6),
        }
    });

    let provider = &infra.mail_providers[rng.random_range(0..infra.mail_providers.len())];
    let mail = Mail {
        provider: provider.id.as_str(),
        host: provider.host.as_str(),
        port: pick(rng, &infra.mail_ports),
        from: format!("{}@{}", pick_str(rng, &infra.mail_senders), id.domain),
    };

    let app_name = if facts.cms {
        id.company.clone()
    } else {
        match rng.random_range(0..4) {
            0 => id.company.clone(),
            1 => id.short().to_string(),
            2 => format!("{}-{}", id.short(), pick_str(rng, &infra.app_name_suffixes)),
            _ => capitalize(id.short()),
        }
    };

    Infra {
        tz,
        cloud,
        region,
        net,
        hostname,
        tz_offset: tz.offset,
        tz_name: tz.name.as_str(),
        db,
        cache,
        mail,
        app_name,
        app_env: pick_str(rng, &infra.app_envs),
    }
}
