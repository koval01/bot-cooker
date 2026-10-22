//! The template catalog: every artifact the trap serves is rendered by one
//! MiniJinja environment from files under `templates/`.
//!
//! The tree is compiled into the binary, so the default deployment touches no
//! disk; `deception.templates_dir` overlays a directory on top for operators
//! who want to change or add files. All templates are compiled at startup, so
//! a syntax error fails fast instead of on the first matching request.

mod filters;
mod json;
mod source;

pub use source::{Sources, normalize};

use anyhow::Context;
use minijinja::{AutoEscape, Environment, UndefinedBehavior};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub struct Catalog {
    env: Environment<'static>,
    sources: Sources,
}

impl Catalog {
    /// The embedded catalog, optionally overlaid from `dir`.
    pub fn load(dir: Option<&std::path::Path>) -> anyhow::Result<Self> {
        let mut sources = Sources::embedded();
        if let Some(dir) = dir {
            let n = sources.overlay_dir(dir)?;
            tracing::info!("templates: {n} file(s) overlaid from {}", dir.display());
        }
        Self::from_sources(sources)
    }

    pub fn from_sources(sources: Sources) -> anyhow::Result<Self> {
        let mut env = Environment::new();
        env.set_syntax(
            minijinja::syntax::SyntaxConfig::builder()
                .trim_blocks(true)
                .lstrip_blocks(true)
                .keep_trailing_newline(true)
                .build()
                .context("template syntax config")?,
        );
        // A missing variable is a bug in the template or the context, never
        // something to paper over with an empty string.
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        env.set_auto_escape_callback(auto_escape);
        env.set_formatter(markup_formatter);
        filters::register(&mut env);
        for (name, text) in sources.templates() {
            env.add_template_owned(name.to_string(), text.to_string()).with_context(|| format!("compiling template {name}"))?;
        }
        Ok(Catalog { env, sources })
    }

    /// Render a named template with LF line endings.
    pub fn render(&self, name: &str, ctx: impl Serialize) -> Result<String, minijinja::Error> {
        self.env.get_template(name)?.render(minijinja::Value::from(minijinja::value::Serde(ctx)))
    }

    /// Render a short inline pattern taken from a data file (e.g. a cookie
    /// format), through the same engine and filters.
    pub fn render_str(&self, source: &str, ctx: impl Serialize) -> Result<String, minijinja::Error> {
        self.env.render_str(source, minijinja::Value::from(minijinja::value::Serde(ctx)))
    }

    /// The source text of a catalog file.
    pub fn source(&self, name: &str) -> Option<String> {
        self.sources.get(name).map(str::to_string)
    }

    pub fn has(&self, name: &str) -> bool {
        self.env.get_template(name).is_ok()
    }

    pub fn template_names(&self) -> Vec<String> {
        self.env.templates().map(|(n, _)| n.to_string()).collect()
    }

