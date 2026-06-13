// SPDX-License-Identifier: Apache-2.0

//! Compact French frequency model — the always-loaded fallback predictor
//! (SPEC §2.C « le clavier parle toujours », D-2.6).
//!
//! When the LLM worker is down, the hub still predicts from this embedded
//! model; the evaluation harness measures **this very code** as the mandatory
//! n-gram baseline (SPEC §8.A, ADR-0006) — never a parallel reimplementation.
//!
//! v0 is a word-frequency (unigram) model: completions of a prefix ranked by
//! frequency, plus a next-character distribution for adaptive scanning (the
//! `next-chars` contract). It trains from text (the synthetic corpus, free
//! frequency lists) and (de)serialises as compact JSON. A hand-curated French
//! base vocabulary ships now ([`NgramModel::french_base`], the hub's always-on
//! fallback); bigram context and the teacher-corpus expansion (#18) are later
//! refinements the prediction-source API will keep.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The embedded French base vocabulary, most-frequent first
/// (`data/french_base_words.txt`); [`NgramModel::french_base`] builds from it.
const FRENCH_BASE_WORDS: &str = include_str!("../data/french_base_words.txt");

/// A word-frequency model. Iteration order is deterministic (sorted keys), so
/// predictions and serialisation are reproducible.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NgramModel {
    counts: BTreeMap<String, u64>,
    total: u64,
}

/// A ranked completion of a prefix.
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    /// The full candidate word.
    pub word: String,
    /// Relative frequency in `[0, 1]` — comparable within one model only.
    pub score: f64,
}

/// Relative frequency as a float. Word counts are tiny next to `2^53`, so the
/// `u64`→`f64` cast is lossless in practice.
#[allow(clippy::cast_precision_loss)]
fn relative(count: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        count as f64 / total as f64
    }
}

impl NgramModel {
    /// An empty model.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The always-loaded French base model (D-2.6 « le clavier parle toujours »):
    /// what the hub predicts from whenever the LLM worker is down, so the
    /// keyboard keeps offering completions instead of nothing.
    ///
    /// Built from an embedded, hand-curated frequency list — a v0 base the
    /// teacher corpus (#18) later expands. Cheap enough to build once at boot.
    #[must_use]
    pub fn french_base() -> Self {
        Self::from_ranked(FRENCH_BASE_WORDS.lines())
    }

    /// Builds a model from words listed **most-frequent first**, weighting each
    /// by its rank (linear, strictly decreasing) so completions rank by
    /// frequency rather than alphabetically. Blank lines and `#` comments are
    /// skipped and words are lowercased; a repeated word keeps its first
    /// (highest) rank.
    #[must_use]
    pub fn from_ranked<'a>(words: impl IntoIterator<Item = &'a str>) -> Self {
        let ranked: Vec<String> = words
            .into_iter()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_lowercase)
            .collect();
        let mut model = Self::new();
        for (index, word) in ranked.iter().enumerate() {
            if model.counts.contains_key(word) {
                continue; // keep the first (highest) rank of a repeated word
            }
            let count = (ranked.len() - index) as u64;
            model.add_word(word, count);
        }
        model
    }

    /// Adds `count` occurrences of an already-tokenised, lowercased `word`.
    fn add_word(&mut self, word: &str, count: u64) {
        *self.counts.entry(word.to_owned()).or_insert(0) += count;
        self.total += count;
    }

    /// Counts the words of `text` into the model (whitespace-split, lowercased,
    /// surrounding punctuation stripped; internal `'` and `-` kept so `j'ai`
    /// and `rendez-vous` stay whole).
    pub fn train(&mut self, text: &str) {
        for token in tokens(text) {
            *self.counts.entry(token).or_insert(0) += 1;
            self.total += 1;
        }
    }

    /// Number of distinct words known.
    #[must_use]
    pub fn vocabulary_size(&self) -> usize {
        self.counts.len()
    }

    /// Whether the model has been trained on anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// The `n` most frequent words beginning with `prefix`, best first.
    ///
    /// Ties break lexicographically, so the result is deterministic. An empty
    /// prefix returns the globally most frequent words; an `n` of 0 is empty.
    #[must_use]
    pub fn complete(&self, prefix: &str, n: usize) -> Vec<Completion> {
        let prefix = prefix.to_lowercase();
        let mut hits: Vec<(&String, u64)> = self
            .counts
            .iter()
            .filter(|(word, _)| word.starts_with(&prefix))
            .map(|(word, &count)| (word, count))
            .collect();
        hits.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        hits.into_iter()
            .take(n)
            .map(|(word, count)| Completion {
                word: word.clone(),
                score: relative(count, self.total),
            })
            .collect()
    }

    /// Distribution over the character that follows `prefix`, frequency-weighted
    /// across every word beginning with `prefix` (the `next-chars` analog).
    ///
    /// Probabilities sum to 1 and are returned descending (ties break by
    /// character). Empty when no known word extends the prefix.
    #[must_use]
    pub fn next_char_dist(&self, prefix: &str) -> Vec<(char, f64)> {
        let prefix = prefix.to_lowercase();
        let position = prefix.chars().count();
        let mut mass: BTreeMap<char, u64> = BTreeMap::new();
        let mut total = 0u64;
        for (word, &count) in &self.counts {
            if !word.starts_with(&prefix) {
                continue;
            }
            if let Some(next) = word.chars().nth(position) {
                *mass.entry(next).or_insert(0) += count;
                total += count;
            }
        }
        let mut dist: Vec<(char, f64)> = mass
            .into_iter()
            .map(|(ch, count)| (ch, relative(count, total)))
            .collect();
        dist.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });
        dist
    }

    /// Serialises the model to compact JSON (the on-disk fallback artifact).
    ///
    /// # Errors
    ///
    /// [`serde_json::Error`] if serialisation fails (not expected for this
    /// plain map).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Loads a model from JSON produced by [`NgramModel::to_json`].
    ///
    /// # Errors
    ///
    /// [`serde_json::Error`] if the JSON is malformed or mistyped.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Tokenises text the way the model counts it: whitespace-split, lowercased,
