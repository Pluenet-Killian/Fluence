// SPDX-License-Identifier: Apache-2.0

//! Voice pipeline for Fluence (SPEC §2.B, §6).
//!
//! Two regimes (D-6.1): an everyday voice in real time on CPU
//! (Piper/VITS, < 200 ms to first audio sample) and a quality stage
//! (F5-TTS) for GPU tiers and deferred uses. Voice banking and cloning
//! consume the same recording dataset (D-6.2); prosody tags are realized
//! per stage (D-6.3).
//!
//! The cloned voice is P0 intimate data: encrypted, never leaves the
//! household (§9.A).
//!
//! PLAN Phase 5 builds `worker-tts` with Piper FR and the OS-voice
//! fallback; the cloning pipeline is a P2 milestone. This crate
//! intentionally stays empty until then.

#[cfg(test)]
mod tests {
    /// D-10.1: the voice pipeline is a reusable brick, licensed Apache-2.0.
    #[test]
    fn crate_license_follows_d_10_1() {
        assert_eq!(env!("CARGO_PKG_LICENSE"), "Apache-2.0");
    }
}
