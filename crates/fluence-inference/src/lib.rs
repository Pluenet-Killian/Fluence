// SPDX-License-Identifier: Apache-2.0

//! Inference workers for Fluence (SPEC §2.B, §3).
//!
//! Hosts the worker processes the hub supervises: `worker-llm` (llama.cpp),
//! `worker-asr`, `worker-tts` (Piper), `worker-embed`. Workers run in child
//! processes so a native-library crash (GGML/ONNX) never kills input
//! (D-2.6); scheduling follows strict priorities — TTS preempts everything,
//! suggestions are cancellable per slot (D-3.3).
//!
//! Native bindings live behind isolated FFI crates when they appear; this
//! crate itself stays `forbid(unsafe_code)`.
//!
//! PLAN Phase 4 (4.1) lands the [`LlmBackend`] abstraction (ADR-0007) and a
//! deterministic stub; (4.2) adds [`LlamaServerBackend`], the local llama.cpp
//! backend realised as an HTTP client to a supervised `llama-server`
//! subprocess. `worker-tts` (Phase 5) implements and hosts the trait next.

mod backend;
mod llama_server;

pub use backend::{
    BackendError, CancelToken, GenerateOutcome, GenerateRequest, LlmBackend, StubBackend,
    UnavailableBackend,
};
pub use llama_server::LlamaServerBackend;

#[cfg(test)]
mod tests {
    /// D-10.1: inference workers are reusable bricks, licensed Apache-2.0.
    #[test]
    fn crate_license_follows_d_10_1() {
        assert_eq!(env!("CARGO_PKG_LICENSE"), "Apache-2.0");
    }
}
