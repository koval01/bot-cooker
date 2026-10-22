//! Offline content packs: per-instance text written by Claude on the owner's
//! machine, saved as a `templates_dir` overlay. The trap itself never calls
//! out; this module is compiled only with `--features pack`.
//!
//! A pack replaces what every instance would otherwise share: the prose
//! corpus, the comment and commit vocabularies, and (for non-CMS stacks) the
//! site theme the persona uses. Only the fictional persona goes into the
//! prompts; the master key never leaves the machine.

mod client;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, bail};
use http::Method;
use minijinja::context;
use rand::RngExt;
use serde_json::{Value, json};

pub use client::Client;

use crate::catalog::{Catalog, Sources};
use crate::config::{Config, Profile};
use crate::data::Data;
use crate::engine::{Engine, RequestInfo};
use crate::persona::Persona;
use crate::render::{Req, Scope, ser};
use crate::world::World;

pub struct Options {
    pub out: PathBuf,
    pub paragraphs: u32,
    pub theme: bool,
    /// Print the prompts instead of calling the API.
    pub dry_run: bool,
}

const CORPUS_KEYS: [&str; 6] = ["blog", "docs", "company", "support", "updates", "notes"];
const SLOTS: [&str; 5] = ["{topic}", "{topic2}", "{resource}", "{company}", "{product}"];

pub fn run(cfg: &Config, client: Option<&Client>, opts: &Options) -> anyhow::Result<()> {
    if cfg.deception.master_key.is_empty() {
        bail!("set deception.master_key (or COOKER_MASTER_KEY): a pack is made for one persona");
    }
    let catalog = Arc::new(Catalog::load(None)?);
    let data = Data::load(&catalog)?;
    let engine = Engine::new(&cfg.deception, catalog.clone(), data, cfg.routes.clone());
    let inc = engine.current();
    let p = &inc.persona;
    println!("persona: {}", p.summary());

    let prompt_ctx = || context! { persona => ser(p), site => ser(&p.site), paragraphs => opts.paragraphs };
    let system = catalog.render("pack/system.j2", prompt_ctx())?;
    let corpus_prompt = catalog.render("pack/corpus.j2", prompt_ctx())?;
    let vocab_prompt = catalog.render("pack/vocab.j2", prompt_ctx())?;
    if opts.dry_run {
        println!("--- system\n{system}\n--- corpus\n{corpus_prompt}\n--- vocab\n{vocab_prompt}");
        return Ok(());
    }
    let client = client.context("no API client")?;
    std::fs::create_dir_all(opts.out.join("data/corpus"))?;

    println!("writing the corpus ({} paragraphs x {})...", opts.paragraphs, CORPUS_KEYS.len());
    let corpus = client.structured(&system, &corpus_prompt, &string_lists(&CORPUS_KEYS), 32_000)?;
    for key in CORPUS_KEYS {
        let text: Vec<String> = strings(&corpus[key]).iter().map(|s| clean_prose(s)).filter(|s| !s.is_empty()).collect();
        if text.len() < 3 {
            bail!("the corpus section {key} came back nearly empty");
        }
        std::fs::write(opts.out.join(format!("data/corpus/{key}.txt")), text.join("\n") + "\n")?;
    }

    println!("writing comment and commit vocabularies...");
    let keys = ["todo", "fixme", "aside", "commit_verbs", "commit_objects", "commit_tails", "markers"];
    let vocab = client.structured(&system, &vocab_prompt, &string_lists(&keys), 8_000)?;
    write_vocab(&catalog, &opts.out, &vocab)?;

    if opts.theme && !p.cms {
        println!("writing the site theme...");
        write_theme(&catalog, client, &system, p, &engine, &opts.out)?;
    }

    verify(cfg, &opts.out)?;
    std::fs::write(opts.out.join("PACK.md"), pack_readme(p, client))?;
    println!("done: set deception.templates_dir to {}", opts.out.display());
    Ok(())
}

/// JSON schema: an object of string arrays.
fn string_lists(keys: &[&str]) -> Value {
    let props: serde_json::Map<String, Value> =
        keys.iter().map(|k| (k.to_string(), json!({"type": "array", "items": {"type": "string"}}))).collect();
    json!({"type": "object", "properties": props, "required": keys, "additionalProperties": false})
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect()
}

