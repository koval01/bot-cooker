//! The repository history: commits by the company's developers, in time
//! order, written in the team's own commit-message convention.

use rand::{Rng, RngExt};
use serde::Serialize;

use super::World;
use crate::persona::{Person, capitalize};
use crate::util::{self, pick, pick_str};

#[derive(Clone, Debug, Serialize)]
pub struct Commit {
    pub n: u64,
    pub sha: String,
    pub parent: String,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub subject: String,
}

/// How many commits exist; the repo keeps moving between config epochs.
pub fn count(w: &World) -> u64 {
    let mut r = w.fixed_rng("git-commits");
    r.random_range(40..900) + w.epochs.cfg % 500 * r.random_range(1..6)
}

pub fn sha(w: &World, n: u64) -> String {
    w.persona.mac(&[b"commit", &n.to_le_bytes()])[..20].iter().map(|b| format!("{b:02x}")).collect()
}

/// The developers: a third of the staff, plus whoever set the repo up.
pub fn developers<'a>(w: &World<'a>) -> Vec<&'a Person> {
    let staff = &w.persona.staff;
    let n = (staff.len() / 3).max(1);
    staff.iter().skip(1).take(n).chain(staff.first()).collect()
}

pub fn commit(w: &World, n: u64) -> Commit {
    let devs = developers(w);
    let total = count(w).max(1);
    let span = (w.epochs.cfg_anchor - w.founded()).max(86_400) / total as i64;
    let mut r = util::keyed_rng(&w.persona.mac(&[b"reflog"]), &[&n.to_le_bytes()]);
    let dev = devs[r.random_range(0..devs.len())];
    Commit {
        n,
        sha: sha(w, n),
        parent: if n == 0 { "0".repeat(40) } else { sha(w, n - 1) },
        author: dev.display(),
        email: dev.email.clone(),
        time: w.founded() + n as i64 * span + r.random_range(0..span.max(2) / 2),
        subject: subject(w, &mut r),
    }
}

/// The last `limit` commits, oldest first.
pub fn recent(w: &World, limit: u64) -> Vec<Commit> {
    let n = count(w);
    (n.saturating_sub(limit)..n).map(|i| commit(w, i)).collect()
}

fn subject(w: &World, r: &mut impl Rng) -> String {
    let words = &w.data().words;
    let resource = pick(r, &w.persona.site.api_resources);
    let tail = pick_str(r, &words.commit_tails).replace("{resource}", resource);
    let body = format!(
        "{} {}{}",
        pick_str(r, &words.commit_verbs),
        pick_str(r, &words.commit_objects),
        if tail.is_empty() { String::new() } else { format!(" {tail}") }
    );
    // The team convention is fixed per persona.
    match w.fixed_rng("commit-style").random_range(0..4) {
        0 => format!("{}: {body}", pick_str(r, &words.commit_types)),
        1 => capitalize(&body),
        2 => format!("{}-{} {}", ticket_key(w), r.random_range(10..900), capitalize(&body)),
        _ => body,
    }
}

/// Jira-style project key from the company name (`Northwind Freight` -> `NF`).
fn ticket_key(w: &World) -> String {
    let initials: String = w.persona.company.split_whitespace().filter_map(|s| s.chars().next()).collect();
    if initials.len() >= 2 {
        initials.to_ascii_uppercase()
    } else {
        w.persona.slug.chars().take(3).collect::<String>().to_ascii_uppercase()
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;

    #[test]
    fn history_is_linear_and_chronological() {
        let p = persona(7);
        let w = World::new(&p, epochs());
        let log = recent(&w, 50);
        assert_eq!(log.len(), 50);
        assert!(log.windows(2).all(|c| c[0].time <= c[1].time && c[1].parent == c[0].sha));
        assert!(log.iter().all(|c| c.email.ends_with(&p.domain)));
    }

    #[test]
    fn subjects_vary_and_follow_one_convention() {
        let p = persona(8);
        let w = World::new(&p, epochs());
        let subjects: std::collections::HashSet<String> = recent(&w, 100).into_iter().map(|c| c.subject).collect();
        assert!(subjects.len() > 80, "{} distinct", subjects.len());
    }
}
