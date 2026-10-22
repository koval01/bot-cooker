//! Users: staff first (ids 1..=staff), customers after, created in id order.

use rand::{Rng, RngExt};
use serde::Serialize;

use super::World;
use crate::persona::capitalize;
use crate::util::{self, chance, pick, pick_str};

#[derive(Clone, Debug, Serialize)]
pub struct User {
    pub id: u64,
    pub first: &'static str,
    pub last: &'static str,
    pub name: String,
    pub email: String,
    pub login: String,
    pub created: i64,
    pub updated: i64,
    pub last_login: Option<i64>,
    pub staff: bool,
    pub active: bool,
    pub role: &'static str,
    pub hash: String,
    pub token: String,
}

/// Users grow slowly from epoch to epoch.
pub fn count(w: &World) -> u64 {
    let mut rng = w.fixed_rng("user-count");
    let base: u64 = match rng.random_range(0..4) {
        0 => rng.random_range(300..3_000),
        1 => rng.random_range(3_000..30_000),
        2 => rng.random_range(30_000..200_000),
        _ => rng.random_range(800..12_000),
    };
    let growth = rng.random_range(0..base / 2_000 + 2);
    base + growth * (w.epochs.cfg % 1000)
}

/// Seconds between consecutive sign-ups, on average.
pub fn signup_gap(w: &World) -> i64 {
    let span = (w.epochs.cfg_anchor - w.founded()).max(86_400);
    (span / count(w).max(1) as i64).max(1)
}

pub fn user(w: &World, id: u64) -> User {
    let p = w.persona;
    let world = &w.data().world;
    let mut rng = util::keyed_rng(&p.mac(&[b"user"]), &[&id.to_le_bytes()]);
    let gap = signup_gap(w);
    let created = w.founded() + id as i64 * gap + rng.random_range(0..gap.max(2) / 2);
    let (first, last, email, login, staff) = match id.checked_sub(1).and_then(|i| p.staff.get(i as usize)) {
        Some(s) => (s.first, s.last, s.email.clone(), s.login.clone(), true),
        None => {
            let (first, last) = p.person_name(&mut rng);
            let n = rng.random_range(1..999);
            let local = match rng.random_range(0..5) {
                0 => format!("{first}.{last}"),
                1 => format!("{first}{n}"),
                2 => format!("{}{last}{}", &first[..1], n % 100),
                3 => format!("{first}_{last}"),
                _ => format!("{last}.{first}{}", n % 10),
            };
            let email = format!("{local}@{}", pick_str(&mut rng, &world.email_domains));
            (first, last, email, local.replace('.', ""), false)
        }
    };
    let anchor = w.epochs.cfg_anchor;
    let updated = created + rng.random_range(0..(anchor - created).max(1));
    let last_login = chance(&mut rng, 0.7).then(|| updated + rng.random_range(0..(anchor - updated).max(1)));
    let role = match (staff, id) {
        (true, 1) => "admin",
        (true, _) => pick_str(&mut rng, &world.staff_roles),
        _ => pick_str(&mut rng, &world.customer_roles),
    };
    User {
        id,
        first,
        last,
        name: format!("{} {}", capitalize(first), capitalize(last)),
        email,
        login,
        created,
        updated,
        last_login,
        staff,
        active: chance(&mut rng, if staff { 0.9 } else { 0.85 }),
        role,
        hash: password_hash(w, &mut rng),
        token: util::token(&mut rng, util::ALNUM, 60),
    }
}

/// A password hash in the format the stack's framework writes.
fn password_hash(w: &World, rng: &mut impl Rng) -> String {
    let p = w.persona;
    let fw = &p.versions.framework;
    let mut fixed = w.fixed_rng("hash-style");
    match w.data().stack(p.stack).hash.as_str() {
        "pbkdf2_django" => {
            // Django raises the default iteration count every release.
            let iters = match (fw.major, fw.minor) {
                (3, _) => 260_000,
                (4, _) => 600_000,
                (5, 0) => 720_000,
                _ => 870_000,
            };
            format!("pbkdf2_sha256${iters}${}${}=", util::token(rng, util::ALNUM, 22), util::token(rng, util::B64, 43))
        }
        "phpass" => format!("$P$B{}", util::token(rng, util::BCRYPT, 30)),
        "drupal" if fw.major < 10 => format!("$S$E{}", util::token(rng, util::BCRYPT, 51)),
        "aspnet_identity_v3" => format!("AQAAAAIAAYagAAAAE{}==", util::token(rng, util::B64, 65)),
        "bcrypt_or_argon2" if chance(&mut fixed, 0.5) => {
            format!("$argon2id$v=19$m=65536,t=4,p=1${}${}", util::token(rng, util::B64, 22), util::token(rng, util::B64, 43))
        }
        "bcrypt_2a" => bcrypt(rng, "2a", pick(&mut fixed, &["10", "11", "12"])),
        "bcrypt_2b" => bcrypt(rng, pick(&mut fixed, &["2a", "2b"]), "10"),
        _ => bcrypt(rng, "2y", pick(&mut fixed, &["10", "10", "12"])),
    }
}

fn bcrypt(rng: &mut impl Rng, variant: &str, cost: &str) -> String {
    format!("${variant}${cost}${}", util::token(rng, util::BCRYPT, 53))
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;

    #[test]
    fn staff_come_first_and_ids_are_chronological() {
        let p = persona(3);
        let w = World::new(&p, epochs());
        let u1 = user(&w, 1);
        assert!(u1.staff && u1.email.ends_with(&p.domain) && u1.role == "admin");
        let n = p.staff.len() as u64 + 1;
        assert!(!user(&w, n).staff);
        let created: Vec<i64> = (1..200).map(|i| user(&w, i).created).collect();
        assert!(created.windows(2).all(|c| c[0] <= c[1]));
        assert!(created.iter().all(|c| *c <= w.epochs.cfg_anchor));
    }

    #[test]
    fn user_is_stable_across_requests() {
        let p = persona(4);
        let w = World::new(&p, epochs());
        assert_eq!(user(&w, 42).email, user(&w, 42).email);
        assert_eq!(user(&w, 42).hash, user(&w, 42).hash);
    }

    #[test]
    fn hashes_follow_the_stack() {
        for i in 0..60 {
            let p = persona(i);
            let w = World::new(&p, epochs());
            let h = user(&w, 5).hash;
            match w.data().stack(p.stack).hash.as_str() {
                "pbkdf2_django" => assert!(h.starts_with("pbkdf2_sha256$")),
                "phpass" => assert!(h.starts_with("$P$B")),
                "aspnet_identity_v3" => assert!(h.starts_with("AQAAAA")),
                _ => assert!(h.starts_with('$')),
            }
        }
    }
}