    /// Plain-text data files under `prefix`, keyed by file stem.
    pub fn texts(&self, prefix: &str) -> std::collections::BTreeMap<String, String> {
        self.sources.under(prefix).map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    /// Deserialize every YAML file under `prefix`, keyed by file stem.
    pub fn data_dir<T: DeserializeOwned>(&self, prefix: &str) -> anyhow::Result<std::collections::BTreeMap<String, T>> {
        self.sources
            .under(prefix)
            .map(|(stem, text)| {
                Ok((stem.to_string(), serde_norway::from_str(text).with_context(|| format!("parsing {prefix}{stem}.yaml"))?))
            })
            .collect()
    }

    /// Deserialize a YAML data file from the catalog.
    pub fn data<T: DeserializeOwned>(&self, name: &str) -> anyhow::Result<T> {
        let text = self.sources.get(name).with_context(|| format!("data file {name} is missing"))?;
        serde_norway::from_str(text).with_context(|| format!("parsing data file {name}"))
    }
}

/// HTML-ish outputs are escaped; config files, SQL and logs are not.
fn auto_escape(name: &str) -> AutoEscape {
    let stem = name.strip_suffix(".j2").unwrap_or(name);
    if [".html", ".htm", ".xml"].iter().any(|ext| stem.ends_with(ext)) { AutoEscape::Html } else { AutoEscape::None }
}

/// Escape like real servers and frameworks do: `& < > " '` only. (MiniJinja's
/// default HTML escaping also turns `/` into `&#x2f;`, which no real page
/// does in URLs and would stand out.)
fn markup_formatter(out: &mut minijinja::Output, state: &mut minijinja::State, value: &minijinja::Value) -> Result<(), minijinja::Error> {
    match value.as_str() {
        Some(s) if matches!(state.auto_escape(), AutoEscape::Html) && !value.is_safe() => {
            let mut escaped = String::with_capacity(s.len());
            for c in s.chars() {
                match c {
                    '&' => escaped.push_str("&amp;"),
                    '<' => escaped.push_str("&lt;"),
                    '>' => escaped.push_str("&gt;"),
                    '"' => escaped.push_str("&quot;"),
                    '\'' => escaped.push_str("&#39;"),
                    c => escaped.push(c),
                }
            }
            out.write_str(&escaped).map_err(|_| minijinja::Error::from(minijinja::ErrorKind::WriteFailure))
        }
        _ => minijinja::escape_formatter(out, state, value),
    }
}

/// Convert LF output to CRLF where the imitated software writes CRLF.
pub fn to_crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use minijinja::context;

    fn catalog(files: &[(&str, &str)]) -> anyhow::Result<Catalog> {
        let mut s = Sources::default();
        for (n, t) in files {
            s.insert(n, t);
        }
        Catalog::from_sources(s)
    }

    #[test]
    fn renders_with_filters_and_whitespace_control() {
        let c = catalog(&[("a.txt.j2", "{% for k in keys %}\n{{ k|quote('double') }}\n{% endfor %}\n")]).unwrap();
        assert_eq!(c.render("a.txt.j2", context! { keys => vec!["x", "y"] }).unwrap(), "\"x\"\n\"y\"\n");
    }

    #[test]
    fn escapes_markup_only() {
        let c = catalog(&[("p.html.j2", "{{ v }}"), ("p.env.j2", "{{ v }}")]).unwrap();
        assert_eq!(c.render("p.html.j2", context! { v => "<b>" }).unwrap(), "&lt;b&gt;");
        assert_eq!(c.render("p.html.j2", context! { v => "https://a/b?x=1&y=2" }).unwrap(), "https://a/b?x=1&amp;y=2");
        assert_eq!(c.render("p.env.j2", context! { v => "<b>" }).unwrap(), "<b>");
    }

    #[test]
    fn undefined_variables_are_errors() {
        let c = catalog(&[("u.j2", "{{ missing }}")]).unwrap();
        assert!(c.render("u.j2", context! {}).is_err());
    }

    #[test]
    fn syntax_errors_fail_at_load() {
        let err = catalog(&[("bad.j2", "{% for x in %}")]).err().expect("must fail");
        assert!(format!("{err:#}").contains("bad.j2"));
    }

    #[test]
    fn data_files_deserialize() {
        let mut s = Sources::default();
        s.insert("d.yaml", "a: 1\nb: [x, y]\n");
        let c = Catalog::from_sources(s).unwrap();
        #[derive(serde::Deserialize)]
        struct D {
            a: u32,
            b: Vec<String>,
        }
        let d: D = c.data("d.yaml").unwrap();
        assert_eq!((d.a, d.b.len()), (1, 2));
        assert!(c.data::<D>("nope.yaml").is_err());
    }

    #[test]
    fn crlf_conversion_is_idempotent() {
        assert_eq!(to_crlf("a\nb\r\n"), "a\r\nb\r\n");
    }

    #[test]
    fn embedded_catalog_compiles() {
        let c = Catalog::load(None).unwrap();
        assert!(!c.template_names().is_empty());
    }
}
