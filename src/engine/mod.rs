//! The engine: which persona is live, what time it is in content terms, and
//! the entry points that turn a routed request into an artifact.
//!
//! Content is a pure function of `(master_key, path, epoch)`. Within an epoch
//! it's byte-for-byte stable from any IP (a scanner re-fetching `/.env` sees
//! the same secrets); when the epoch rolls over it changes, as if the box were
//! maintained. On top sits the persona: one coherent imaginary organisation
//! per master key, re-derived only when the persona era changes.

use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use http::Method;
use rand::Rng;

use crate::catalog::Catalog;
use crate::config::{DeceptionConfig, Profile, RouteRule};
use crate::data::Data;
use crate::persona::{HostOs, Persona};
use crate::render::{self, Artifact, Continuation, DocCache, Req, Scope};
use crate::routing::Router;
use crate::util;
use crate::world::{Epochs, World};

pub struct Engine {
    master_key: [u8; 32],
    config_epoch_secs: u64,
    log_epoch_secs: u64,
    jitter_secs: u64,
    persona_era_secs: u64,
    host_os: HostOs,
    route_override: Option<Vec<RouteRule>>,
    catalog: Arc<Catalog>,
    data: &'static Data,
    current: RwLock<Arc<Incarnation>>,
    /// Frozen clock for tests.
    fixed_now: Option<u64>,
}

/// The persona of the current era plus its compiled route table.
pub struct Incarnation {
    pub era: u64,
    pub persona: Persona,
    pub router: Router,
    /// Expensive static bodies of this era.
    pub cache: DocCache,
}

/// What the handler knows about a request, already sanitised.
pub struct RequestInfo<'a> {
    pub method: &'a Method,
    pub path: &'a str,
    pub query: Option<&'a str>,
    /// Lowercased, validated `Host` (falls back to the persona domain).
    pub host: Option<&'a str>,
    pub https: bool,
}

impl Engine {
    pub fn new(cfg: &DeceptionConfig, catalog: Arc<Catalog>, data: &'static Data, route_override: Option<Vec<RouteRule>>) -> Self {
        Self::with_clock(cfg, catalog, data, route_override, None)
    }

    pub fn with_clock(
        cfg: &DeceptionConfig,
        catalog: Arc<Catalog>,
        data: &'static Data,
        route_override: Option<Vec<RouteRule>>,
        fixed_now: Option<u64>,
    ) -> Self {
        let master_key = derive_master_key(&cfg.master_key);
        let persona_era_secs = cfg.persona_era_days * 86_400;
        let era = era_at(now_or(fixed_now), persona_era_secs);
        let host_os = cfg.host_os.resolve();
        let current = Arc::new(incarnate(&master_key, era, host_os, data, route_override.as_deref()));
        Self {
            master_key,
            config_epoch_secs: cfg.config_epoch_hours.max(1) * 3600,
            log_epoch_secs: cfg.log_epoch_hours.max(1) * 3600,
            jitter_secs: cfg.epoch_jitter_hours * 3600,
            persona_era_secs,
            host_os,
            route_override,
            catalog,
            data,
            current: RwLock::new(current),
            fixed_now,
        }
    }

    fn now(&self) -> u64 {
        now_or(self.fixed_now)
    }

    /// The persona (and route table) of the current era.
    pub fn current(&self) -> Arc<Incarnation> {
        let era = era_at(self.now(), self.persona_era_secs);
        {
            let cur = self.current.read().unwrap_or_else(|e| e.into_inner());
            if cur.era == era {
                return Arc::clone(&cur);
            }
        }
        let fresh = Arc::new(incarnate(&self.master_key, era, self.host_os, self.data, self.route_override.as_deref()));
        let mut w = self.current.write().unwrap_or_else(|e| e.into_inner());
        *w = Arc::clone(&fresh);
        fresh
    }

    /// This instance's shift of epoch boundaries: stable per epoch window,
    /// different for every master key, so neither rotations nor the dates
    /// derived from them line up across instances.
    fn shift(&self, epoch_secs: u64, window: u64) -> u64 {
        // A floor of an hour even without configured jitter: anchors must
        // never sit on the same instant on every instance.
        let span = self.jitter_secs.max(3_600);
        let tag = util::hmac(&self.master_key, &[b"epoch-jitter", &epoch_secs.to_le_bytes(), &window.to_le_bytes()]);
        u64::from_le_bytes(tag[0..8].try_into().unwrap_or([0; 8])) % span
    }

