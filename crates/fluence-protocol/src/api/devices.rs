// SPDX-License-Identifier: Apache-2.0

//! Paired-device management (caregiver space, SPEC §7.C).
//!
//! Listing is read-only metadata for the caregiver: who is paired, with what
//! scope, since when, and whether revoked. **No token is ever exposed** — only
//! its SHA-256 is stored (ADR-0005), and not even that leaves the hub.
//!
//! Stability: **stable** (the caregiver space ships at A1).

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::api::pair::{DeviceKind, Scope};

/// `GET /devices` — every paired device, revoked included (caregiver space).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DeviceList {
    /// Paired devices, oldest first.
    pub devices: Vec<DeviceInfo>,
}

/// One paired device as the caregiver sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DeviceInfo {
    /// Stable device id (the handle for revocation).
    pub id: String,
    /// Human name chosen at pairing (« Tablette du lit »).
    pub name: String,
    /// Device kind.
    pub kind: DeviceKind,
    /// Granted scope.
    pub scope: Scope,
    /// Pairing time.
    pub created_at: DateTime<Utc>,
    /// Revocation time, if revoked — a revoked device stays listed (greyed out),
    /// it is not silently dropped, so the caregiver keeps a full picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<DateTime<Utc>>,
}
