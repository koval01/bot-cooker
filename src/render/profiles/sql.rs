//! Database dumps: the stack's schema (data/schemas.yaml), rows from the
//! world, written in the persona's dialect by `sql/<dialect>/*.j2`. The big
//! users table is streamed in INSERT/COPY blocks; everything after it is the
//! tail, so a capped stream still ends with a complete dump.

use minijinja::context;
use rand::RngExt;
use serde::Serialize;

use crate::catalog::to_crlf;
use crate::data::{RowsSpec, TableSpec};
use crate::persona::{DbEngine, Naming, convert_case};
use crate::render::artifact::StreamKind;
use crate::render::doc::Doc;
use crate::render::scope::{Scope, ser};
use crate::render::static_origin;
use crate::util::{self, parse_date};
use crate::world::people::{self, User};
use crate::world::records::{self, Value};

#[derive(Clone, Debug, Serialize)]
struct Column {
    name: String,
    #[serde(rename = "type")]
    ty: String,
    null: bool,
    #[serde(skip)]
    src: String,
}

#[derive(Clone, Debug)]
enum Rows {
    Users,
    Resource(&'static str),
    Fixed(Vec<Vec<Value>>),
}

#[derive(Clone, Debug, Serialize)]
struct Table {
    name: String,
    columns: Vec<Column>,
    count: u64,
    #[serde(skip)]
    rows: Rows,
}

fn dialect(scope: &Scope) -> &'static str {
    match scope.persona().db.engine {
        DbEngine::MySql => "mysql",
        DbEngine::MariaDb => "mariadb",
        DbEngine::Postgres => "postgres",
        DbEngine::MsSql => "mssql",
    }
}

/// Template for a dump part, falling back from mariadb to mysql.
fn part(scope: &Scope, file: &str) -> String {
    let own = format!("sql/{}/{file}", dialect(scope));
    scope.pick(&[own.clone(), format!("sql/mysql/{file}")]).unwrap_or(own)
}

