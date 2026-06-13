// SPDX-License-Identifier: Apache-2.0

//! System surface: `/system/health` and `/system/capabilities`
//! (SPEC §5.A, D-3.3).

use axum::Json;
use axum::extract::{Query, State};
use fluence_protocol::api::system::{
    AccessJournalEntry, AccessJournalResponse, CapabilitiesResponse, HardwareTier, HealthResponse,
    WorkerHealth,
};
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