/// One paragraph per line, only the known slots may keep braces.
fn clean_prose(s: &str) -> String {
    let mut out = s.split_whitespace().collect::<Vec<_>>().join(" ");
    for (i, slot) in SLOTS.iter().enumerate() {
        out = out.replace(slot, &format!("\u{1}{i}\u{1}"));
    }
    out = out.replace(['{', '}'], "");
    for (i, slot) in SLOTS.iter().enumerate() {
        out = out.replace(&format!("\u{1}{i}\u{1}"), slot);
    }
    out
}

fn short(s: &str) -> Option<String> {
    let s = s.trim().trim_matches(['"', '\'', ',']).to_string();
    (s.len() <= 60 && s.chars().all(|c| c.is_ascii() && c != '"')).then_some(s)
}

/// Overlay the embedded vocab files with the generated lists.
fn write_vocab(catalog: &Catalog, out: &Path, vocab: &Value) -> anyhow::Result<()> {
    let list = |key: &str| -> Vec<serde_norway::Value> {
        strings(&vocab[key]).iter().filter_map(|s| short(s)).map(serde_norway::Value::String).collect()
    };
    let edit = |file: &str, f: &dyn Fn(&mut serde_norway::Mapping)| -> anyhow::Result<()> {
        let mut doc: serde_norway::Value = serde_norway::from_str(&catalog.source(file).context("embedded file")?)?;
        f(doc.as_mapping_mut().context("mapping")?);
        std::fs::write(out.join(file), serde_norway::to_string(&doc)?)?;
        Ok(())
    };
    let set = |m: &mut serde_norway::Mapping, key: &str, items: Vec<serde_norway::Value>| {
        if items.len() >= 3 {
            m.insert(key.into(), serde_norway::Value::Sequence(items));
        }
    };
    edit("data/words.yaml", &|m| {
        set(m, "commit_verbs", list("commit_verbs"));
        set(m, "commit_objects", list("commit_objects"));
        set(m, "commit_tails", list("commit_tails"));
    })?;
    edit("data/notes.yaml", &|m| {
        if let Some(parts) = m.get_mut("parts").and_then(|p| p.as_mapping_mut()) {
            for key in ["todo", "fixme", "aside"] {
                set(parts, key, list(key));
            }
        }
    })?;
    edit("data/synonyms.yaml", &|m| set(m, "markers", list("markers")))?;
    Ok(())
}

/// The theme this persona picks (the pack overwrites exactly that one).
fn chosen_theme(catalog: &Catalog, persona: &Persona) -> Option<String> {
    let mut themes: Vec<String> = catalog.template_names().into_iter().filter(|n| n.starts_with("site/themes/")).collect();
    themes.sort();
    let i = persona.rng("theme").random_range(0..themes.len().max(1));
    themes.get(i).cloned()
}

fn write_theme(catalog: &Catalog, client: &Client, system: &str, p: &Persona, engine: &Engine, out: &Path) -> anyhow::Result<()> {
    let name = chosen_theme(catalog, p).context("no themes in the catalog")?;
    let schema =
        json!({"type": "object", "properties": {"template": {"type": "string"}}, "required": ["template"], "additionalProperties": false});
    let mut error: Option<String> = None;
    for attempt in 1..=3 {
        let prompt = catalog.render("pack/theme.j2", context! { persona => ser(p), error => error.clone() })?;
        let template = client.structured(system, &prompt, &schema, 16_000)?["template"].as_str().unwrap_or_default().to_string();
        match check_theme(&name, &template, p, engine) {
            Ok(()) => {
                let path = out.join(&name);
                std::fs::create_dir_all(path.parent().context("theme path")?)?;
                std::fs::write(path, template)?;
                return Ok(());
            }
            Err(e) => {
                eprintln!("  theme attempt {attempt} failed: {e:#}");
                error = Some(format!("{e:#}"));
            }
        }
    }
    bail!("could not get a working theme; the pack keeps the built-in one")
}