fn yaml_text(v: &serde_norway::Value) -> String {
    match v {
        serde_norway::Value::String(s) => s.clone(),
        serde_norway::Value::Number(n) => n.to_string(),
        serde_norway::Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn typed(ty: &str, text: &str) -> Value {
    match ty {
        _ if text.is_empty() && ty != "text" && !ty.starts_with("str") => Value::Null,
        "pk" | "int" | "bigint" => text.parse().map(Value::Int).unwrap_or(Value::Null),
        "ts" => text.parse().map(Value::Ts).or_else(|_| parse_date(text).map(Value::Ts).ok_or(())).unwrap_or(Value::Null),
        "bool" => Value::Bool(text == "true" || text == "1"),
        "money" => Value::Num(text.to_string()),
        _ => Value::Str(text.to_string()),
    }
}

fn schema(scope: &Scope) -> Vec<Table> {
    let p = scope.persona();
    let schemas = &p.data.schemas;
    let own = schemas.get(p.stack.id());
    let file = own.or_else(|| schemas.get("default"));
    let Some(file) = file else { return Vec::new() };
    let naming = match p.stack.id() {
        "express" => Naming::Camel,
        "asp_net" => Naming::Pascal,
        _ => Naming::Snake,
    };
    let mut specs: Vec<&TableSpec> = Vec::new();
    if let Some(base) = file.extends.as_ref().and_then(|b| schemas.get(b)) {
        specs.extend(base.tables.iter());
    }
    specs.extend(file.tables.iter());
    let convert = file.naming || file.extends.as_ref().and_then(|b| schemas.get(b)).is_some_and(|b| b.naming);

    let w = &scope.world;
    let tagline = p.sentence(&mut w.fixed_rng("tagline"), 12);
    let eval = |tpl: &str, i: u64| {
        scope
            .catalog
            .render_str(tpl, context! { founded => w.founded(), i => i, tagline => tagline.clone(), ..scope.base() })
            .unwrap_or_default()
    };

    let mut tables: Vec<Table> = specs
        .into_iter()
        .map(|spec| {
            let columns: Vec<Column> = spec
                .columns
                .iter()
                .map(|c| {
                    let name = yaml_text(&c[0]);
                    let name = if convert && !spec.name.contains("migration") && !spec.name.contains("flyway") {
                        convert_case(&name, naming)
                    } else {
                        name
                    };
                    Column {
                        name,
                        ty: c.get(1).map(yaml_text).unwrap_or_default(),
                        null: c.get(2).and_then(|v| v.as_bool()).unwrap_or(false),
                        src: c.get(3).map(yaml_text).unwrap_or_default(),
                    }
                })
                .collect();
            let name = eval(&spec.name, 0).replace("{prefix}", &p.db.table_prefix);
            let row_of = |texts: Vec<String>| texts.iter().zip(&columns).map(|(t, c)| typed(&c.ty, t)).collect::<Vec<_>>();
            let rows = match (&spec.rows, &spec.generate) {
                (Some(RowsSpec::Kind(k)), _) if k == "users" => Rows::Users,
                (Some(RowsSpec::Literal(lits)), _) => {
                    Rows::Fixed(lits.iter().map(|r| row_of(r.iter().map(|v| eval(&yaml_text(v), 0)).collect())).collect())
                }
                (_, Some(g)) => {
                    let n = match g.count.split_once("..") {
                        Some((a, b)) => {
                            // An overlay may write an empty range; never panic on it.
                            let lo: u64 = a.parse().unwrap_or(1);
                            w.fixed_rng(&format!("gen:{name}")).random_range(lo..b.parse().unwrap_or(2).max(lo + 1))
                        }
                        None if g.count == "staff" => p.staff.len() as u64,
                        None => g.count.parse().unwrap_or(1),
                    };
                    Rows::Fixed((0..n).map(|i| row_of(g.row.iter().map(|t| eval(t, i)).collect())).collect())
                }
                _ => Rows::Fixed(Vec::new()),
            };
            let count = match &rows {
                Rows::Users => people::count(w),
                Rows::Resource(r) => records::count(w, r).min(400),
                Rows::Fixed(v) => v.len() as u64,
            };
            Table { name, columns, count, rows }
        })
        .collect();

    // A business table from the industry, for application stacks.
    if !file.cms {
        let res = p.site.api_resources.iter().copied().find(|r| *r != "users").unwrap_or("orders");
        let sample = records::record(w, res, 1);
        let columns = sample
            .iter()
            .map(|c| Column {
                name: convert_case(c.name, naming),
                ty: match (&c.value, c.name) {
                    (_, "id") => "pk".into(),
                    (Value::Int(_), _) => "bigint".into(),
                    (Value::Ts(_), _) => "ts".into(),
                    (Value::Num(_), _) => "money".into(),
                    _ => "str:255".into(),
                },
                null: c.name == "updated_at",
                src: String::new(),
            })
            .collect();
        let name = if naming == Naming::Pascal { convert_case(res, naming) } else { res.to_string() };
        tables.push(Table { name, columns, count: records::count(w, res).min(400), rows: Rows::Resource(res) });
    }
    // Dumps list tables in name order.
    tables.sort_by(|a, b| a.name.cmp(&b.name));
    tables
}

fn user_cell(scope: &Scope, u: &User, src: &str) -> Value {
    let p = scope.persona();
    match src {
        "id" => Value::Int(u.id as i64),
        "name" => Value::Str(u.name.clone()),
        "email" => Value::Str(u.email.clone()),
        "email_upper" => Value::Str(u.email.to_ascii_uppercase()),
        "login" => Value::Str(u.login.clone()),
        "first" => Value::Str(crate::persona::capitalize(u.first)),
        "last" => Value::Str(crate::persona::capitalize(u.last)),
        "hash" => Value::Str(u.hash.clone()),
        "token" if u.id.is_multiple_of(3) => Value::Null,
        "token" => Value::Str(u.token.clone()),
        "created" => Value::Ts(u.created),
        "updated" => Value::Ts(u.updated),
        "verified" if u.active => Value::Ts(u.created + 600),
        "last_login" => u.last_login.map(Value::Ts).unwrap_or(Value::Null),
        "superuser" => Value::Bool(u.id == 1),
        "staff" => Value::Bool(u.staff),
        "active" => Value::Bool(u.active),
        "active_int" => Value::Int(u.active as i64),
        "blocked" => Value::Int(!u.active as i64),
        "role" if p.stack.id() == "symfony" => Value::Str(if u.staff { "[\"ROLE_ADMIN\"]".into() } else { "[]".into() }),
        "role" => Value::Str(u.role.to_string()),
        "logins" => Value::Int((u.id as i64 * 7919) % 97),
        "empty" => Value::Str(String::new()),
        "zero" => Value::Int(0),
        "true" => Value::Bool(true),
        "params" => Value::Str("{}".into()),
        "lang" => Value::Str("en".into()),
        "tz" => Value::Str(p.tz_name.into()),
        "created_unix" => Value::Int(u.created),
        "updated_unix" => Value::Int(u.updated),
        "access_unix" => Value::Int(u.last_login.unwrap_or(0)),
        "uuid" | "uuid2" => Value::Str(util::uuid(&mut util::keyed_rng(&p.mac(&[b"uuid"]), &[src.as_bytes(), &u.id.to_le_bytes()]))),
        "stamp" => Value::Str(util::token(&mut util::keyed_rng(&p.mac(&[b"stamp"]), &[&u.id.to_le_bytes()]), util::UPPER_ALNUM, 32)),
        _ => Value::Null,
    }
}

fn row(scope: &Scope, table: &Table, id: u64) -> Vec<Value> {
    match &table.rows {
        Rows::Users => {
            let u = people::user(&scope.world, id);
            table.columns.iter().map(|c| user_cell(scope, &u, &c.src)).collect()
        }
        Rows::Resource(res) => records::record(&scope.world, res, id).into_iter().map(|c| c.value).collect(),
        Rows::Fixed(rows) => rows.get(id as usize - 1).cloned().unwrap_or_default(),
    }
}

/// Rows per streamed statement, fixed per persona and dialect.
fn per_block(scope: &Scope) -> u64 {
    let mut r = scope.world.fixed_rng("dump-style");
    match dialect(scope) {
        "postgres" => r.random_range(40..120),
        "mssql" => r.random_range(20..60),
        _ => r.random_range(15..80),
    }
}

struct Dump<'s, 'a> {
    scope: &'s Scope<'a>,
}

impl Dump<'_, '_> {
    fn render(&self, file: &str, extra: minijinja::Value) -> String {
        self.scope.render_or_empty(&part(self.scope, file), extra)
    }

    fn rows(&self, t: &Table, from: u64, to: u64) -> String {
        if from > to {
            return String::new();
        }
        let rows: Vec<Vec<Value>> = (from..=to).map(|id| row(self.scope, t, id)).collect();
        self.render("rows.sql.j2", context! { table => ser(t), rows => ser(&rows), extended => self.scope.persona().db.extended_insert })
    }

    fn data(&self, t: &Table) -> String {
        let mut out = self.render("data_open.sql.j2", context! { table => ser(t) });
        let step = per_block(self.scope);
        let mut from = 1;
        while from <= t.count {
            let to = (from + step - 1).min(t.count);
            out += &self.rows(t, from, to);
            from = to + 1;
        }
        out + &self.render("data_close.sql.j2", context! { table => ser(t) })
    }

    fn create(&self, t: &Table) -> String {
        self.render("table.sql.j2", context! { table => ser(t) })
    }
}

fn crlf_if_mssql(scope: &Scope, s: String) -> String {
    if dialect(scope) == "mssql" { to_crlf(&s) } else { s }
}

pub fn render(scope: &Scope) -> Doc {
    let tables = schema(scope);
    let Some(big) = tables.iter().position(|t| matches!(t.rows, Rows::Users)) else {
        return Doc::new(static_origin(scope, "sql"), None, Vec::new());
    };
    let d = Dump { scope };
    let ran = scope.world.epochs.cfg_anchor - scope.world.fixed_rng("dump-time").random_range(3_600..86_400);
    let mut head = d.render("preamble.sql.j2", context! { host_is_local => scope.world.fixed_rng("dump-host").random_bool(0.6) });
    let mut tail = String::new();
    if dialect(scope) == "postgres" {
        // pg_dump: all DDL first, then all data, then constraints.
        for t in &tables {
            head += &d.create(t);
        }
        for t in &tables[..big] {
            head += &d.data(t);
        }
        head += &d.render("data_open.sql.j2", context! { table => ser(&tables[big]) });
        tail += &d.render("data_close.sql.j2", context! { table => ser(&tables[big]) });
        for t in &tables[big + 1..] {
            tail += &d.data(t);
        }
    } else {
        for t in &tables[..big] {
            head += &d.create(t);
            head += &d.data(t);
        }
        head += &d.create(&tables[big]);
        head += &d.render("data_open.sql.j2", context! { table => ser(&tables[big]) });
        tail += &d.render("data_close.sql.j2", context! { table => ser(&tables[big]) });
        for t in &tables[big + 1..] {
            tail += &d.create(t);
            tail += &d.data(t);
        }
    }
    tail += &d.render("footer.sql.j2", context! { tables => ser(&tables), ran => ran });
    let blocks = tables[big].count.div_ceil(per_block(scope));
    Doc::new(static_origin(scope, "sql"), None, crlf_if_mssql(scope, head)).streamed(StreamKind::Sql, blocks, crlf_if_mssql(scope, tail))
}

pub fn block(scope: &Scope, seq: u64) -> Vec<u8> {
    let tables = schema(scope);
    let Some(t) = tables.iter().find(|t| matches!(t.rows, Rows::Users)) else { return Vec::new() };
    let step = per_block(scope);
    let from = seq * step + 1;
    let to = (from + step - 1).min(t.count);
    crlf_if_mssql(scope, Dump { scope }.rows(t, from, to)).into_bytes()
}
