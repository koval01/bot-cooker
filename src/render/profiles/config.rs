//! Leaked configuration files. The template follows the file name
//! (`config_files.yaml`); the content follows the persona, so the same DB
//! name, user, password, mail provider and cloud show up in `.env`,
//! `wp-config.php.bak` or `docker-compose.yml` alike.

use minijinja::{Value, context};
use rand::RngExt;
use rand_chacha::ChaCha8Rng;
use serde::Serialize;

use crate::catalog::to_crlf;
use crate::data::{EnvVar, NoteData};
use crate::persona::{Cloud, DbEngine, fill, short_slug};
use crate::render::doc::Doc;
use crate::render::scope::{Scope, ser};
use crate::render::static_origin;
use crate::routing::glob_match;
use crate::util::{self, Tm, chance, pick_str, weighted};

/// Derived values configs share (see the header of data/env/common.yaml).
#[derive(Serialize)]
pub struct Shared {
    pub db_url: String,
    pub redis_url: String,
    pub mail_user: String,
    pub mail_password: String,
    pub aws_key_id: String,
    pub aws_secret: String,
    pub bucket: String,
    pub region: String,
    pub sentry_dsn: String,
    pub has_redis: bool,
    pub uses_aws: bool,
}

pub fn shared(scope: &Scope) -> Shared {
    let w = &scope.world;
    let p = scope.persona();
    let scheme = match (p.db.engine, p.stack.id()) {
        (DbEngine::Postgres, "django") => "postgres",
        (DbEngine::Postgres, _) => "postgresql",
        (DbEngine::MsSql, _) => "sqlserver",
        _ => "mysql",
    };
    let redis = p.cache.as_ref().filter(|c| c.redis);
    let mut fixed = w.fixed_rng("config-shared");
    let mail_user = match p.mail.provider {
        "sendgrid" => "apikey".to_string(),
        "ses" => format!("AKIA{}", w.secret_as("ses_user", "upper", 16)),
        "mailgun" => format!("postmaster@mg.{}", p.domain),
        _ => p.mail.from.clone(),
    };
    let mail_password = match p.mail.provider {
        "sendgrid" => format!("SG.{}.{}", w.secret_as("sg1", "b64url", 22), w.secret_as("sg2", "b64url", 43)),
        "postmark" => util::uuid(&mut util::keyed_rng(&w.secret_key(), &[b"postmark"])),
        _ => w.secret_as("mail_password", "alnum", 32),
    };
    Shared {
        db_url: format!("{scheme}://{}:{}@{}:{}/{}", p.db.user, w.secret("db_password"), p.db.host, p.db.port, p.db.name),
        redis_url: match redis {
            Some(c) if c.has_password => format!("redis://:{}@{}:{}/0", w.secret("redis_password"), c.host, c.port),
            Some(c) => format!("redis://{}:{}/0", c.host, c.port),
            None => "redis://127.0.0.1:6379/0".into(),
        },
        mail_user,
        mail_password,
        aws_key_id: format!("AKIA{}", w.secret_as("aws_key_id", "upper", 16)),
        aws_secret: w.secret_as("aws_secret", "b64", 40),
        bucket: format!(
            "{}-{}",
            short_slug(&p.slug),
            util::pick(&mut fixed, &["uploads", "media", "assets", "backups", "storage", "prod"])
        ),
        region: if p.cloud == Cloud::Aws {
            p.region.to_string()
        } else {
            util::pick(&mut fixed, &["us-east-1", "eu-central-1", "eu-west-1"]).to_string()
        },
        sentry_dsn: format!(
            "https://{}@o{}.ingest.sentry.io/{}",
            util::hex(&mut fixed, 16),
            fixed.random_range(100_000..999_999),
            fixed.random_range(1_000_000..9_999_999)
        ),
        has_redis: redis.is_some(),
        uses_aws: p.cloud == Cloud::Aws || fixed.random_bool(0.25),
    }
}

pub fn render(scope: &Scope) -> Doc {
    let p = scope.persona();
    let file = scope.req.path.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
    let template = p
        .data
        .config_files
        .files
        .iter()
        .find(|f| glob_match(&f.pattern.to_ascii_lowercase(), &file))
        .map(|f| f.template.clone())
        .unwrap_or_else(|| "common/dotenv.j2".into());
    let cfg = shared(scope);
    let extra = if template == "common/dotenv.j2" {
        context! { cfg => ser(&cfg), groups => ser(&dotenv_groups(scope, &cfg, file.ends_with(".example"))), header => scope.world.fixed_rng("env-header").random_bool(0.25) }
    } else {
        context! { cfg => ser(&cfg), note => note(scope, &mut scope.world.cfg_rng("config-note", &scope.req.path)) }
    };
    let mut body = scope.render_or_empty(&template, extra);
    if !p.style.trailing_newline && template.ends_with("dotenv.j2") {
        body.truncate(body.trim_end_matches('\n').len());
    }
    if p.style.crlf {
        body = to_crlf(&body);
    }
    Doc::new(static_origin(scope, "config"), None, body)
}