/// surrounding punctuation trimmed, internal `'`/`-` preserved.
fn tokens(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split_whitespace().filter_map(|raw| {
        let trimmed = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-');
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_lowercase())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trained() -> NgramModel {
        let mut model = NgramModel::new();
        model.train("je voudrais des pâtes. je voudrais de l'eau fraîche !");
        model
    }

    #[test]
    fn d_10_1_license() {
        assert_eq!(env!("CARGO_PKG_LICENSE"), "Apache-2.0");
    }

    #[test]
    fn train_counts_words_lowercased_and_trimmed() {
        let model = trained();
        assert_eq!(model.vocabulary_size(), 7); // je voudrais des pâtes de l'eau fraîche
        assert!(!model.is_empty());
    }

    #[test]
    fn complete_ranks_by_frequency_then_lexicographically() {
        let model = trained();
        // "je" and "voudrais" each occur twice; a prefix of "v" yields the one
        // word that matches.
        let completions = model.complete("vou", 3);
        assert_eq!(completions.len(), 1);
        assert_eq!(completions[0].word, "voudrais");
        assert!(completions[0].score > 0.0);
    }

    #[test]
    fn complete_saves_keystrokes_on_a_long_word() {
        let mut model = NgramModel::new();
        model.train("anticonstitutionnellement anticonstitutionnellement");
        // Four characters typed, the whole long word offered: a real saving.
        let completions = model.complete("anti", 1);
        assert_eq!(completions[0].word, "anticonstitutionnellement");
    }

    #[test]
    fn complete_respects_the_count_and_is_empty_for_unknown_prefixes() {
        let model = trained();
        assert!(model.complete("zzz", 5).is_empty());
        assert!(model.complete("je", 0).is_empty());
    }

    #[test]
    fn empty_prefix_returns_the_most_frequent_words() {
        let model = trained();
        let top = model.complete("", 2);
        // "je" and "voudrais" tie at 2; lexicographic tie-break puts "je" first.
        assert_eq!(top[0].word, "je");
        assert_eq!(top[1].word, "voudrais");
    }

    #[test]
    fn next_char_distribution_sums_to_one_and_is_ordered() {
        let model = trained();
        let dist = model.next_char_dist("vou");
        assert_eq!(dist, vec![('d', 1.0)]); // only "voudrais" extends "vou"
        let total: f64 = model.next_char_dist("").iter().map(|(_, p)| p).sum();
        assert!((total - 1.0).abs() < 1e-9);
    }

    #[test]
    fn json_round_trips() {
        let model = trained();
        let restored = NgramModel::from_json(&model.to_json().unwrap()).unwrap();
        assert_eq!(restored.vocabulary_size(), model.vocabulary_size());
        assert_eq!(restored.complete("vou", 1), model.complete("vou", 1));
    }

    #[test]
    fn french_base_is_a_useful_nonempty_model() {
        let model = NgramModel::french_base();
        assert!(!model.is_empty(), "the fallback must not be empty (D-2.6)");
        assert!(
            model.vocabulary_size() > 150,
            "the curated base should hold a few hundred words, got {}",
            model.vocabulary_size()
        );
    }

    #[test]
    fn french_base_completes_common_prefixes() {
        let model = NgramModel::french_base();
        // Where the empty model offered nothing, the base completes a common
        // prefix — and a frequent word is among the offers.
        let bon = model.complete("bon", 3);
        assert!(!bon.is_empty(), "the base must complete a common prefix");
        assert!(
            bon.iter().any(|c| c.word == "bonjour"),
            "expected 'bonjour' among completions of 'bon', got {bon:?}"
        );
        // A care word the personas need.
        assert!(
            !model.complete("aid", 1).is_empty(),
            "'aid' should complete"
        );
    }

    #[test]
    fn french_base_next_char_distribution_is_nonempty() {
        // Degraded adaptive dwell still needs a distribution to modulate on.
        let model = NgramModel::french_base();
        let dist = model.next_char_dist("po");
        assert!(
            !dist.is_empty(),
            "a common prefix must yield next characters"
        );
        let total: f64 = dist.iter().map(|(_, p)| p).sum();
        assert!((total - 1.0).abs() < 1e-9, "must sum to 1, got {total}");
    }

    #[test]
    fn from_ranked_weights_earlier_words_higher_and_dedupes() {
        // "alpha" leads, then "beta", then a repeat of "alpha": the repeat is
        // ignored so "alpha" keeps its top weight and ranks first.
        let model = NgramModel::from_ranked(["alpha", "beta", "alpha", "# skip", ""]);
        assert_eq!(model.vocabulary_size(), 2);
        let top = model.complete("", 2);
        assert_eq!(top[0].word, "alpha", "the earliest-ranked word ranks first");
        assert_eq!(top[1].word, "beta");
    }
}
