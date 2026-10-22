//! PHP's own error output: what a persona with `display_errors` on prints
//! into its responses (`templates/php/display.j2`), and what its PHP error
//! log holds. The faults are the persona's (`Persona::php_errors`); which of
//! them fire changes with the config epoch, their wording follows the
//! persona's PHP release and their lines its application release.

use minijinja::context;
use rand::RngExt;
use serde::Serialize;

use super::doc::{Doc, Origin};
use super::scope::{Scope, ser};
use crate::config::Profile;
use crate::persona::fill::{Vars, fill};
use crate::persona::{PhpDisplay, PhpErrors, PhpFault, PhpLevel, PhpPlace};

/// What PHP buffers before the first flush (php.ini `output_buffering`):
/// output beyond it would send the headers early and make every later
/// `header()` call warn, so a booting script stays below it.
const OUTPUT_BUFFER: usize = 4096;

/// One error as PHP reports it.
#[derive(Debug, Serialize)]
pub struct Shown {
    pub label: String,
    pub message: String,
    pub file: String,
    pub line: u32,
    #[serde(skip)]
    pub level: PhpLevel,
}

/// A response from a script that loads the application. What PHP printed
/// while it booted goes first; a content page may instead die of memory
/// exhaustion, which discards everything buffered before the fatal error.
pub fn booted(scope: &Scope, profile: Profile, doc: Doc) -> Doc {
    let Some(errors) = displayed(scope) else { return doc };
    let boots = match profile {
        Profile::Blank => scope.world.data().php_errors.stacks.get(&scope.persona().stack).is_some_and(|s| s.blank_boots),
        // phpinfo is a script of its own; error pages of the application
        // add theirs (`errors::app_error`), php-fpm's own answers have none.
        // API responses stay clean: clients parse them.
        Profile::PhpInfo | Profile::NotFound | Profile::Forbidden | Profile::Json => false,
        _ => matches!(doc.origin, Origin::App { .. }),
    };
    if !boots {
        return doc;
    }
    if profile == Profile::Labyrinth
        && let Some(fatal) = fatal(scope, errors)
    {
        let mut doc = doc;
        doc.head = fatal.into_bytes();
        doc.stream = None;
        doc.tail.clear();
        return doc;
    }
    prepend(scope, doc)
}

/// Put what PHP printed while the application booted before `doc`.
pub fn prepend(scope: &Scope, mut doc: Doc) -> Doc {
    let top = output(scope, PhpPlace::Top);
    doc.head.splice(0..0, top.into_bytes());
    doc
}

/// What PHP prints at `place` on this request (empty for most personas).
pub fn output(scope: &Scope, place: PhpPlace) -> String {
    let Some(errors) = displayed(scope) else { return String::new() };
    let mut shown: Vec<Shown> =
        firing(scope, errors).into_iter().filter(|(f, s)| f.place == place && errors.levels.contains(&s.level)).map(|(_, s)| s).collect();
    loop {
        let out = render(scope, &shown);
        if out.len() < OUTPUT_BUFFER - 256 || shown.is_empty() {
            return out;
        }
        shown.pop();
    }
}

/// Everything the persona's code raises this epoch, for the PHP error log
/// (logged whether or not it is displayed).
pub fn logged(scope: &Scope) -> Vec<Shown> {
    scope.persona().php_errors.as_ref().map(|e| firing(scope, e).into_iter().map(|(_, s)| s).collect()).unwrap_or_default()
}

fn displayed<'a>(scope: &Scope<'a>) -> Option<&'a PhpErrors> {
    scope.persona().php_errors.as_ref().filter(|e| e.display != PhpDisplay::Off)
}

/// The faults that fire during the current config epoch: the same on every
/// request, as the same code runs on every request.
fn firing<'f>(scope: &Scope, errors: &'f PhpErrors) -> Vec<(&'f PhpFault, Shown)> {
    let mut rng = scope.world.cfg_rng("php-errors", "");
    let fires: Vec<bool> = errors.faults.iter().map(|_| rng.random_bool(0.7)).collect();
    let any = fires.iter().any(|f| *f);
    errors
        .faults
        .iter()
        .zip(fires)
        .enumerate()
        .filter(|(i, (_, fires))| *fires || (!any && *i == 0))
        .filter_map(|(_, (f, _))| resolve(scope, f).map(|s| (f, s)))
        .collect()
}

/// A fault as this persona's PHP and application release report it; `None`
/// when the release does not raise it.
fn resolve(scope: &Scope, fault: &PhpFault) -> Option<Shown> {
    let p = scope.persona();
    let data = &scope.world.data().php_errors;
    let message = data.message(fault.kind, (p.versions.runtime.major, p.versions.runtime.minor))?;
    let line = match fault.line {
        Some(line) => line,
        None => {
            let core = data.stacks.get(&p.stack)?.core.as_ref()?;
            let release = crate::data::php_series(&p.versions.framework.at(scope.world.epochs.cfg_anchor));
            let since = crate::data::php_series(core.kinds.get(fault.kind)?);
            if release < since {
                return None;
            }
            *core.lines.get(&format!("{}.{}", release.0, release.1))?
        }
    };
    let vars = fault.slots.iter().fold(Vars::default(), |v, (k, val)| v.with(k, val.clone()));
    let root = scope.docroot();
    let file = if root.contains('\\') { format!("{root}\\{}", fault.file.replace('/', "\\")) } else { format!("{root}/{}", fault.file) };
    Some(Shown {
        label: data.labels.get(&message.level)?.clone(),
        message: fill(&message.text, &vars, &mut scope.world.fixed_rng("php-fill")),
        file,
        line,
        level: message.level,
    })
}

/// The fatal error a content page dies of, on the share of pages that do.
fn fatal(scope: &Scope, errors: &PhpErrors) -> Option<String> {
    let fault = errors.fatal.as_ref()?;
    let home = scope.req.path.trim_end_matches('/').is_empty();
    if home
        || !errors.levels.contains(&PhpLevel::Error)
        || !scope.world.cfg_rng("php-fatal", &scope.req.path).random_bool(errors.fatal_share)
    {
        return None;
    }
    Some(render(scope, &[resolve(scope, fault)?]))
}

fn render(scope: &Scope, shown: &[Shown]) -> String {
    if shown.is_empty() {
        return String::new();
    }
    scope.render_or_empty("php/display.j2", context! { errors => ser(&shown) })
}
