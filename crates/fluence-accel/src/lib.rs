// SPDX-License-Identifier: Apache-2.0

//! Acceleration engine for Fluence — the heart of the platform (SPEC §5).
//!
//! Four functions, one infrastructure: `suggestReplies`, `rephrase`,
//! `expand`, `nextTokenDistribution`. This crate assembles the prompt
//! context (§5.C: stable→volatile ordering to maximize KV-cache reuse,
//! ≤ 2200 tokens), performs personal-memory retrieval (§5.B: hybrid
//! BM25 + vector, < 30 ms), and post-processes generations (dedup, casing,
//! punctuation) with per-slot cancellation.
//!
//! Agency rule (§5): the system speaks *like the person*, never instead of
//! her — every suggestion is editable and rejectable in one gesture.
//!
//! PLAN Phase 4 (4.4) lands context assembly (blocks 1/4/5 — memory and the
//! rolling summary are the §5.B subsystem, P2) and the v0 `rephrase`/`continue`
//! prompts, plus post-processing. The pieces are pure functions: the hub
//! composes them with an `LlmBackend` and owns per-slot cancellation.

mod postprocess;
mod prompt;
mod tokens;

pub use postprocess::clean_suggestions;
pub use prompt::{
    AssembledPrompt, ContextParts, ContextTurn, DEFAULT_BUDGET_TOKENS, Speaker, StyleProfile,
    assemble, relative_label,
};
pub use tokens::estimate_tokens;

#[cfg(test)]
mod tests {
    /// D-10.1: the acceleration engine is a reusable brick, Apache-2.0.
    #[test]
    fn crate_license_follows_d_10_1() {
        assert_eq!(env!("CARGO_PKG_LICENSE"), "Apache-2.0");
    }
}
