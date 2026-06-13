// SPDX-License-Identifier: Apache-2.0

//! A budget-only token estimator (SPEC §5.C).
//!
//! The authoritative tokenizer is the model's BPE, in the `worker-llm`
//! process. The hub only needs a *budget* estimate to bound the assembled
//! context (≤ 2200 tokens, §5.C) before sending it, so this is a deliberate
//! approximation: it is **monotonic** (more text never estimates fewer tokens)
//! and **conservative-ish** for French, which is what budget enforcement needs.
//!
//! Heuristic: ~4 characters per token (the common rule of thumb), rounded up.
//! Documented as v0; replaced by the real tokenizer's count when the worker can
//! report it.

/// Average characters per token used by the budget estimate.
const CHARS_PER_TOKEN: usize = 4;

/// Estimates the token count of `text` for budget enforcement (not exact).
#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    // Count Unicode scalar values, not bytes: accents must not inflate the
    // estimate (« é » is one char, two bytes).
    let chars = text.chars().count();
    chars.div_ceil(CHARS_PER_TOKEN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_is_zero_tokens() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn estimate_rounds_up() {
        assert_eq!(estimate_tokens("abcd"), 1); // exactly 4 chars
        assert_eq!(estimate_tokens("abcde"), 2); // 5 chars → ceil(5/4)
    }

    #[test]
    fn accents_count_as_one_character_each() {
        // « été » is 3 chars (6 bytes); must estimate like any 3-char word.
        assert_eq!(estimate_tokens("été"), estimate_tokens("abc"));
    }

    #[test]
    fn estimate_is_monotonic() {
        assert!(estimate_tokens("bonjour") <= estimate_tokens("bonjour tout le monde"));
    }
}
