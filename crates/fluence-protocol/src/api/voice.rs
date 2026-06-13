// SPDX-License-Identifier: Apache-2.0

//! Voice API: `/voice/speak` and `/voice/voices` (SPEC §5.A, §6).
//!
//! `speak` is the **P0 priority class** of the scheduler (D-3.3): it
//! preempts everything. The response is streamed audio (WAV in v0 — ADR-0009;
//! Opus for LAN/home mode is deferred to Phase 7), not JSON.
//!
//! Stability: **stable** (A1 core — basic voice; the cloning pipeline is a
//! P2 domain and not part of this contract yet).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::common::VoiceId;

/// `POST /voice/speak` request. Response: streamed audio (`audio/wav` in v0 —
/// ADR-0009), first sample < 200 ms (SPEC §5.A).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpeakRequest {
    /// Text to vocalize. **P0 content** — never logged.
    pub text: String,
    /// Which installed voice to use.
    pub voice_id: VoiceId,
    /// Prosody tag applied to the whole utterance (one commit = one tag,
    /// D-6.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prosody: Option<ProsodyTag>,
}

/// Prosody tags v1 (D-6.3). Piper realizes them as rate/pitch/volume
/// presets; F5-TTS uses the person's own emotional reference recordings.
///
/// Wire names are English (code convention); UIs label them in the user's
/// language (`question` → « Question », `tenderness` → « Tendresse »…).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProsodyTag {
    /// Default delivery.
    Neutral,
    /// Rising interrogative contour (shortcut `??`, D-6.3).
    Question,
    /// Exclamative emphasis (shortcut `!!`).
    Exclamation,
    /// Joyful.
    Joy,
    /// Tender, affectionate.
    Tenderness,
    /// Annoyed.
    Annoyance,
    /// Sad.
    Sadness,
    /// Whispered.
    Whisper,
    /// Louder delivery.
    Loud,
    /// Slower delivery.
    Slow,
    /// Faster delivery.
    Fast,
}

/// `GET /voice/voices` response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoicesResponse {
    /// Installed voices, ready to speak.
    pub voices: Vec<VoiceInfo>,
}

/// One installed voice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VoiceInfo {
    /// Stable id to pass to `speak`.
    pub id: VoiceId,
    /// Display name (« Siwis (médium) », « Ma voix »).
    pub name: String,
    /// Which engine family serves it.
    pub kind: VoiceKind,
    /// BCP 47 language tag (`fr-FR`).
    pub language: String,
}

/// Engine family of a voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VoiceKind {
    /// Operating-system fallback voice (SAPI / espeak-ng) — always
    /// available (« une voix, toujours », SPEC §2.C).
    System,
    /// Piper voice (stock or personal fine-tuning, D-6.1).
    Piper,
    /// Personal cloned voice, quality stage (F5-TTS, GPU tiers).
    Cloned,
    /// Kind added by a newer hub.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speak_request_wire_format() {
        let request = SpeakRequest {
            text: "On y va !".to_owned(),
            voice_id: "piper:fr_FR-siwis-medium".into(),
            prosody: Some(ProsodyTag::Exclamation),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["prosody"], "exclamation");
        // Absent prosody is omitted from the wire, not null.
        let bare = SpeakRequest {
            prosody: None,
            ..request
        };
        assert!(
            !serde_json::to_value(&bare)
                .unwrap()
                .as_object()
                .unwrap()
                .contains_key("prosody")
        );
    }

    #[test]
    fn all_eleven_prosody_tags_serialize() {
        // D-6.3 fixes eleven tags in v1 — a removal would be a breaking
        // contract change, caught here.
        let tags = [
            ProsodyTag::Neutral,
            ProsodyTag::Question,
            ProsodyTag::Exclamation,
            ProsodyTag::Joy,
            ProsodyTag::Tenderness,
            ProsodyTag::Annoyance,
            ProsodyTag::Sadness,
            ProsodyTag::Whisper,
            ProsodyTag::Loud,
            ProsodyTag::Slow,
            ProsodyTag::Fast,
        ];
        assert_eq!(tags.len(), 11);
        for tag in tags {
            assert!(serde_json::to_value(tag).unwrap().is_string());
        }
    }
}
