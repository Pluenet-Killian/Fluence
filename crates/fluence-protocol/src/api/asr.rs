// SPDX-License-Identifier: Apache-2.0

//! ASR listening control — first-class consent (SPEC §5.A).
//!
//! Turning the microphone on requires a `consent_token` produced by an
//! explicit UI action, journaled, with a TTL. The listening state is
//! broadcast on the `system` topic so **every** UI shows the indicator
//! (third-party privacy, SPEC §5).
//!
//! Stability: **experimental** (P2 domain — the ASR engine itself is
//! benchmarked in D-3.4 and built in Phase 8; the consent flow may be
//! refined then).

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// `POST /asr/consent` — obtain a consent token via an explicit UI action.
/// The action is journaled locally (who, when, which device).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ConsentResponse {
    /// Token to pass to `POST /asr/listening`.
    pub consent_token: String,
    /// When this token stops being accepted.
    pub expires_at: DateTime<Utc>,
}

/// `POST /asr/listening` request — start/stop partner-speech listening.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ListeningRequest {
    /// Desired listening state.
    pub enabled: bool,
    /// Consent token (required to enable; see [`ConsentResponse`]).
    pub consent_token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listening_request_wire_format() {
        let request: ListeningRequest =
            serde_json::from_str(r#"{"enabled":true,"consent_token":"ct_abc"}"#).unwrap();
        assert!(request.enabled);
        assert_eq!(request.consent_token, "ct_abc");
    }
}
