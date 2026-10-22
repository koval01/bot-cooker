//! An exposed `.git/` directory of the persona's repository.

use minijinja::context;
use rand::RngExt;
use serde::Serialize;

use crate::render::doc::Doc;
use crate::render::scope::{Scope, ser};
use crate::render::static_origin;
use crate::world::vcs;

#[derive(Serialize)]
struct Ref {
    sha: String,
    name: String,
}

pub fn render(scope: &Scope) -> Doc {
    let w = &scope.world;
    let p = scope.persona();
    let n = vcs::count(w);
    let file = scope.req.path.trim_start_matches("/.git/");
    let name = match file {
        "HEAD" | "config" | "description" | "packed-refs" | "ORIG_HEAD" | "FETCH_HEAD" | "COMMIT_EDITMSG" => format!("vcs/{file}.j2"),
        "logs/HEAD" => "vcs/logs_HEAD.j2".into(),
        _ => "vcs/ref.j2".into(),
    };
    let mut tags = w.fixed_rng("git-tags");
    let mut refs = vec![Ref { sha: vcs::sha(w, n - 1), name: format!("refs/remotes/origin/{}", p.site.default_branch) }];
    for i in 0..tags.random_range(0..6u64) {
        refs.push(Ref { sha: vcs::sha(w, n / 2 + i * 5), name: format!("refs/tags/v{}.{}.0", 1 + i / 3, i % 3 * 2) });
    }
    refs.sort_by(|a, b| a.name.cmp(&b.name));
    let devs = vcs::developers(w);
    let dev = devs[0];
    let body = scope.render_or_empty(
        &name,
        context! {
            head => ser(&vcs::commit(w, n - 1)),
            previous => ser(&vcs::commit(w, n.saturating_sub(2))),
            commits => if name.ends_with("logs_HEAD.j2") { ser(&vcs::recent(w, 120)) } else { ser(&Vec::<vcs::Commit>::new()) },
            refs => ser(&refs),
            developer => context! { name => dev.display(), email => dev.email.clone() },
        },
    );
    Doc::new(static_origin(scope, "git"), Some("text/plain"), body)
}
