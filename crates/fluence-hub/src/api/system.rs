// SPDX-License-Identifier: Apache-2.0

//! System surface: `/system/health` and `/system/capabilities`
//! (SPEC §5.A, D-3.3).

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use chrono::Utc;
use fluence_protocol::api::system::{
    AccessJournalEntry, AccessJournalResponse, CapabilitiesResponse, EmergencyRequest,
    HardwareTier, HealthResponse, SystemEvent, WorkerHealth,
};
use fluence_protocol::ws::ServerFrame;

use serde::Deserialize;

use crate::api::problem_response;
use crate::state::AppState;

/// `GET /api/v1/system/health`: worker states and rolling latencies
/// (latency classes start reporting when their pipelines exist, Phase 4+).
pub async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    let workers = state
        .workers()
        .iter()
        .map(|handle| {
            let status = handle.status();
            WorkerHealth {
                worker: status.kind,
                state: status.state,
                restart_count: status.restart_count,
                model: None,
            }
        })
        .collect();
    Json(HealthResponse {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        started_at: state.started_at(),
        workers,
        latencies: Vec::new(),
    })
}

/// `GET /api/v1/system/capabilities`: tier from a coarse RAM probe —
/// full installation profiling is the onboarding's job (SPEC §7.B,
/// Phase 7); features stay empty until real capabilities ship.
pub async fn capabilities(State(_state): State<AppState>) -> Json<CapabilitiesResponse> {
    Json(CapabilitiesResponse {
        api_version: 1,
        tier: detect_tier(),
        features: Vec::new(),
    })
}

/// ≥ 12 GiB total RAM → nominal, below → reduced. GPU tiers arrive with
/// the model runtime (Phase 4).
fn detect_tier() -> HardwareTier {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let total_gib = system.total_memory() / (1024 * 1024 * 1024);
    if total_gib >= 12 {
        HardwareTier::Nominal
    } else {
        HardwareTier::Reduced
    }
}

/// Query parameters of `GET /system/journal`.
#[derive(Debug, Deserialize)]
pub struct JournalQuery {
    /// Maximum entries (newest first).
    limit: Option<u32>,
}

/// Default number of journal entries returned when no limit is given.
const JOURNAL_DEFAULT_LIMIT: u32 = 100;
/// Hard cap on journal entries per call.
const JOURNAL_MAX_LIMIT: u32 = 1000;

/// `GET /api/v1/system/journal` (care scope): the local access journal
/// (SPEC §2.A). Metadata only — the store never holds P0 here (§9.A).
pub async fn journal(
    State(state): State<AppState>,
    Query(query): Query<JournalQuery>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let limit = query
        .limit
        .unwrap_or(JOURNAL_DEFAULT_LIMIT)
        .min(JOURNAL_MAX_LIMIT);
    match state.store().journal_recent(limit).await {
        Ok(entries) => {
            let entries = entries
                .into_iter()
                .map(|e| AccessJournalEntry {
                    at: e.at,
                    device_id: e.device_id,
                    action: e.action,
                    detail: e.detail,
                })
                .collect();
            Json(AccessJournalResponse { entries }).into_response()
        }
        Err(error) => {
            tracing::error!(%error, "journal read failed");
            problem_response(fluence_protocol::error::ErrorCode::Internal, None)
        }
    }
}

/// `POST /api/v1/system/emergency` (control scope): raise or clear the
/// emergency alert (D-7.4, SPEC §7.A).
///
/// Broadcasts [`SystemEvent::Emergency`] on the `system` topic to every paired
/// client (banner everywhere, local ring) and returns `204`: the resulting
/// state reaches the caller through that broadcast (it is subscribed), so no
/// response body is needed. The double confirmation is the composer's job — the
/// hub just fans out. The broadcast happens **before** the (best-effort)
/// journal append, so a slow store never delays a critical alert (D-2.6).
pub async fn emergency(
    State(state): State<AppState>,
    Json(request): Json<EmergencyRequest>,
) -> StatusCode {
    state
        .bus()
        .publish(ServerFrame::System(SystemEvent::Emergency {
            active: request.active,
            at: Utc::now(),
        }));
    // Audit trail for the caregiver space (metadata only, never P0 — §9.A).
    let action = if request.active {
        "emergency.raised"
    } else {
        "emergency.cleared"
    };
    state.journal(action, None, None).await;
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use fluence_store::{KeySource, Store, StoreConfig};

    use super::*;
    use crate::config::HubConfig;
    use crate::events::EventBus;

    async fn test_state(dir: &tempfile::TempDir) -> AppState {
        let store = Store::open(StoreConfig {
            path: dir.path().join("store.db"),
            key: KeySource::File(dir.path().join("store.key")),
        })
        .await
        .expect("store opens");
        AppState::new(HubConfig::default(), store, EventBus::new())
    }

    #[tokio::test]
    async fn emergency_broadcasts_on_the_system_topic_and_returns_204() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;
        let mut receiver = state.bus().subscribe();

        let status = emergency(
            State(state.clone()),
            Json(EmergencyRequest { active: true }),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        match receiver.recv().await.expect("a frame is broadcast") {
            ServerFrame::System(SystemEvent::Emergency { active, .. }) => assert!(active),
            other => panic!("expected a system.emergency frame, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn clearing_the_emergency_broadcasts_active_false() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;
        let mut receiver = state.bus().subscribe();

        let status = emergency(
            State(state.clone()),
            Json(EmergencyRequest { active: false }),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        match receiver.recv().await.expect("a frame is broadcast") {
            ServerFrame::System(SystemEvent::Emergency { active, .. }) => assert!(!active),
            other => panic!("expected a system.emergency frame, got {other:?}"),
        }
    }
}
