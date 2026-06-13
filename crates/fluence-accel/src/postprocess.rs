// SPDX-License-Identifier: Apache-2.0

//! Post-processing of raw generations into clean suggestions (SPEC §5.A:
//! dedup, casing, punctuation).
//!
//! Models emit stray wrapping quotes, double spaces and lowercase starts; this
//! normalises them and de-duplicates, producing the contract's [`Suggestion`]
//! list. Scores are rank-based placeholders in v0 (the real backend will carry
//! logprob-derived confidences).

use fluence_protocol::api::suggest::{Suggestion, SuggestionOrigin};

/// Cleans and de-duplicates `raw` generations into ranked suggestions.
///
/// Each entry is trimmed, unwrapped from surrounding quotes, internally
/// whitespace-collapsed and sentence-capitalised; empties are dropped and
/// case-insensitive duplicates removed (first kept). `origin` marks the source
/// (model, n-gram fallback…). Scores decrease with rank.
#[must_use]
pub fn clean_suggestions(raw: &[String], origin: SuggestionOrigin) -> Vec<Suggestion> {
    let mut seen: Vec<String> = Vec::new();
    let mut cleaned: Vec<String> = Vec::new();
    for candidate in raw {
        let Some(text) = clean_one(candidate) else {
            continue;
        };
        let key = text.to_lowercase();
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        cleaned.push(text);
    }

    let total = cleaned.len();
    cleaned
        .into_iter()
        .enumerate()
        .map(|(rank, text)| Suggestion {
            text,
            score: rank_score(rank, total),
            origin: Some(origin),
        })
        .collect()
}

/// Cleans one candidate, returning `None` if nothing meaningful remains.
fn clean_one(raw: &str) -> Option<String> {
    let unwrapped = strip_wrapping_quotes(raw.trim());
    let collapsed = unwrapped.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(capitalize_first(&collapsed))
}

/// Removes one layer of surrounding quotes a model often wraps output in.
fn strip_wrapping_quotes(text: &str) -> &str {
    for (open, close) in [('«', '»'), ('"', '"'), ('\'', '\'')] {
        if let Some(inner) = text.strip_prefix(open).and_then(|t| t.strip_suffix(close)) {
            return inner.trim();
        }
    }
    text
}

/// Upper-cases the first character, leaving the rest untouched.
fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Rank-based placeholder confidence in `(0, 1]`, highest for rank 0.
#[allow(clippy::cast_precision_loss)] // ranks are tiny; precision loss is irrelevant
fn rank_score(rank: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        (total - rank) as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn trims_collapses_and_capitalises() {
        let out = clean_suggestions(
            &texts(&["  je   voudrais  de l'eau "]),
            SuggestionOrigin::Model,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "Je voudrais de l'eau");
    }

    #[test]
    fn strips_wrapping_quotes_the_model_adds() {
        let out = clean_suggestions(
            &texts(&["« Bonjour »", "\"Salut\""]),
            SuggestionOrigin::Model,
        );
        assert_eq!(out[0].text, "Bonjour");
        assert_eq!(out[1].text, "Salut");
    }

    #[test]
    fn drops_empties_and_deduplicates_case_insensitively() {
        let out = clean_suggestions(
            &texts(&["Oui", "", "   ", "oui", "Non"]),
            SuggestionOrigin::Model,
        );
        assert_eq!(
            out.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
            ["Oui", "Non"]
        );
    }

    #[test]
    fn scores_decrease_with_rank_and_origin_is_tagged() {
        let out = clean_suggestions(&texts(&["A", "B", "C"]), SuggestionOrigin::Ngram);
        assert!(out[0].score > out[1].score && out[1].score > out[2].score);
        assert_eq!(out[0].origin, Some(SuggestionOrigin::Ngram));
    }

    #[test]
    fn an_all_empty_input_yields_no_suggestions() {
        assert!(clean_suggestions(&texts(&["", "  ", "«»"]), SuggestionOrigin::Model).is_empty());
    }
}
