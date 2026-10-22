//! The persona's route table: which paths "exist" on this instance.
//!
//! The stack decides what is native (a WordPress box has `/wp-login.php`, a
//! Django box has `/admin/login/`), coin flips taken once per persona decide
//! which leaks and leftovers lie around, and everything else falls through to
//! a realistic 404 (or 403 for denied dotfiles) instead of an endless maze.

use crate::config::{Profile, RouteRule};

use super::{AreaKind, DotPolicy, Favicon, Persona};

impl Persona {
    pub fn routes(&self) -> Vec<RouteRule> {
        let mut r = Routes::default();
        let site = &self.site;

        // The files pages link to: exact paths, ahead of every pattern.
        for f in self.assets.files.values() {
            r.add(&f.path, Profile::Asset);
        }
        if self.assets.favicon != Favicon::None {
            r.add("/favicon.ico", Profile::Asset);
        }

        // Leaks first, so later broad patterns cannot shadow them.
        for p in &site.env_paths {
            r.add(p, Profile::Env);
        }
        for p in &site.config_leaks {
            r.add(p, Profile::Env);
        }
        if site.docker_compose {
            r.add("/docker-compose.yml", Profile::Env);
        }
        if site.git {
            for p in [
                "/.git/HEAD",
                "/.git/config",
                "/.git/description",
                "/.git/packed-refs",
                "/.git/logs/HEAD",
                "/.git/ORIG_HEAD",
                "/.git/COMMIT_EDITMSG",
                "/.git/FETCH_HEAD",
            ] {
                r.add(p, Profile::Git);
            }
            r.add("/.git/refs/heads/*", Profile::Git);
        }
        for b in &site.backups {
            r.add(b, Profile::SqlDump);
        }
        for (path, _) in &site.logs {
            r.add(path, Profile::Log);
        }
        // Directories holding those leftovers: listable on some boxes,
        // `autoindex off` (403) on others.
        let listable = rand::RngExt::random_bool(&mut self.rng("dir-listing"), 0.5);
        for dir in self.leak_dirs() {
            r.add(&dir, if listable { Profile::Labyrinth } else { Profile::Forbidden });
        }
        if site.aws_credentials {
            r.add("/.aws/credentials", Profile::Secrets);
        }
        if site.ssh_key {
            r.add("/.ssh/id_rsa", Profile::Secrets);
        }
        if site.htpasswd {
            r.add("/.htpasswd", Profile::Secrets);
        }
        if let Some(p) = &site.phpinfo {
            r.add(p, Profile::PhpInfo);
        }
        if site.server_status {
            r.add("/server-status", Profile::ServerStatus);
        }

        // Dotfiles that aren't leaks: denied or simply missing.
        if self.web.dotfiles == DotPolicy::Deny {
            r.add("/.*", Profile::Forbidden);
            r.add("*/.*", Profile::Forbidden);
        }

        // Stack-native surface.
        r.add(&site.login_path, Profile::Login);
        for rule in &self.data.stack(self.stack).routes {
            r.add(&rule.pattern, rule.profile);
        }
        if !self.cms {
            r.add(&site.api_prefix, Profile::Json);
            r.add(&format!("{}/*", site.api_prefix), Profile::Json);
        }

        if let Some(p) = &site.sitemap {
            r.add(p, Profile::Sitemap);
            r.add(&sitemap_children(p), Profile::Sitemap);
        }
        if let Some(p) = &site.feed {
            r.add(p, Profile::Rss);
        }

        // Content areas: files linked from listings route to the matching
        // generator, everything else under the area is more site.
        for area in &site.areas {
            let pre = &area.prefix;
            if area.kind == AreaKind::Autoindex {
                for (ext, prof) in [
                    ("sql", Profile::SqlDump),
                    ("log", Profile::Log),
                    ("json", Profile::Json),
                    ("xml", Profile::Sitemap),
                    ("bak", Profile::Env),
                ] {
                    r.add(&format!("{pre}*.{ext}"), prof);
                }
            }
            r.add(&format!("{pre}*"), Profile::Labyrinth);
            r.add(pre.trim_end_matches('/'), Profile::Labyrinth);
        }
        r.add("/", Profile::Labyrinth);
        r.0
    }

    /// Non-root directories that contain leaked backups or logs.
    pub fn leak_dirs(&self) -> Vec<String> {
        let mut dirs: Vec<String> = self
            .site
            .backups
            .iter()
            .chain(self.site.logs.iter().map(|(p, _)| p))
            .filter_map(|p| p.rfind('/').filter(|i| *i > 0).map(|i| p[..=i].to_string()))
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }

    /// The bait lines for robots.txt — only paths that really "exist" here.
    pub fn robots_baits(&self) -> Vec<String> {
        let mut out = Vec::new();
        let site = &self.site;
        let dir_of = |p: &str| match p.rfind('/') {
            Some(0) | None => p.to_string(),
            Some(i) => p[..=i].to_string(),
        };
        for b in &site.backups {
            out.push(dir_of(b));
        }
        for (p, _) in &site.logs {
            out.push(dir_of(p));
        }
        if let Some(p) = &site.phpinfo {
            out.push(p.clone());
        }
        if !self.cms {
            out.push(format!("{}/", site.api_prefix));
        }
        if site.server_status {
            out.push("/server-status".to_string());
        }
        for a in &site.areas {
            if a.kind == AreaKind::Autoindex {
                out.push(a.prefix.clone());
            }
        }
        out.retain(|p| p != "/");
        out.sort();
        out.dedup();
        out
    }
}

/// `/wp-sitemap.xml` -> `/wp-sitemap-*.xml`
fn sitemap_children(index: &str) -> String {
    match index.strip_suffix(".xml") {
        Some(stem) => format!("{}-*.xml", stem.trim_end_matches("_index")),
        None => format!("{index}*"),
    }
}

#[derive(Default)]
struct Routes(Vec<RouteRule>);

impl Routes {
    fn add(&mut self, pattern: &str, profile: Profile) {
        if !self.0.iter().any(|r| r.pattern == pattern) {
            self.0.push(RouteRule { pattern: pattern.to_string(), profile });
        }
    }
}
