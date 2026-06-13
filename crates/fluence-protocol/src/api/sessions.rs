// SPDX-License-Identifier: Apache-2.0

//! Conversation sessions, turns and draft sync (SPEC §5.A).
//!
//! One session = one conversation with a warm KV-cache hub-side. Turns feed
//! the context (§5.C); the draft is the text being composed, autosaved
//! continuously (≤ 1 s guaranteed loss bound, D-2.6).
//!
//! Stability: **stable** (A1 core).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::common::SessionId;

/// `POST /sessions` response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CreateSessionResponse {
    /// Newly created session.
    pub session_id: SessionId,
}

/// `POST /sessions/{id}/turns` — one conversation turn (SPEC §5.A).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Turn {
    /// Who produced this turn.
    pub speaker: Speaker,
    /// Turn text. **P0 content** — never logged (SPEC §9.A).
    pub text: String,
    /// How the text was produced.
    pub source: TurnSource,
}

/// Who produced a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    /// The Fluence user.
    User,
    /// The conversation partner.
    Partner,
}

/// How a turn's text was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TurnSource {
    /// Typed in the composer.
    Typed,
    /// Transcribed from the partner's speech (consented ASR, §5.A).
    Asr,
    /// Spoken through TTS by the user.
    Spoken,
}

/// `PUT /sessions/{id}/draft` — draft synchronization (SPEC §5.A).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Draft {
    /// Current draft text. **P0 content** — never logged.
    pub text: String,
    /// Caret position, in Unicode scalar values (not bytes), `0 ≤ caret ≤
    /// text.chars().count()`.
    pub caret: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_wire_format() {
        let turn = Turn {
            speaker: Speaker::Partner,
            text: "On mange quoi ce soir ?".to_owned(),
            source: TurnSource::Asr,
        };
        let json = serde_json::to_value(&turn).unwrap();
        assert_eq!(json["speaker"], "partner");
        assert_eq!(json["source"], "asr");
    }

    #[test]
    fn unknown_speaker_is_rejected() {
        // PLAN T2: speaker identity matters for context assembly — fail
        // closed rather than misattribute a turn.
        assert!(serde_json::from_str::<Speaker>("\"narrator\"").is_err());
    }
}
