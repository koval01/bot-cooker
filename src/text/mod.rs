//! Procedural prose: an order-2 word Markov chain trained on the corpus in
//! `templates/data/corpus/`. Each persona trains on its own subset of the
//! corpus, so two instances neither share sentences verbatim nor the same
//! transition statistics. Slots such as `{topic}` survive generation and are
//! filled by the caller with the persona's own words.

use std::collections::{HashMap, HashSet};

use rand::{Rng, RngExt};

/// Sentence-level order-2 Markov model.
#[derive(Debug, Default)]
pub struct TextModel {
    starts: Vec<(String, String)>,
    next: HashMap<(String, String), Vec<String>>,
    /// Training sentences, so generation can avoid quoting them verbatim.
    seen: HashSet<String>,
}

/// How hard `sentence` tries to produce something not in the corpus.
const ATTEMPTS: usize = 12;

const END: &str = "\u{0}";

impl TextModel {
    /// Train on sentences; each is split on whitespace.
    pub fn train<'a>(sentences: impl IntoIterator<Item = &'a str>) -> Self {
        let mut m = TextModel::default();
        for s in sentences {
            let words: Vec<&str> = s.split_whitespace().collect();
            if words.len() < 3 {
                continue;
            }
            m.seen.insert(words.join(" "));
            m.starts.push((words[0].to_string(), words[1].to_string()));
            for w in words.windows(3) {
                m.next.entry((w[0].to_string(), w[1].to_string())).or_default().push(w[2].to_string());
            }
            let n = words.len();
            m.next.entry((words[n - 2].to_string(), words[n - 1].to_string())).or_default().push(END.to_string());
        }
        m
    }

    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    /// One sentence of at most `max_words` words that, if at all possible,
    /// is not a verbatim corpus sentence (those would be shared by every
    /// instance trained on it).
    pub fn sentence(&self, rng: &mut impl Rng, max_words: usize) -> String {
        let mut last = String::new();
        for _ in 0..ATTEMPTS {
            last = self.walk(rng, max_words);
            if !self.seen.contains(last.as_str()) {
                break;
            }
        }
        last
    }

    fn walk(&self, rng: &mut impl Rng, max_words: usize) -> String {
        if self.starts.is_empty() {
            return String::new();
        }
        let (a, b) = &self.starts[rng.random_range(0..self.starts.len())];
        let mut out = vec![a.clone(), b.clone()];
        while out.len() < max_words {
            let key = (out[out.len() - 2].clone(), out[out.len() - 1].clone());
            let Some(options) = self.next.get(&key) else { break };
            let w = &options[rng.random_range(0..options.len())];
            if w == END {
                break;
            }
            out.push(w.clone());
        }
        let mut s = out.join(" ");
        if !s.ends_with(['.', '!', '?']) {
            s = s.trim_end_matches([',', ';', ':']).to_string();
            s.push('.');
        }
        s
    }

    pub fn paragraph(&self, rng: &mut impl Rng, sentences: usize) -> String {
        (0..sentences).map(|_| self.sentence(rng, 40)).collect::<Vec<_>>().join(" ")
    }
}

/// Split corpus text into sentences (one per line or per terminal stop).
pub fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut start = 0;
        for (i, c) in line.char_indices() {
            if matches!(c, '.' | '!' | '?') && line[i + 1..].starts_with(' ') {
                out.push(line[start..=i].trim());
                start = i + 1;
            }
        }
        let rest = line[start..].trim();
        if !rest.is_empty() {
            out.push(rest);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::keyed_rng;

    const TEXT: &str = "The team shipped the new report today. The team fixed the export. Our customers asked for faster exports. The export is faster now!";

    #[test]
    fn splits_sentences() {
        assert_eq!(sentences(TEXT).len(), 4);
    }

    #[test]
    fn generates_terminated_sentences_from_the_corpus_vocabulary() {
        let m = TextModel::train(sentences(TEXT));
        let mut rng = keyed_rng(b"t", &[]);
        let vocab: Vec<&str> = TEXT.split_whitespace().collect();
        for _ in 0..50 {
            let s = m.sentence(&mut rng, 30);
            assert!(s.ends_with(['.', '!', '?']), "{s}");
            assert!(s.split_whitespace().all(|w| vocab.contains(&w) || vocab.contains(&w.trim_end_matches('.'))), "{s}");
        }
    }

    #[test]
    fn avoids_quoting_the_corpus_when_it_can() {
        const BRANCHY: &str = "We fixed the export and the import today. We fixed the report and the dashboard yesterday. The export and the dashboard are faster now.";
        let m = TextModel::train(sentences(BRANCHY));
        let mut rng = keyed_rng(b"q", &[]);
        let quoted = (0..200).filter(|_| m.seen.contains(m.sentence(&mut rng, 30).as_str())).count();
        let raw = (0..200).filter(|_| m.seen.contains(m.walk(&mut rng, 30).as_str())).count();
        assert!(quoted < raw, "retries must reduce verbatim output ({quoted} vs {raw})");
    }

    #[test]
    fn empty_model_is_harmless() {
        let m = TextModel::train(std::iter::empty());
        assert!(m.is_empty());
        assert_eq!(m.sentence(&mut keyed_rng(b"t", &[]), 10), "");
    }
}