#[derive(Serialize)]
struct Line {
    kind: &'static str,
    key: String,
    value: String,
}

#[derive(Serialize)]
struct Group {
    name: String,
    lines: Vec<Line>,
}

/// The `.env` content: groups and lines, with presence decided once per
/// persona (and drifting a little per epoch), notes and commented-out vars.
fn dotenv_groups(scope: &Scope, cfg: &Shared, example: bool) -> Vec<Group> {
    let p = scope.persona();
    let data = p.data;
    let env = data.env.get(p.stack.id()).or_else(|| data.env.get("express"));
    let common = data.env.get("common");
    let Some(env) = env else { return Vec::new() };
    let mut layout = scope.world.fixed_rng("env-layout");
    let mut drift = scope.world.cfg_rng("env-drift", "");
    let base = context! { cfg => ser(cfg), ..scope.base() };
    let eval = |tpl: &str, extra: &Value| scope.catalog.render_str(tpl, context! { ..extra.clone(), ..base.clone() }).unwrap_or_default();
    let truthy = |tpl: &Option<String>, extra: &Value| tpl.as_ref().is_none_or(|t| eval(t, extra).trim().eq_ignore_ascii_case("true"));

    let mut groups = Vec::new();
    for g in env.list() {
        let extra = context! { region_key => g.region_key.clone().unwrap_or_else(|| "AWS_DEFAULT_REGION".into()) };
        if !truthy(&g.when, &extra) {
            continue;
        }
        let vars: &[EnvVar] = match (&g.uses, common) {
            (Some(name), Some(c)) => c.named(name),
            _ => &g.vars,
        };
        let mut lines = Vec::new();
        for var in vars {
            if !truthy(&var.when, &extra) {
                continue;
            }
            let present = layout.random_bool(var.p.clamp(0.0, 1.0));
            // A little epoch drift: optional vars come and go.
            let present = if var.p < 1.0 && chance(&mut drift, 0.05) { !present } else { present };
            let key = eval(&var.key, &extra);
            let value = if example && looks_secret(&key) { String::new() } else { eval(&var.value, &extra) };
            if !present {
                if layout.random_bool(p.style.comment_density * 0.5) {
                    lines.push(Line { kind: "disabled", key, value });
                }
                continue;
            }
            if layout.random_bool(p.style.comment_density * 0.3) {
                lines.push(Line { kind: "note", key: String::new(), value: note(scope, &mut drift) });
            }
            lines.push(Line { kind: "var", key, value });
        }
        if !lines.is_empty() {
            groups.push(Group { name: g.name.clone(), lines });
        }
    }
    groups
}

fn looks_secret(key: &str) -> bool {
    ["PASSWORD", "SECRET", "KEY", "TOKEN", "DSN", "SALT", "PASS"].iter().any(|s| key.contains(s))
        || (key.ends_with("_URL") && key.contains("DATABASE"))
}

/// A comment someone left (data/notes.yaml), dated before the epoch.
pub fn note(scope: &Scope, rng: &mut ChaCha8Rng) -> String {
    let p = scope.persona();
    let notes: &NoteData = &p.data.notes;
    if notes.patterns.is_empty() {
        return String::new();
    }
    let weights: Vec<(usize, u32)> = notes.patterns.iter().enumerate().map(|(i, n)| (i, n.weight)).collect();
    let pattern = &notes.patterns[weighted(rng, &weights)].text;
    let dev = &p.staff[rng.random_range(0..p.staff.len())];
    let who = if rng.random_bool(0.5) { dev.first.to_string() } else { dev.login.clone() };
    let date = Tm::at(scope.world.epochs.cfg_anchor - rng.random_range(5..300) * 86_400, 0).date();
    let initials: String = p.company.split_whitespace().filter_map(|w| w.chars().next()).collect();
    let mut vars = fill::Vars::default()
        .with("who", who)
        .with("date", date)
        .with("ticket", format!("{}-{}", initials.to_ascii_uppercase(), rng.random_range(10..2000)));
    for (name, values) in &notes.parts {
        vars = vars.with(name.as_str(), pick_str(rng, values).to_string());
    }
    fill::fill(pattern, &vars, rng)
}
