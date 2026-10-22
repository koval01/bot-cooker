//! Where template and data files come from: the `templates/` tree embedded at
//! build time, optionally overlaid by a directory on disk.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context;
use include_dir::{Dir, include_dir};

static EMBEDDED: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/templates");

/// File extensions the catalog cares about; anything else (READMEs) is ignored.
const TEMPLATE_EXT: &str = ".j2";
const DATA_EXT: &str = ".yaml";
/// Plain-text corpora are data too, but only under `data/`.
const CORPUS_EXT: &str = ".txt";

/// A flat map of `relative/path` to file contents.
#[derive(Clone, Debug, Default)]
pub struct Sources {
    files: BTreeMap<String, String>,
}

impl Sources {
    /// The tree compiled into the binary.
    pub fn embedded() -> Self {
        let mut s = Sources::default();
        collect_embedded(&EMBEDDED, &mut s);
        s
    }

    /// Replace or add files from `dir`, keeping the embedded ones it doesn't
    /// mention. Returns how many files were taken from disk.
    pub fn overlay_dir(&mut self, dir: &Path) -> anyhow::Result<usize> {
        let mut count = 0;
        walk(dir, dir, &mut |rel, text| {
            if is_relevant(rel) {
                self.files.insert(rel.to_string(), normalize(&text));
                count += 1;
            }
        })
        .with_context(|| format!("reading templates from {}", dir.display()))?;
        Ok(count)
    }

    pub fn insert(&mut self, name: &str, text: &str) {
        self.files.insert(name.to_string(), text.to_string());
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.files.get(name).map(String::as_str)
    }

    /// All files under a directory prefix, keyed by file stem.
    pub fn under<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
        self.files
            .iter()
            .filter(move |(k, _)| k.starts_with(prefix))
            .map(|(k, v)| (k.rsplit('/').next().unwrap_or(k).split('.').next().unwrap_or(k), v.as_str()))
    }

    pub fn templates(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files.iter().filter(|(k, _)| k.ends_with(TEMPLATE_EXT)).map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// Files edited on Windows (or checked out with `core.autocrlf`) must not
/// change what the trap serves: drop a BOM and store LF line endings.
/// Templates that need CRLF produce it at render time.
pub fn normalize(text: &str) -> String {
    text.strip_prefix('\u{feff}').unwrap_or(text).replace("\r\n", "\n")
}

fn is_relevant(name: &str) -> bool {
    name.ends_with(TEMPLATE_EXT) || name.ends_with(DATA_EXT) || (name.starts_with("data/") && name.ends_with(CORPUS_EXT))
}

fn collect_embedded(dir: &Dir<'static>, out: &mut Sources) {
    for file in dir.files() {
        let name = file.path().to_string_lossy().replace('\\', "/");
        if let (true, Some(text)) = (is_relevant(&name), file.contents_utf8()) {
            out.files.insert(name, normalize(text));
        }
    }
    for sub in dir.dirs() {
        collect_embedded(sub, out);
    }
}

fn walk(root: &Path, dir: &Path, f: &mut dyn FnMut(&str, String)) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(root, &path, f)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            let rel = rel.to_string_lossy().replace('\\', "/");
            f(&rel, std::fs::read_to_string(&path)?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_line_endings_and_bom_are_normalized() {
        assert_eq!(normalize("\u{feff}a: 1\r\nb: 2\r\n"), "a: 1\nb: 2\n");
        assert_eq!(normalize("x\ny"), "x\ny");
    }

    #[test]
    fn embedded_files_have_lf_line_endings() {
        let s = Sources::embedded();
        assert!(s.files.values().all(|t| !t.contains('\r')));
    }

    #[test]
    fn embedded_tree_is_loaded() {
        let s = Sources::embedded();
        assert!(s.templates().count() > 0, "templates/ is compiled in");
        assert!(s.get("README.md").is_none(), "non-template files are ignored");
    }

    #[test]
    fn overlay_replaces_and_adds() {
        let dir = std::env::temp_dir().join(format!("bot-cooker-overlay-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("site")).unwrap();
        std::fs::write(dir.join("site/robots.txt.j2"), "User-agent: *\n").unwrap();
        std::fs::write(dir.join("site/notes.txt"), "ignored outside data/").unwrap();
        let mut s = Sources::embedded();
        let n = s.overlay_dir(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(n, 1);
        assert_eq!(s.get("site/robots.txt.j2"), Some("User-agent: *\n"));
    }

    #[test]
    fn overlay_missing_dir_is_an_error() {
        let mut s = Sources::default();
        assert!(s.overlay_dir(Path::new("/definitely/not/here")).is_err());
    }
}
