//! Business records (`orders`, `patients`, ...): dense ids, created in id
//! order, owned by users who had already signed up.

use rand::RngExt;
use serde::Serialize;

use super::{World, people};
use crate::persona::{capitalize, short_slug};
use crate::util::{self, Tm, pick, pick_str};

/// A typed cell, so each format can render it its own way.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Value {
    Null,
    Int(i64),
    /// Decimal kept as text (`12.50`), never a float.
    Num(String),
    Str(String),
    Bool(bool),
    /// Unix timestamp.
    Ts(i64),
}

#[derive(Clone, Debug, Serialize)]
pub struct Cell {
    pub name: &'static str,
    pub value: Value,
}

pub fn count(w: &World, resource: &str) -> u64 {
    let mut rng = w.fixed_rng(&format!("count:{resource}"));
    people::count(w) * rng.random_range(1..6) / rng.random_range(1..4)
}

/// Field names of a resource, in table order.
pub fn columns(w: &World, resource: &str) -> Vec<&'static str> {
    record(w, resource, 1).into_iter().map(|c| c.name).collect()
}

pub fn record(w: &World, resource: &str, id: u64) -> Vec<Cell> {
    let p = w.persona;
    let world = &w.data().world;
    let shape = world.shape(resource);
    let mut rng = util::keyed_rng(&p.mac(&[b"record"]), &[resource.as_bytes(), &id.to_le_bytes()]);

    let start = w.founded();
    let anchor = w.epochs.cfg_anchor;
    let gap = ((anchor - start) / count(w, resource).max(1) as i64).max(1);
    let created = start + id as i64 * gap + rng.random_range(0..gap.max(2) / 2);
    let users = people::count(w).max(1) as i64;
    let signed_up = ((created - start) / people::signup_gap(w)).clamp(1, users) as u64;
    let updated = created + rng.random_range(0..(anchor - created).max(1)) / 4;
    let code = resource.chars().take(3).collect::<String>().to_ascii_uppercase();

    let mut cells = vec![Cell { name: "id", value: Value::Int(id as i64) }];
    if shape.owner {
        cells.push(Cell { name: "user_id", value: Value::Int(rng.random_range(1..=signed_up) as i64) });
    }
    let (first, last) = p.person_name(&mut rng);
    for field in &shape.fields {
        let value = match field.as_str() {
            "name" => {
                Value::Str(format!("{} {}", capitalize(&pick(&mut rng, &p.site.vocabulary).replace('-', " ")), rng.random_range(1..400)))
            }
            "sku" => Value::Str(format!("{code}-{id:05}")),
            "price" | "total" => Value::Num(format!("{}.{:02}", rng.random_range(5..5_000), rng.random_range(0..100))),
            "first_name" => Value::Str(capitalize(first)),
            "last_name" => Value::Str(capitalize(last)),
            "email" => Value::Str(format!("{first}.{last}{}@{}", rng.random_range(1..99), pick_str(&mut rng, &world.email_domains))),
            "phone" => Value::Str(format!(
                "+{} {} {:07}",
                pick(&mut rng, &world.phone_codes),
                rng.random_range(100..999),
                rng.random_range(0..9_999_999)
            )),
            "key_name" => Value::Str(pick_str(&mut rng, &world.api_key_names).to_string()),
            "token" => Value::Str(format!("{}_{}", short_slug(&p.slug), util::token(&mut rng, util::ALNUM, 40))),
            "title" => Value::Str(format!(
                "{} {}",
                capitalize(&pick(&mut rng, &p.site.vocabulary).replace('-', " ")),
                pick_str(&mut rng, &world.title_words)
            )),
            "slug" => Value::Str(format!("{}-{id}", pick(&mut rng, &p.site.vocabulary))),
            "reference" => Value::Str(format!("{code}-{}-{id:06}", Tm::at(created, 0).year)),
            "currency" => Value::Str(pick_str(&mut rng, &world.currencies).to_string()),
            _ => Value::Null,
        };
        cells.push(Cell { name: field_name(field), value });
    }
    cells.push(Cell { name: "status", value: Value::Str(pick_str(&mut rng, world.statuses(resource)).to_string()) });
    cells.push(Cell { name: "created_at", value: Value::Ts(created) });
    cells.push(Cell { name: "updated_at", value: Value::Ts(updated) });
    cells
}

/// Column name for a data-file field id (`key_name` is stored as `name`).
fn field_name(field: &str) -> &'static str {
    match field {
        "key_name" | "name" => "name",
        "sku" => "sku",
        "price" => "price",
        "total" => "total",
        "first_name" => "first_name",
        "last_name" => "last_name",
        "email" => "email",
        "phone" => "phone",
        "token" => "token",
        "title" => "title",
        "slug" => "slug",
        "reference" => "reference",
        "currency" => "currency",
        _ => "extra",
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;

    fn ts(cells: &[Cell], name: &str) -> i64 {
        match cells.iter().find(|c| c.name == name).map(|c| &c.value) {
            Some(Value::Ts(t)) => *t,
            other => panic!("{name}: {other:?}"),
        }
    }

    #[test]
    fn records_are_chronological_and_owned_by_earlier_users() {
        let p = persona(5);
        let w = World::new(&p, epochs());
        let res = "orders";
        let mut prev = 0;
        for id in 1..300 {
            let r = record(&w, res, id);
            let created = ts(&r, "created_at");
            assert!(created >= prev);
            prev = created;
            // The very first records belong to the admin who set things up.
            if let Some(Value::Int(owner)) = r.iter().find(|c| c.name == "user_id").map(|c| &c.value) {
                assert!(*owner == 1 || people::user(&w, *owner as u64).created <= created + people::signup_gap(&w));
            }
        }
    }

    #[test]
    fn shapes_come_from_data() {
        let p = persona(6);
        let w = World::new(&p, epochs());
        assert_eq!(columns(&w, "patients"), ["id", "first_name", "last_name", "email", "phone", "status", "created_at", "updated_at"]);
        assert!(columns(&w, "invoices").contains(&"reference"));
    }
}
