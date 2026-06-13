// SPDX-License-Identifier: Apache-2.0

//! Voice pipeline for Fluence (SPEC §2.B, §6).
//!
//! Two regimes (D-6.1): an everyday voice in real time on CPU
//! ([`PiperBackend`], < 200 ms to first audio sample) and a quality stage
//! (F5-TTS) for GPU tiers — deferred (P2). Below the engines sits the OS
//! fallback ([`SystemVoiceBackend`]): a non-neural voice that is *always*
//! available, so vocalising never depends on the neural TTS being up — the
//! voice side of « le clavier parle toujours » (SPEC §2.C, D-2.6).
//!
//! Backends produce a complete WAV (16-bit mono PCM, [`wav_from_pcm`]) the hub
//! streams as `audio/wav`. Opus/Ogg (bandwidth for LAN/home mode) is deferred to
//! Phase 7 (ADR-0009). The cloned voice is P0 intimate data: encrypted, never
//! leaves the household (§9.A).

mod piper;
mod system;
mod wav;

use std::sync::Arc;

use fluence_protocol::api::voice::{ProsodyTag, VoiceInfo};

pub use piper::PiperBackend;
pub use system::{SYSTEM_VOICE_ID, SystemVoiceBackend};
pub use wav::wav_from_pcm;

/// A voice backend failure.
#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    /// The backend cannot serve (not configured, binary/model absent).
    #[error("voice backend unavailable: {0}")]
    Unavailable(String),
    /// Synthesis was attempted but failed (subprocess error, bad output).
    #[error("synthesis failed: {0}")]
    Synthesis(String),
}

/// A synthesized utterance: a complete WAV (16-bit mono PCM) ready to stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Utterance {
    /// The WAV bytes (RIFF/WAVE header + PCM).
    pub wav: Vec<u8>,
    /// The voice that actually produced it (may differ from the request when a
    /// fallback was used).
    pub voice_id: String,
}

/// A text-to-speech backend producing a complete WAV utterance.
///
/// Synchronous: the hub calls it from a blocking task. Implementations must be
/// `Send + Sync` so the engine can be shared behind an `Arc` across requests.
pub trait VoiceBackend: Send + Sync {
    /// The voice(s) this backend offers (`GET /voice/voices`).
    fn voices(&self) -> Vec<VoiceInfo>;

    /// Synthesizes `text` into a WAV. `voice_id` selects among the offered
    /// voices (best-effort; a single-voice backend ignores it). `prosody` is
    /// applied best-effort (D-6.3).
    ///
    /// # Errors
    ///
    /// [`VoiceError`] when the backend is unavailable or synthesis fails.
    fn synthesize(
        &self,
        text: &str,
        voice_id: &str,
        prosody: Option<ProsodyTag>,
    ) -> Result<Utterance, VoiceError>;
}

/// The default backend: no voice configured, every call fails cleanly. The hub
/// then answers `/voice/speak` with a degraded error rather than 5xx.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableVoice;

impl VoiceBackend for UnavailableVoice {
    fn voices(&self) -> Vec<VoiceInfo> {
        Vec::new()
    }

    fn synthesize(
        &self,
        _text: &str,
        _voice_id: &str,
        _prosody: Option<ProsodyTag>,
    ) -> Result<Utterance, VoiceError> {
        Err(VoiceError::Unavailable("no voice configured".to_owned()))
    }
}

/// Tries a primary voice (Piper), falling back to a secondary (the OS voice) on
/// failure — « une voix, toujours » (SPEC §2.C). When the request names a
/// secondary voice explicitly it is honoured directly.
pub struct FallbackVoice {
    primary: Option<Arc<dyn VoiceBackend>>,
    secondary: Arc<dyn VoiceBackend>,
}

impl FallbackVoice {
    /// Builds the combinator from an optional primary and a guaranteed
    /// secondary (the always-available OS voice).
    #[must_use]
    pub fn new(primary: Option<Arc<dyn VoiceBackend>>, secondary: Arc<dyn VoiceBackend>) -> Self {
        Self { primary, secondary }
    }
}