/// A persona's word choices: one preferred variant per synonym group, with
/// an occasional switch to another variant.
#[derive(Debug, Default)]
pub struct Voice {
    choice: HashMap<String, (String, Vec<String>)>,
    /// Sentence openers this voice likes.
    markers: Vec<String>,
    marker_rate: f64,
}

impl Voice {
    pub fn with_markers(mut self, all: &[String], rng: &mut impl Rng) -> Self {
        let mut pool: Vec<&String> = all.iter().collect();
        for i in (1..pool.len()).rev() {
            pool.swap(i, rng.random_range(0..=i));
        }
        self.markers = pool.into_iter().take(rng.random_range(2..6)).cloned().collect();
        self.marker_rate = rng.random_range(0.08..0.3);
        self
    }

    pub fn new(groups: &[Vec<String>], rng: &mut impl Rng) -> Self {
        let mut choice = HashMap::new();
        for g in groups.iter().filter(|g| !g.is_empty()) {
            let preferred = g[rng.random_range(0..g.len())].clone();
            for w in g {
                choice.insert(w.clone(), (preferred.clone(), g.clone()));
            }
        }
        Voice { choice, markers: Vec::new(), marker_rate: 0.0 }
    }

    /// Rewrite a sentence in this voice; `wander` is the chance to pick a
    /// non-preferred variant for a word.
    pub fn speak(&self, sentence: &str, rng: &mut impl Rng, wander: f64) -> String {
        let mut out: Vec<String> = Vec::new();
        for token in sentence.split(' ') {
            let core = token.trim_end_matches(['.', ',', '!', '?', ';', ':']);
            let punct = &token[core.len()..];
            let lower = core.to_ascii_lowercase();
            let replaced = match self.choice.get(&lower) {
                Some((preferred, group)) => {
                    let w = if rng.random_bool(wander) { &group[rng.random_range(0..group.len())] } else { preferred };
                    if core.starts_with(|c: char| c.is_ascii_uppercase()) { crate::persona::capitalize(w) } else { w.clone() }
                }
                None => core.to_string(),
            };
            out.push(format!("{replaced}{punct}"));
        }
        let text = out.join(" ");
        if !self.markers.is_empty() && rng.random_bool(self.marker_rate) {
            let m = &self.markers[rng.random_range(0..self.markers.len())];
            let mut rest = text.chars();
            let first = rest.next().map(|c| c.to_lowercase().collect::<String>()).unwrap_or_default();
            return format!("{m}, {first}{}", rest.as_str());
        }
        text
    }
}

#[cfg(test)]
mod voice_tests {
    use super::*;
    use crate::util::keyed_rng;

    #[test]
    fn voices_differ_but_are_consistent() {
        let groups = vec![vec!["team".to_string(), "staff".into(), "crew".into()], vec!["customers".into(), "clients".into()]];
        let voices: Vec<Voice> = (0..6).map(|i| Voice::new(&groups, &mut keyed_rng(b"v", &[&[i]]))).collect();
        let mut rng = keyed_rng(b"s", &[]);
        let outputs: HashSet<String> = voices.iter().map(|v| v.speak("Our team, our customers.", &mut rng, 0.0)).collect();
        assert!(outputs.len() > 1);
        let v = &voices[0];
        assert_eq!(v.speak("Team.", &mut rng, 0.0), v.speak("Team.", &mut rng, 0.0));
        assert!(v.speak("Team.", &mut rng, 0.0).starts_with(char::is_uppercase));
    }
}