/// Compile the candidate in a catalog of its own and render a sample page.
fn check_theme(name: &str, template: &str, p: &Persona, engine: &Engine) -> anyhow::Result<()> {
    if !template.contains("site/_head.html.j2") {
        bail!("the template must include site/_head.html.j2 in <head>");
    }
    let mut sources = Sources::embedded();
    sources.insert(name, template);
    let catalog = Catalog::from_sources(sources)?;
    let req = Req { method: "GET".into(), host: p.domain.clone(), scheme: "https", path: "/".into(), query: None };
    let scope = Scope::new(World::new(p, engine.epochs()), req, &catalog);
    let sample = context! {
        home => true,
        heading => p.company.clone(),
        paragraphs => vec!["First paragraph.", "Second paragraph."],
        links => vec![context! { href => "/a/", title => "A", time => engine.epochs().cfg_anchor }],
        nav => vec![context! { href => "/docs/", label => "Docs" }],
        tagline => "",
    };
    let html = scope.render(name, sample)?;
    if !html.contains("First paragraph.") || !html.contains("/a/") {
        bail!("the template does not render the paragraphs and links");
    }
    Ok(())
}

/// Load the pack as the trap would and render the main surfaces.
fn verify(cfg: &Config, out: &Path) -> anyhow::Result<()> {
    let catalog = Arc::new(Catalog::load(Some(out)).context("the pack does not load")?);
    let data = Data::load(&catalog).context("the pack's data does not load")?;
    let engine = Engine::new(&cfg.deception, catalog, data, cfg.routes.clone());
    let inc = engine.current();
    for (path, profile) in [("/", Profile::Labyrinth), ("/robots.txt", Profile::Labyrinth)] {
        let info = RequestInfo { method: &Method::GET, path, query: None, host: None, https: true };
        let art = if path == "/robots.txt" { engine.robots(&inc, &info) } else { engine.render(&inc, profile, &info) };
        if art.head.is_empty() {
            bail!("{path} renders empty with the pack");
        }
    }
    Ok(())
}

fn pack_readme(p: &Persona, client: &Client) -> String {
    format!(
        "# Content pack\n\nGenerated for persona: {}\nModel: {} (effort {})\n\nPoint `deception.templates_dir` at this directory. \
         Files here replace the built-in ones with the same path; delete one to fall back.\n\n\
         - data/corpus/*.txt: prose the site text is generated from\n\
         - data/words.yaml, data/notes.yaml, data/synonyms.yaml: commit, comment and voice vocabularies\n\
         - site/themes/*.html.j2: the site theme this persona uses (if generated)\n",
        p.summary(),
        client.model,
        client.effort
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prose_keeps_only_known_slots() {
        assert_eq!(clean_prose("Our {topic}  team {evil} and {resource}\n."), "Our {topic} team evil and {resource} .");
    }

    #[test]
    fn schema_lists_every_key() {
        let s = string_lists(&["a", "b"]);
        assert_eq!(s["required"], json!(["a", "b"]));
        assert_eq!(s["additionalProperties"], json!(false));
    }

    #[test]
    fn vocab_items_are_short_and_plain() {
        assert_eq!(short("  fix the importer, "), Some("fix the importer".into()));
        assert_eq!(short(&"x".repeat(80)), None);
    }

    #[test]
    fn a_valid_theme_passes_and_a_broken_one_fails() {
        let catalog = Arc::new(Catalog::load(None).unwrap());
        let cfg = crate::config::DeceptionConfig { master_key: "pack-test".into(), ..Default::default() };
        let engine = Engine::new(&cfg, catalog, Data::embedded(), None);
        let inc = engine.current();
        let good = "<!DOCTYPE html><html><head>{% include \"site/_head.html.j2\" %}</head><body>{% for p in paragraphs %}<p>{{ p }}</p>{% endfor %}{% for l in links %}<a href=\"{{ l.href }}\">{{ l.title }}</a>{% endfor %}</body></html>";
        assert!(check_theme("site/themes/x.html.j2", good, &inc.persona, &engine).is_ok());
        let broken = "<!DOCTYPE html><head>{% include \"site/_head.html.j2\" %}</head>{{ nope }}";
        assert!(check_theme("site/themes/x.html.j2", broken, &inc.persona, &engine).is_err());
    }
}
