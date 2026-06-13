// SPDX-License-Identifier: Apache-2.0

//! The [`LlmBackend`] trait and a deterministic [`StubBackend`] (SPEC §5.A;
//! ADR-0007).
//!
//! One abstraction over every language-model source: the local llama.cpp
//! backend (Phase 4.2), the opt-in remote OpenAI-compatible backend (D-3.1),
//! and the stub. Generation streams token by token and is **cancellable
//! between tokens** (per-slot abort, §5.A); [`LlmBackend::next_chars`] is the
//! warm-KV next-character distribution that feeds adaptive scanning.
//!
//! The stub is fully deterministic, so `fluence-accel` and the hub endpoints
//! (Phase 4.4/4.5) can be built and tested before the real backend lands.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use fluence_protocol::Normalized;
use fluence_protocol::api::suggest::CharProb;

/// Cooperative cancellation, checked by a backend between tokens.
///
/// Cheap to clone (shared flag); the hub trips it when a newer request lands
/// on the same slot, and the backend stops at the next token boundary.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// A fresh, un-cancelled token.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation (idempotent).
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether cancellation has been requested.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// A generation request.
#[derive(Debug, Clone)]
pub struct GenerateRequest {
    /// The assembled prompt (context + instruction — `fluence-accel` builds it).
    pub prompt: String,
    /// Upper bound on generated tokens.
    pub max_tokens: u32,
}

/// How a generation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerateOutcome {
    /// The backend finished (stop token or `max_tokens`).
    Completed,
    /// Cancellation was observed before completion.
    Cancelled,
}

/// A backend failure (model unavailable, decode error…).
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    /// The backend cannot serve the request (worker down, model not loaded).
    #[error("backend unavailable: {0}")]
    Unavailable(String),
}

/// A language-model backend: local (llama.cpp), remote, or stub (ADR-0007).
pub trait LlmBackend: Send + Sync {
    /// Stable category identifier for reports and degradation events
    /// (`"stub"`, `"llama-cpp"`, `"openai-remote"`…); the model name, if any,
    /// is separate metadata.
    fn id(&self) -> &'static str;

    /// Streams a completion of `request.prompt` into `sink`, one text fragment
    /// at a time, stopping early if `cancel` trips.
    ///
    /// # Errors
    ///
    /// [`BackendError`] if the backend cannot serve the request.
    fn generate(
        &self,
        request: &GenerateRequest,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(&str),
    ) -> Result<GenerateOutcome, BackendError>;

    /// Next-character distribution after `context` (warm KV, no full
    /// generation), at most `top_k` entries, descending.
    ///
    /// # Errors
    ///
    /// [`BackendError`] if the backend cannot serve the request.
    fn next_chars(&self, context: &str, top_k: usize) -> Result<Vec<CharProb>, BackendError>;
}

/// A deterministic backend for tests and for building the pipeline before the
/// real model lands: it streams a canned response word by word and returns a
/// fixed next-character distribution.
#[derive(Debug, Clone)]
pub struct StubBackend {
    response: Vec<String>,
}

/// A fixed, frequency-plausible French next-character distribution (sums to 1).
/// A placeholder for the real backend's logit-derived distribution.
const STUB_NEXT_CHARS: &[(char, f64)] = &[('e', 0.4), ('a', 0.25), ('s', 0.2), ('t', 0.15)];

impl StubBackend {
    /// A stub whose generations stream the words of `response`.
    #[must_use]
    pub fn new(response: &str) -> Self {
        Self {
            response: response.split_whitespace().map(str::to_owned).collect(),
        }
    }
}

impl LlmBackend for StubBackend {
    fn id(&self) -> &'static str {
        "stub"
    }

    fn generate(
        &self,
        request: &GenerateRequest,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(&str),
    ) -> Result<GenerateOutcome, BackendError> {
        let _ = &request.prompt; // the stub's output is independent of the prompt
        let limit = usize::try_from(request.max_tokens)
            .unwrap_or(usize::MAX)
            .min(self.response.len());
        for (index, word) in self.response.iter().take(limit).enumerate() {
            if cancel.is_cancelled() {
                return Ok(GenerateOutcome::Cancelled);
            }
            if index > 0 {
                sink(" ");
            }
            sink(word);
        }
        Ok(GenerateOutcome::Completed)
    }

    fn next_chars(&self, _context: &str, top_k: usize) -> Result<Vec<CharProb>, BackendError> {
        let distribution = STUB_NEXT_CHARS
            .iter()
            .take(top_k)
            .map(|&(ch, p)| CharProb {
                ch,
                p: Normalized::new(p).expect("stub probabilities are within [0, 1]"),
            })
            .collect();
        Ok(distribution)
    }
}

