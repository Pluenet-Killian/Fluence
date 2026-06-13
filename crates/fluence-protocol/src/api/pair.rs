// SPDX-License-Identifier: Apache-2.0

//! Pairing and device scopes (SPEC §2.A, D-2.4).
//!
//! Pairing is only possible during a window explicitly opened from the main
//! UI (2 min, single-use code, rate-limited). `POST /pair` and
//! `GET /pair/info` are the only routes reachable without a token.
//!
//! Stability: **stable** (A1 core).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Access scope of a device token (SPEC §2.A scope table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Text to display, speech state — read only (partner-facing screen).
    Display,
    /// Full composer, suggestions, TTS, input (the user's devices).
    Control,
    /// Configuration, memory per ACL, diagnostics (caregiver).
    Care,
    /// Everything (embedded UI and local CLI only).
    System,
}

/// Kind of device requesting pairing — drives naming and display in the
/// caregiver space. Unknown kinds are tolerated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DeviceKind {
    /// Desktop or laptop computer.
    Desktop,
    /// Tablet.
    Tablet,
    /// Phone.
    Phone,
    /// Dedicated partner-facing display.
    Display,
    /// Command-line tool.
    Cli,
    /// Kind added by a newer client.
    #[serde(other)]
    Unknown,
}

/// `POST /pair` request (no token; only valid while the window is open).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PairRequest {
    /// 8-digit single-use code shown on the main screen.
    pub code: String,
    /// Human name for the device (« Tablette du lit »).
    pub device_name: String,
    /// What kind of device this is.
    pub device_kind: DeviceKind,
}

/// `POST /pair` response: everything the device needs to talk to the hub.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PairResponse {
    /// Local CA certificate (PEM) for TLS pinning (home mode, §2.A).
    pub ca_cert: String,
    /// Per-device revocable token (header `X-Fluence-Token`).
    pub device_token: String,
    /// Scope granted by this pairing window.
    pub scope: Scope,
}

/// `GET /pair/info` — what a pairing screen needs to display (no token).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PairInfo {
    /// API version served by this hub.
    pub api_version: u32,
    /// Household name announced over mDNS (§2.A).
    pub household_name: String,
    /// Local CA fingerprint to compare on the pairing screen (TOFU path).
    pub ca_fingerprint: String,
    /// Whether a pairing window is currently open.
    pub pairing_open: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_wire_names_are_lowercase() {
        assert_eq!(serde_json::to_value(Scope::Care).unwrap(), "care");
        // PLAN T2: unknown scopes are rejected — scopes gate security and
        // must fail closed, unlike presentation enums.
        assert!(serde_json::from_str::<Scope>("\"root\"").is_err());
    }

    #[test]
    fn unknown_device_kind_is_tolerated() {
        let kind: DeviceKind = serde_json::from_str("\"watch\"").unwrap();
        assert_eq!(kind, DeviceKind::Unknown);
    }
}