    /// Current epoch id and the unix time it started for this instance
    /// (never in the future).
    fn epoch(&self, epoch_secs: u64) -> (u64, i64) {
        let now = self.now();
        let shifted = |id: u64| (id * epoch_secs + self.shift(epoch_secs, id)) as i64;
        let id = now / epoch_secs;
        // The boundary of window `id` lies a little after its wall-clock start.
        if shifted(id) <= now as i64 {
            (id, shifted(id))
        } else {
            let prev = id.saturating_sub(1);
            (prev, shifted(prev))
        }
    }

    pub fn epochs(&self) -> Epochs {
        let (cfg, cfg_anchor) = self.epoch(self.config_epoch_secs);
        let (log, log_anchor) = self.epoch(self.log_epoch_secs);
        Epochs { cfg, log, cfg_anchor, log_anchor, now: self.now() as i64 }
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    fn scope<'a>(&'a self, persona: &'a Persona, req: &RequestInfo) -> Scope<'a> {
        let req = Req {
            method: req.method.as_str().to_string(),
            host: req.host.map(str::to_string).unwrap_or_else(|| persona.domain.clone()),
            scheme: if req.https { "https" } else { "http" },
            path: req.path.to_string(),
            query: req.query.map(str::to_string),
        };
        Scope::new(World::new(persona, self.epochs()), req, &self.catalog)
    }

    /// Render the response for a request routed to `profile`.
    pub fn render(&self, inc: &Incarnation, profile: Profile, req: &RequestInfo) -> Artifact {
        render::render(&self.scope(&inc.persona, req), profile, req.method, &inc.cache)
    }

    /// The front server's own answer (a malformed request, a missing Host),
    /// rendered before any application sees the request.
    pub fn server_error(&self, inc: &Incarnation, status: http::StatusCode, req: &RequestInfo) -> Artifact {
        let scope = self.scope(&inc.persona, req);
        let doc = render::profiles::errors::server_error(&scope, status, req.method.as_str());
        render::finish(&scope, doc, true)
    }

    /// The robots.txt lure, rendered for the current persona.
    pub fn robots(&self, inc: &Incarnation, req: &RequestInfo) -> Artifact {
        render::robots(&self.scope(&inc.persona, req))
    }

    /// Produce block `seq` of a streamed artifact, in the epochs it started in.
    pub fn continue_stream(&self, persona: &Persona, c: &Continuation, seq: u64) -> Vec<u8> {
        let scope = Scope::new(World::new(persona, c.epochs), c.req.clone(), &self.catalog);
        render::block(&scope, c.kind, seq)
    }
}

fn incarnate(master_key: &[u8; 32], era: u64, host_os: HostOs, data: &'static Data, route_override: Option<&[RouteRule]>) -> Incarnation {
    let persona = Persona::derive(master_key, era, host_os, data);
    let routes = match route_override {
        Some(r) => r.to_vec(),
        None => persona.routes(),
    };
    Incarnation { era, router: Router::new(&routes), persona, cache: DocCache::default() }
}

fn era_at(now: u64, era_secs: u64) -> u64 {
    // `0` means the persona never changes.
    now.checked_div(era_secs).unwrap_or(0)
}

fn now_or(fixed: Option<u64>) -> u64 {
    fixed.unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
}

/// Derive a stable 32-byte master key. If the configured key is empty,
/// generate a random one (artifacts then reset each restart — fine for tests).
fn derive_master_key(configured: &str) -> [u8; 32] {
    if configured.is_empty() {
        let mut key = [0u8; 32];
        rand::rng().fill_bytes(&mut key);
        tracing::warn!(
            "no master_key set — persona and fake artifacts will reset on restart; \
             set deception.master_key (or COOKER_MASTER_KEY) in production"
        );
        return key;
    }
    util::hmac(b"bot-cooker-master-v1", &[configured.as_bytes()])
}

#[cfg(test)]
mod audit;
#[cfg(test)]
mod tests;
