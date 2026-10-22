//! The site layout: content areas, what leaks, where logs and backups lie,
//! the API surface, sitemap/feed and the git remote.

use rand::RngExt;
use rand::seq::{IndexedRandom, SliceRandom};
use rand_chacha::ChaCha8Rng;

use super::identity::Identity;
use super::tech::Tech;
use crate::data::{Data, OneOf};
use crate::persona::fill::{Vars, fill};
use crate::persona::model::{Area, AreaKind, Database, DotPolicy, Envelope, Family, Frontend, LogKind, Site};
use crate::util::{chance, pick_str, weighted_str};

pub fn derive(rng: &mut ChaCha8Rng, data: &'static Data, tech: &Tech, id: &Identity, db: &Database, dotfiles: DotPolicy) -> Site {
    let facts = data.stack(tech.stack);
    let infra = &data.infra;

    let mut vocabulary: Vec<&'static str> = id.industry.vocabulary.iter().map(String::as_str).collect();
    vocabulary.shuffle(rng);
    vocabulary.truncate(rng.random_range(7..vocabulary.len().max(8)));

    let areas = areas(rng, facts.areas.as_deref().unwrap_or(&infra.default_areas), tech.frontend);

    // Config leaks: `.env` and friends for dotenv stacks, the stack's own
    // backup copies otherwise, and a fallback so every box leaks something.
    let mut env_paths = Vec::new();
    if chance(rng, facts.dotenv) {
        if chance(rng, 0.85) {
            env_paths.push("/.env".to_string());
        }
        for extra in &infra.dotenv_extras {
            if chance(rng, infra.dotenv_extra_p) {
                env_paths.push(format!("/{extra}"));
            }
        }
        env_paths.extend(facts.dotenv_extras.iter().filter(|m| chance(rng, m.p)).map(|m| m.path.clone()));
        if env_paths.is_empty() {
            env_paths.push("/.env.bak".to_string());
        }
    }
    let mut config_leaks: Vec<String> = facts.config_leaks.iter().filter(|m| chance(rng, m.p)).map(|m| m.path.clone()).collect();
    if config_leaks.is_empty() {
        config_leaks.extend(facts.config_fallback.clone());
    }

    let vars = Vars::default().with("short", id.short()).with("db", db.name.clone()).with("db_dashed", db.name.replace('_', "-"));
    let mut backups = Vec::new();
    for i in 0..rng.random_range(0..4) {
        let vars = Vars { pairs: vars.pairs.clone() }.with("index", i.to_string());
        let stem = fill(pick_str(rng, &infra.backup_stems), &vars, rng);
        backups.push(format!("{}{stem}.{}", pick_str(rng, &infra.backup_dirs), pick_str(rng, &infra.backup_exts)));
    }
    backups.dedup();

    let mut logs = Vec::new();
    if let Some(path) = facts.app_logs.as_ref().and_then(|o| one_of(rng, o)) {
        logs.push((path, LogKind::App));
    }
    if chance(rng, infra.access_log_p) {
        logs.push((pick_str(rng, &infra.access_logs).to_string(), LogKind::Access));
    }
    if chance(rng, infra.error_log_p) {
        logs.push((pick_str(rng, &infra.error_logs).to_string(), LogKind::Error));
    }

    let api_resources = match &facts.api_resources {
        Some(fixed) => fixed.iter().map(String::as_str).collect(),
        None => {
            let mut r: Vec<&'static str> = id.industry.resources.iter().map(String::as_str).collect();
            r.shuffle(rng);
            r.truncate(rng.random_range(2..r.len().max(3)));
            r.push("users");
            r
        }
    };
    let envelope = crate::util::weighted_key(rng, &facts.envelopes);
    let per_page = match envelope {
        Envelope::SpringPage | Envelope::Hal => 20,
        Envelope::DataLinksMeta => *[15, 15, 25, 50].choose(rng).unwrap_or(&15),
        _ => *[10, 20, 25, 50, 100].choose(rng).unwrap_or(&20),
    };

    let sitemap = one_of(rng, facts.sitemaps.as_ref().unwrap_or(&infra.default_sitemaps));
    let has_blog = areas.iter().any(|a| a.kind == AreaKind::Blog);
    let feed = match &facts.feeds {
        Some(f) => one_of(rng, f),
        None if has_blog => one_of(rng, &infra.default_feeds),
        None => None,
    };

    let git_host = weighted_str(rng, &infra.git_hosts);
    let org = match rng.random_range(0..3) {
        0 => id.short().to_string(),
        1 => id.slug.clone(),
        _ => format!("{}-{}", id.short(), pick_str(rng, &infra.git_org_suffixes)),
    };
    let repo = pick_str(rng, facts.repos.as_deref().unwrap_or(&infra.default_repos));
    let git_remote = match (git_host, rng.random_bool(0.5)) {
        ("self", _) => format!("git@git.{}:{org}/{repo}.git", id.domain),
        (h, true) => format!("git@{h}:{org}/{repo}.git"),
        (h, false) => format!("https://{h}/{org}/{repo}.git"),
    };

    // Behind `deny all` for dotfiles only non-dot leftovers can leak; make
    // sure at least one exists.
    let dot_ok = dotfiles == DotPolicy::Serve;
    if !dot_ok {
        env_paths.clear();
        if config_leaks.is_empty() && backups.is_empty() {
            backups.push(format!("/backup/{}.sql", db.name));
        }
    }

    let php = facts.family == Family::Php;
    Site {
        areas,
        vocabulary,
        env_paths,
        config_leaks,
        backups,
        logs,
        git: dot_ok && chance(rng, 0.55),
        aws_credentials: dot_ok && chance(rng, 0.12),
        ssh_key: dot_ok && chance(rng, 0.08),
        htpasswd: dot_ok && tech.frontend == Frontend::Apache && chance(rng, 0.35),
        phpinfo: (php && chance(rng, 0.4)).then(|| pick_str(rng, &infra.phpinfo_paths).to_string()),
        server_status: tech.frontend == Frontend::Apache && chance(rng, 0.3),
        docker_compose: tech.frontend != Frontend::Iis && chance(rng, 0.25),
        sitemap,
        feed,
        login_path: pick_str(rng, &facts.login_paths).to_string(),
        api_prefix: pick_str(rng, &facts.api_prefixes).to_string(),
        api_resources,
        envelope,
        per_page,
        git_remote,
        default_branch: weighted_str(rng, &infra.default_branches),
    }
}

fn one_of(rng: &mut ChaCha8Rng, o: &OneOf) -> Option<String> {
    (chance(rng, o.p) && !o.paths.is_empty()).then(|| pick_str(rng, &o.paths).to_string())
}

/// Content areas: where the endless-but-plausible site structure lives.
fn areas(rng: &mut ChaCha8Rng, words: &[String], frontend: Frontend) -> Vec<Area> {
    let mut words: Vec<&String> = words.iter().collect();
    words.shuffle(rng);
    let count = rng.random_range(1..4);
    words
        .into_iter()
        .take(count)
        .map(|w| {
            let listable = ["upload", "files", "downloads", "images", "media"].iter().any(|k| w.contains(k));
            let kind = if listable && frontend != Frontend::LiteSpeed {
                AreaKind::Autoindex
            } else if ["doc", "kb", "help"].iter().any(|k| w.contains(k)) {
                AreaKind::Docs
            } else {
                AreaKind::Blog
            };
            Area { prefix: format!("/{w}/"), kind }
        })
        .collect()
}