impl VoiceBackend for FallbackVoice {
    fn voices(&self) -> Vec<VoiceInfo> {
        let mut all = self
            .primary
            .as_ref()
            .map(|primary| primary.voices())
            .unwrap_or_default();
        all.extend(self.secondary.voices());
        all
    }

    fn synthesize(
        &self,
        text: &str,
        voice_id: &str,
        prosody: Option<ProsodyTag>,
    ) -> Result<Utterance, VoiceError> {
        // An explicit request for a secondary (OS) voice goes straight there.
        if self
            .secondary
            .voices()
            .iter()
            .any(|voice| voice.id.0 == voice_id)
        {
            return self.secondary.synthesize(text, voice_id, prosody);
        }
        // Otherwise prefer the primary, falling back so a voice always comes out.
        if let Some(primary) = &self.primary {
            match primary.synthesize(text, voice_id, prosody) {
                Ok(utterance) => return Ok(utterance),
                Err(error) => {
                    tracing::warn!(%error, "primary voice failed; falling back to the OS voice");
                }
            }
        }
        self.secondary.synthesize(text, voice_id, prosody)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D-10.1: the voice pipeline is a reusable brick, licensed Apache-2.0.
    #[test]
    fn crate_license_follows_d_10_1() {
        assert_eq!(env!("CARGO_PKG_LICENSE"), "Apache-2.0");
    }

    /// A fake backend that records calls and returns a canned utterance or error.
    struct FakeVoice {
        id: &'static str,
        fail: bool,
    }

    impl VoiceBackend for FakeVoice {
        fn voices(&self) -> Vec<VoiceInfo> {
            vec![VoiceInfo {
                id: self.id.into(),
                name: self.id.to_owned(),
                kind: fluence_protocol::api::voice::VoiceKind::Piper,
                language: "fr-FR".to_owned(),
            }]
        }

        fn synthesize(
            &self,
            _text: &str,
            _voice_id: &str,
            _prosody: Option<ProsodyTag>,
        ) -> Result<Utterance, VoiceError> {
            if self.fail {
                Err(VoiceError::Synthesis("boom".to_owned()))
            } else {
                Ok(Utterance {
                    wav: vec![1, 2, 3],
                    voice_id: self.id.to_owned(),
                })
            }
        }
    }

    #[test]
    fn unavailable_voice_always_errors() {
        let voice = UnavailableVoice;
        assert!(voice.voices().is_empty());
        assert!(matches!(
            voice.synthesize("salut", "x", None),
            Err(VoiceError::Unavailable(_))
        ));
    }

    #[test]
    fn fallback_uses_primary_when_it_succeeds() {
        let fallback = FallbackVoice::new(
            Some(Arc::new(FakeVoice {
                id: "piper:a",
                fail: false,
            })),
            Arc::new(FakeVoice {
                id: "system:default",
                fail: false,
            }),
        );
        let utterance = fallback.synthesize("salut", "piper:a", None).expect("ok");
        assert_eq!(utterance.voice_id, "piper:a");
    }

    #[test]
    fn fallback_falls_back_to_secondary_when_primary_fails() {
        let fallback = FallbackVoice::new(
            Some(Arc::new(FakeVoice {
                id: "piper:a",
                fail: true,
            })),
            Arc::new(FakeVoice {
                id: "system:default",
                fail: false,
            }),
        );
        let utterance = fallback.synthesize("salut", "piper:a", None).expect("ok");
        assert_eq!(utterance.voice_id, "system:default");
    }

    #[test]
    fn an_explicit_system_voice_request_skips_the_primary() {
        let fallback = FallbackVoice::new(
            Some(Arc::new(FakeVoice {
                id: "piper:a",
                fail: false,
            })),
            Arc::new(FakeVoice {
                id: "system:default",
                fail: false,
            }),
        );
        let utterance = fallback
            .synthesize("salut", "system:default", None)
            .expect("ok");
        assert_eq!(utterance.voice_id, "system:default");
    }

    #[test]
    fn fallback_lists_all_voices() {
        let fallback = FallbackVoice::new(
            Some(Arc::new(FakeVoice {
                id: "piper:a",
                fail: false,
            })),
            Arc::new(FakeVoice {
                id: "system:default",
                fail: false,
            }),
        );
        assert_eq!(fallback.voices().len(), 2);
    }
}
