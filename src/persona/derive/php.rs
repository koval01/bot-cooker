//! How the persona's PHP application reports errors (mostly: it doesn't)
//! and the few places in its code that misbehave.

use rand::RngExt;
use rand_chacha::ChaCha8Rng;

use super::tech::Tech;
use crate::data::{Data, PhpErrorData, PhpStack};
use crate::persona::model::{PhpErrors, PhpFault, PhpPlace};
use crate::util::{chance, pick, pick_str, weighted};

/// Messages a misbehaving line of plugin or extension code can raise;
/// those needing an object only happen in files that define a class.
const KINDS: &[&str] =
    &["undefined_key", "undefined_variable", "offset_on_null", "property_on_null", "foreach_on_null", "null_to_internal"];
const OBJECT_KINDS: &[&str] = &["dynamic_property"];

pub fn derive(rng: &mut ChaCha8Rng, data: &'static Data, tech: &Tech) -> Option<PhpErrors> {
    let all = &data.php_errors;
    let facts = all.stacks.get(&tech.stack)?;
    if facts.framework_major.is_some_and(|major| major != tech.versions.framework.major) {
        return None;
    }
    let modes: Vec<(usize, u32)> = facts.modes.iter().enumerate().map(|(i, m)| (i, m.weight)).collect();
    let mode = &facts.modes[weighted(rng, &modes)];
    let php = (tech.versions.runtime.major, tech.versions.runtime.minor);

    let mut faults: Vec<PhpFault> = Vec::new();
    let wanted = rng.random_range(2..6);
    for _ in 0..wanted * 4 {
        if faults.len() >= wanted {
            break;
        }
        let f = fault(rng, all, facts, php);
        if !faults.iter().any(|g| g.file == f.file && (g.line == f.line || g.kind == f.kind)) {
            faults.push(f);
        }
    }
    faults.extend(core(rng, facts));

    let memory_limit = pick(rng, &[128u32, 128, 128, 256, 256, 512]);
    let fatal = (!facts.fatal.is_empty()).then(|| {
        let at = &facts.fatal[rng.random_range(0..facts.fatal.len())];
        let size = pick(rng, &all.allocations);
        PhpFault {
            kind: "memory_exhausted",
            file: at.file.as_str(),
            line: Some(line(rng, at.lines)),
            place: PhpPlace::Top,
            slots: vec![("limit", (u64::from(memory_limit) << 20).to_string()), ("size", size.to_string())],
        }
    });
    Some(PhpErrors {
        setting: mode.setting.as_str(),
        display: mode.display,
        levels: mode.levels.clone(),
        memory_limit,
        faults,
        fatal,
        fatal_share: facts.fatal_share,
    })
}

/// A plugin or extension line that raises one of the engine's own messages.
fn fault(rng: &mut ChaCha8Rng, all: &'static PhpErrorData, facts: &'static PhpStack, php: (u32, u32)) -> PhpFault {
    let source = &facts.sources[rng.random_range(0..facts.sources.len())];
    let kinds: Vec<&'static str> = KINDS
        .iter()
        .chain(if source.class.is_some() { OBJECT_KINDS } else { &[] })
        .copied()
        .filter(|k| all.message(k, php).is_some())
        .collect();
    let kind = pick(rng, &kinds);
    let word = |rng: &mut ChaCha8Rng, slot: &str| pick_str(rng, &facts.words[slot]).to_string();
    let slots = match kind {
        "undefined_key" => vec![("key", word(rng, "key"))],
        "undefined_variable" => vec![("var", word(rng, "var"))],
        "property_on_null" => vec![("prop", word(rng, "prop"))],
        "null_to_internal" => {
            let arg = &all.null_args[rng.random_range(0..all.null_args.len())];
            vec![("fn", arg.function.clone()), ("n", arg.n.to_string()), ("param", arg.param.clone()), ("type", arg.kind.clone())]
        }
        "dynamic_property" => vec![("class", source.class.clone().unwrap_or_default()), ("prop", word(rng, "var"))],
        _ => Vec::new(),
    };
    PhpFault { kind, file: source.file.as_str(), line: Some(line(rng, source.lines)), place: source.place, slots }
}

/// Notices the application raises itself (WordPress's `_doing_it_wrong`
/// and deprecated functions): a plugin of the persona's triggers them.
fn core(rng: &mut ChaCha8Rng, facts: &'static PhpStack) -> Vec<PhpFault> {
    let Some(core) = &facts.core else { return Vec::new() };
    let mut out = Vec::new();
    let domains: Vec<&str> = facts.sources.iter().filter_map(|s| s.domain.as_deref()).collect();
    for kind in core.kinds.keys() {
        if !chance(rng, 0.5) {
            continue;
        }
        let slots = match kind.as_str() {
            "wp_textdomain" if !domains.is_empty() => vec![("domain", pick(rng, &domains).to_string())],
            "wp_deprecated_function" => {
                let d = &facts.deprecated[rng.random_range(0..facts.deprecated.len())];
                vec![("fn", d.function.clone()), ("since", d.since.clone()), ("use", d.replacement.clone())]
            }
            _ => continue,
        };
        out.push(PhpFault { kind: kind.as_str(), file: core.file.as_str(), line: None, place: PhpPlace::Top, slots });
    }
    out
}

/// A line somewhere in the body of a file `len` lines long.
fn line(rng: &mut ChaCha8Rng, len: u32) -> u32 {
    rng.random_range(len.min(30)..len.saturating_sub(5).max(len.min(30) + 1))
}
