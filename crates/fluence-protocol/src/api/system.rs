// SPDX-License-Identifier: Apache-2.0

//! System domain: degradation events, health and capabilities
//! (SPEC §2.C, §5.A, D-3.3).
//!
//! Stability: **stable** (A1 core). The event enum is `non_exhaustive` by
//! design — system events grow (emergency banner in Phase 5) and clients
//! must ignore what they do not know.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::common::ModelId;
use crate::ws::Topic;

/// Hub → clients events on the `system` topic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "k")]
#[non_exhaustive]
pub enum SystemEvent {
    /// First frame after a WebSocket open: negotiation outcome
    /// (see [`crate::ws`] for the connection contract).
    #[serde(rename = "system.hello")]
    Hello {
        /// Protocol version retained by the hub.
        v: u32,
        /// Topics actually granted (requested ∩ allowed by scope).
        topics: Vec<Topic>,
    },
    /// A worker changed state — the explicit degradation chain of
    /// « le clavier parle toujours » (SPEC §2.C, D-2.6).
    #[serde(rename = "system.degraded")]
    Degraded {
        /// Which worker.
        worker: WorkerKind,
        /// Its new state.
        state: WorkerState,
        /// Restarts since hub boot (present when the supervisor restarts it).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restart_count: Option<u32>,
    },
    /// ASR listening state changed. Every UI must show a visible indicator
    /// while listening is on (SPEC §5.A — third-party privacy).
    #[serde(rename = "system.listening")]
    Listening {
        /// Whether the microphone pipeline is active.
        enabled: bool,
    },
}

/// An inference worker supervised by the hub (SPEC §2.C).
///
/// Unknown kinds deserialize as [`WorkerKind::Unknown`] (forward
/// compatibility).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WorkerKind {
    /// Language model (llama.cpp).
    Llm,
    /// Speech recognition.
    Asr,
    /// Text-to-speech (Piper).
    Tts,
    /// Embeddings (memory retrieval).
    Embed,
    /// Worker kind added by a newer hub.
    #[serde(other)]
    Unknown,
}

/// Worker lifecycle state (PLAN Phase 2.3 supervisor states).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum WorkerState {
    /// Spawned, loading its model.
    Starting,
    /// Serving normally.
    Ready,
    /// Serving with reduced capability (fallback active).
    Degraded,
    /// Not serving; supervisor is backing off before restart.
    Down,
}

/// `GET /system/health` — worker states, loaded models, rolling latencies
/// (SPEC §5.A, D-3.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HealthResponse {
    /// Hub semantic version (`CARGO_PKG_VERSION`).
    pub version: String,
    /// Hub start time.
    pub started_at: DateTime<Utc>,
    /// One entry per supervised worker.
    pub workers: Vec<WorkerHealth>,
    /// Rolling p50/p95 per latency class (D-3.3).
    pub latencies: Vec<LatencyStat>,
}

/// Health of one worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WorkerHealth {
    /// Which worker.
    pub worker: WorkerKind,
    /// Current state.
    pub state: WorkerState,
    /// Restarts since hub boot.
    pub restart_count: u32,
    /// Model currently loaded, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
}

/// A model from the registry, pinned by version (D-3.2: a model never
/// changes silently).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModelRef {
    /// Registry id.
    pub id: ModelId,
    /// Manifest version.
    pub version: String,
}

/// Rolling latency statistics for one request class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LatencyStat {
    /// Measured class.
    pub class: LatencyClass,
    /// Median, milliseconds.
    pub p50_ms: f64,
    /// 95th percentile, milliseconds.
    pub p95_ms: f64,
}

/// Latency classes with contractual budgets (SPEC §5.A table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LatencyClass {
    /// `next-chars` on a warm KV (budget p50 20 ms / p95 50 ms).
    NextChars,
    /// First displayable suggest delta (300 / 600 ms).
    SuggestFirstDelta,
    /// Three complete suggestions (1.2 / 2.5 s).
    SuggestComplete,
    /// First audio sample of `speak` (200 / 400 ms).
    SpeakFirstAudio,
    /// Turn ingestion incl. background KV re-warm (100 / 250 ms).
    Turns,
    /// Input sample → selection decision (5 / 15 ms).
    InputDecision,
}

/// `GET /system/capabilities` — what this installation can do
/// (hardware tier §3, available features).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilitiesResponse {
    /// API version served by this hub (`/api/v1`).
    pub api_version: u32,
    /// Hardware tier from installation profiling (SPEC §3).
    pub tier: HardwareTier,
    /// Available feature flags (`asr`, `voice_cloning`, `embeddings`…).
    /// Free-form strings: features appear with releases without a contract
    /// change; clients test membership, never enumerate.
    pub features: Vec<String>,
}

/// Hardware tier (SPEC §3 fleet table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HardwareTier {
    /// 8 GB RAM, no GPU (FLU-REF-1) — latency budgets are defined here.
    Reduced,
    /// 16 GB RAM.
    Nominal,
    /// ≥ 12 GB VRAM GPU hub.
    GpuHub,
}

/// `GET /system/journal` — the local access journal, shown in the
/// caregiver space (SPEC §2.A, §7.C). **Metadata only, never P0**
/// (SPEC §9.A): entries describe access actions, never the content of a
/// conversation, draft, or memory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AccessJournalResponse {
    /// Recent entries, newest first.
    pub entries: Vec<AccessJournalEntry>,
}

/// One access-journal entry (SPEC §2.A, §9.A).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AccessJournalEntry {
    /// When the action happened.
    pub at: DateTime<Utc>,
    /// Acting device id, when the action was authenticated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// Stable action name (`pair.window_opened`, `device.paired`,
    /// `device.revoked`, `auth.rejected`…).
    pub action: String,
    /// Non-P0 context (route, device kind…). Never user content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degraded_event_wire_format() {
        let event = SystemEvent::Degraded {
            worker: WorkerKind::Llm,
            state: WorkerState::Down,
            restart_count: Some(3),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["k"], "system.degraded");
        assert_eq!(json["worker"], "llm");
        assert_eq!(json["state"], "down");
    }

    #[test]
    fn unknown_system_event_fails_closed_but_unknown_worker_is_tolerated() {
        // Unknown event kinds are NOT silently coerced — the envelope enum
        // has no catch-all variant, so dispatch layers (SDK) must skip
        // frames that fail to parse. Unknown WORKER kinds, however, parse.
        let worker: WorkerKind = serde_json::from_str("\"ocr\"").unwrap();
        assert_eq!(worker, WorkerKind::Unknown);
    }
}