/// A backend that is always unavailable — the hub's default until a real
/// `worker-llm` is configured, so every request degrades to the n-gram
/// fallback (D-2.6 « le clavier parle toujours ») instead of failing.
#[derive(Debug, Clone, Default)]
pub struct UnavailableBackend;

impl LlmBackend for UnavailableBackend {
    fn id(&self) -> &'static str {
        "unavailable"
    }

    fn generate(
        &self,
        _request: &GenerateRequest,
        _cancel: &CancelToken,
        _sink: &mut dyn FnMut(&str),
    ) -> Result<GenerateOutcome, BackendError> {
        Err(BackendError::Unavailable(
            "no LLM worker configured".to_owned(),
        ))
    }

    fn next_chars(&self, _context: &str, _top_k: usize) -> Result<Vec<CharProb>, BackendError> {
        Err(BackendError::Unavailable(
            "no LLM worker configured".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(
        backend: &dyn LlmBackend,
        max_tokens: u32,
        cancel: &CancelToken,
    ) -> (String, GenerateOutcome) {
        let request = GenerateRequest {
            prompt: "veu eau frache ce soir".to_owned(),
            max_tokens,
        };
        let mut out = String::new();
        let outcome = backend
            .generate(&request, cancel, &mut |delta| out.push_str(delta))
            .expect("stub never errors");
        (out, outcome)
    }

    #[test]
    fn stub_streams_the_canned_response() {
        let backend = StubBackend::new("je voudrais de l'eau");
        let (text, outcome) = collect(&backend, 100, &CancelToken::new());
        assert_eq!(text, "je voudrais de l'eau");
        assert_eq!(outcome, GenerateOutcome::Completed);
    }

    #[test]
    fn stub_stops_at_max_tokens() {
        let backend = StubBackend::new("un deux trois quatre");
        let (text, outcome) = collect(&backend, 2, &CancelToken::new());
        assert_eq!(text, "un deux");
        assert_eq!(outcome, GenerateOutcome::Completed);
    }

    #[test]
    fn stub_honours_cancellation_between_tokens() {
        let backend = StubBackend::new("un deux trois");
        let cancel = CancelToken::new();
        cancel.cancel(); // tripped before the first token
        let (text, outcome) = collect(&backend, 100, &cancel);
        assert_eq!(text, "");
        assert_eq!(outcome, GenerateOutcome::Cancelled);
    }

    #[test]
    fn next_chars_is_a_descending_distribution_summing_to_one() {
        let backend = StubBackend::new("");
        let dist = backend.next_chars("bonjou", 8).expect("stub never errors");
        assert_eq!(dist.len(), STUB_NEXT_CHARS.len());
        let total: f64 = dist.iter().map(|c| c.p.get()).sum();
        assert!(
            (total - 1.0).abs() < 1e-9,
            "distribution must sum to 1, got {total}"
        );
        for pair in dist.windows(2) {
            assert!(pair[0].p.get() >= pair[1].p.get(), "must be descending");
        }
    }

    #[test]
    fn next_chars_truncates_to_top_k() {
        let backend = StubBackend::new("");
        assert_eq!(backend.next_chars("ctx", 2).expect("ok").len(), 2);
    }

    #[test]
    fn cancel_token_starts_uncancelled_and_latches() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn unavailable_backend_errors_on_every_path() {
        let backend = UnavailableBackend;
        let request = GenerateRequest {
            prompt: "x".to_owned(),
            max_tokens: 8,
        };
        assert!(matches!(
            backend.generate(&request, &CancelToken::new(), &mut |_| {}),
            Err(BackendError::Unavailable(_))
        ));
        assert!(matches!(
            backend.next_chars("x", 4),
            Err(BackendError::Unavailable(_))
        ));
    }
}
