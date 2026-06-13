// SPDX-License-Identifier: Apache-2.0

//! API error envelope — RFC 9457 `application/problem+json` with a stable
//! machine-readable code catalogue (SPEC §2.B, PLAN task 1.2).
//!
//! **P0 rule**: `detail` and `instance` must never contain user content
//! (conversation text, memory items, voice data — SPEC §9.A). They describe
//! the *request handling*, never the *person*.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// RFC 9457 problem document, extended with [`ErrorCode`].
///
/// The `type` URI is derived from the code (`urn:fluence:problem:<code>`):
/// a stable URN that does not depend on owning a domain yet; it may become
/// a resolvable URL later without changing the codes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Problem {
    /// Stable problem-type URI (`urn:fluence:problem:<code>`).
    #[serde(rename = "type")]
    pub problem_type: String,
    /// Short human-readable summary, stable per code (English).
    pub title: String,
    /// HTTP status code.
    pub status: u16,
    /// Occurrence-specific explanation. **Never P0 content.**
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// URI identifying this occurrence (e.g. the request path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    /// Stable machine-readable code (the catalogue clients switch on).
    pub code: ErrorCode,
}

impl Problem {
    /// Builds a problem from a catalogue code, with its canonical title,
    /// status and type URI.
    #[must_use]
    pub fn from_code(code: ErrorCode) -> Self {
        let (status, title) = code.canonical();
        Self {
            problem_type: format!("urn:fluence:problem:{}", code.as_str()),
            title: title.to_owned(),
            status,
            detail: None,
            instance: None,
            code,
        }
    }

    /// Attaches an occurrence-specific detail. **Never pass P0 content.**
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// Stable error codes (wire: `snake_case`).
///
/// Marked `non_exhaustive` and tolerant on input ([`ErrorCode::Unknown`]):
/// the catalogue grows; clients fall back on `status` for codes they do not
/// know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ErrorCode {
    /// Pairing attempted outside an explicitly opened window (SPEC §2.A).
    PairingWindowClosed,
    /// Wrong or already-consumed pairing code.
    PairingCodeInvalid,
    /// Too many attempts; retry later (always carried by a 429).
    RateLimited,
    /// No `X-Fluence-Token` header on a protected route.
    TokenMissing,
    /// Unknown or revoked device token.
    TokenInvalid,
    /// Token scope does not allow this route (SPEC §2.A scope table).
    ScopeInsufficient,
    /// Unknown session id.
    SessionNotFound,
    /// Unknown profile id.
    ProfileNotFound,
    /// Unknown memory item id.
    MemoryItemNotFound,
    /// Unknown voice id.
    VoiceNotFound,
    /// Request shape was valid JSON but violated an invariant.
    ValidationFailed,
    /// ASR control requires a valid consent token (SPEC §5.A).
    AsrConsentRequired,
    /// The consent token expired (TTL elapsed).
    AsrConsentExpired,
    /// The serving worker is down and no fallback applies. Routes with a
    /// fallback (n-gram, OS voice) degrade instead of erroring (SPEC §2.C).
    WorkerUnavailable,
    /// Unexpected internal error (carries no internals in `detail`).
    Internal,
    /// Code added by a newer hub; clients fall back on `status`.
    #[serde(other)]
    Unknown,
}

impl ErrorCode {
    /// Canonical `(status, title)` for this code.
    #[must_use]
    pub fn canonical(self) -> (u16, &'static str) {
        match self {
            Self::PairingWindowClosed => (403, "Pairing window is closed"),
            Self::PairingCodeInvalid => (403, "Invalid pairing code"),
            Self::RateLimited => (429, "Too many attempts"),
            Self::TokenMissing => (401, "Missing device token"),
            Self::TokenInvalid => (401, "Invalid device token"),
            Self::ScopeInsufficient => (403, "Insufficient scope"),
            Self::SessionNotFound => (404, "Session not found"),
            Self::ProfileNotFound => (404, "Profile not found"),
            Self::MemoryItemNotFound => (404, "Memory item not found"),
            Self::VoiceNotFound => (404, "Voice not found"),
            Self::ValidationFailed => (422, "Validation failed"),
            Self::AsrConsentRequired => (403, "ASR consent required"),
            Self::AsrConsentExpired => (403, "ASR consent expired"),
            Self::WorkerUnavailable => (503, "Worker unavailable"),
            Self::Internal | Self::Unknown => (500, "Internal error"),
        }
    }

    /// Wire representation of this code (`snake_case`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PairingWindowClosed => "pairing_window_closed",
            Self::PairingCodeInvalid => "pairing_code_invalid",
            Self::RateLimited => "rate_limited",
            Self::TokenMissing => "token_missing",
            Self::TokenInvalid => "token_invalid",
            Self::ScopeInsufficient => "scope_insufficient",
            Self::SessionNotFound => "session_not_found",
            Self::ProfileNotFound => "profile_not_found",
            Self::MemoryItemNotFound => "memory_item_not_found",
            Self::VoiceNotFound => "voice_not_found",
            Self::ValidationFailed => "validation_failed",
            Self::AsrConsentRequired => "asr_consent_required",
            Self::AsrConsentExpired => "asr_consent_expired",
            Self::WorkerUnavailable => "worker_unavailable",
            Self::Internal => "internal",
            Self::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every catalogue code keeps `as_str` aligned with its serde wire name
    /// — the URN in `type` is built from `as_str`, drift would silently
    /// desynchronize them.
    #[test]
    fn as_str_matches_wire_name() {
        for code in [
            ErrorCode::PairingWindowClosed,
            ErrorCode::PairingCodeInvalid,
            ErrorCode::RateLimited,
            ErrorCode::TokenMissing,
            ErrorCode::TokenInvalid,
            ErrorCode::ScopeInsufficient,
            ErrorCode::SessionNotFound,
            ErrorCode::ProfileNotFound,
            ErrorCode::MemoryItemNotFound,
            ErrorCode::VoiceNotFound,
            ErrorCode::ValidationFailed,
            ErrorCode::AsrConsentRequired,
            ErrorCode::AsrConsentExpired,
            ErrorCode::WorkerUnavailable,
            ErrorCode::Internal,
        ] {
            let wire = serde_json::to_value(code).unwrap();
            assert_eq!(wire, code.as_str(), "{code:?}");
        }
    }

    #[test]
    fn unknown_codes_are_tolerated() {
        let code: ErrorCode = serde_json::from_str("\"quota_exceeded\"").unwrap();
        assert_eq!(code, ErrorCode::Unknown);
    }

    #[test]
    fn from_code_builds_canonical_document() {
        let problem = Problem::from_code(ErrorCode::ScopeInsufficient);
        assert_eq!(problem.status, 403);
        assert_eq!(
            problem.problem_type,
            "urn:fluence:problem:scope_insufficient"
        );
        let json = serde_json::to_value(&problem).unwrap();
        assert_eq!(json["code"], "scope_insufficient");
        assert_eq!(json["type"], "urn:fluence:problem:scope_insufficient");
    }
}
