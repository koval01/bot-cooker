//! Stack, front server, distro and software versions.

use rand_chacha::ChaCha8Rng;

use crate::data::Data;
use crate::persona::model::{DbEngine, Distro, Frontend, HostOs, Stack, Versions};
use crate::persona::version::Ver;
use crate::util::{pick_str, weighted, weighted_key};

pub struct Tech {
    pub stack: Stack,
    pub frontend: Frontend,
    pub distro: Distro,
    pub engine: DbEngine,
    pub versions: Versions,
}

/// Only servers that run on the host OS: a Windows banner over a Linux TCP
/// stack (or the reverse) is visible to any OS fingerprinting scan.
pub fn derive(rng: &mut ChaCha8Rng, data: &'static Data, host: HostOs) -> Tech {
    let runs_on_host = |f: &Frontend| data.server(*f).distros.keys().any(|d| d.host_os() == host);
    let by_weight: Vec<(Stack, u32)> =
        data.stacks.iter().filter(|(_, f)| f.frontends.keys().any(runs_on_host)).map(|(s, f)| (*s, f.weight)).collect();
    let stack = weighted(rng, &by_weight);
    let facts = data.stack(stack);
    let frontends: Vec<(Frontend, u32)> = facts.frontends.iter().filter(|(f, _)| runs_on_host(f)).map(|(f, w)| (*f, *w)).collect();
    let frontend = weighted(rng, &frontends);
    let server = data.server(frontend);
    let distros: Vec<(Distro, u32)> = server.distros.iter().filter(|(d, _)| d.host_os() == host).map(|(d, w)| (*d, *w)).collect();
    let distro = weighted(rng, &distros);
    let engine = weighted_key(rng, &facts.databases);

    let server_versions = server.versions.get(distro.id()).or_else(|| server.versions.get("default")).expect("validated");
    // What shows in headers (server banner, X-Powered-By) stays fixed for
    // the persona's era; the framework and the database get upgraded.
    let mut version = |list: &[String], drifts: bool| {
        let text = pick_str(rng, list);
        if drifts { Ver::parse(rng, text) } else { Ver::fixed(rng, text) }
    };
    let versions = Versions {
        server: version(server_versions, false),
        runtime: version(&data.infra.runtimes[&facts.family], false),
        framework: version(&facts.versions, true),
        db: version(&data.infra.databases[&engine].versions, true),
    };
    Tech { stack, frontend, distro, engine, versions }
}
