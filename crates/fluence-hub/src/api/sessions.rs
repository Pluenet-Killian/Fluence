// SPDX-License-Identifier: Apache-2.0

//! Phase 2 session handlers: ids and the persisted draft. The KV-cache
//! semantics (turns, suggest) arrive with the LLM worker (Phase 4) — a
//! session here is the durable anchor of its draft.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use fluence_protocol::api::sessions::{CreateSessionResponse, Draft};
use secrecy::{ExposeSecret, SecretString};

use crate::api::problem_response;
use crate::state::{AppState, PendingDraft};

/// `POST /api/v1/sessions`: mints a session id. Drafts are keyed by it;
/// the id needs no other server-side state in Phase 2.
pub async fn create_session(State(_state): State<AppState>) -> Json<CreateSessionResponse> {
    Json(CreateSessionResponse {
        session_id: uuid::Uuid::new_v4().to_string().into(),
    })
}

/// `DELETE /api/v1/sessions/{id}`: a closed conversation's draft has no
/// reason to outlive it (it was spoken or abandoned).
///
/// `discard_pending_draft` both drops the unflushed buffer **and** plants
/// a tombstone, so a concurrent autosave flush that already snapshotted
/// this draft is suppressed before it can write back — the `DELETE` is
/// authoritative against the flusher, and the P0 text is purged for good
/// (SPEC §9.A; closes the delete-vs-flush race F10).
pub async fn delete_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Response {
    state.discard_pending_draft(&session_id);
    match state.store().delete_draft(session_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            tracing::error!(%error, "draft purge failed");
            problem_response(fluence_protocol::error::ErrorCode::Internal, None)
        }
    }
}

/// `PUT /api/v1/sessions/{id}/draft`: buffers the keystroke state; the
/// periodic flusher persists it (≤ 1 s loss bound, D-2.6). The text
/// becomes a `SecretString` at the boundary — P0 never travels bare
/// through the hub.
///
/// Two guards keep an authenticated Control device from exhausting hub
/// resources (F09): oversized text is rejected before it is ever wrapped
/// as P0 (a 422 carrying *no* P0), and an overflow of the in-RAM buffer cap
/// forces an immediate flush so memory use stays bounded.
pub async fn put_draft(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(draft): Json<Draft>,
) -> Response {
    // Reject oversized text before it becomes a P0 `SecretString`. `len()`
    // is the byte length; the error mentions only the limit, never the text.
    if draft.text.len() > crate::state::MAX_DRAFT_TEXT_BYTES {
        return problem_response(
            fluence_protocol::error::ErrorCode::ValidationFailed,
            Some(format!(
                "draft text exceeds the {}-byte limit",
                crate::state::MAX_DRAFT_TEXT_BYTES
            )),
        );
    }

    let updated_at_micros = u64::try_from(chrono::Utc::now().timestamp_micros()).unwrap_or(0);
    let overflow = state.buffer_draft(
        session_id,
        PendingDraft {
            text: SecretString::from(draft.text),
            caret: draft.caret,
            updated_at_micros,
        },
    );
    // Too many distinct unflushed sessions: drain to disk right away — the
    // same flush the periodic task does, only brought forward. It never
    // blocks the keystroke (already buffered); the P0 lands in the encrypted
    // store within this request instead of accumulating in RAM (F09).
    if overflow {
        state.flush_drafts().await;
    }
    StatusCode::NO_CONTENT.into_response()
}

/// `GET /api/v1/sessions/{id}/draft`: freshest view — the dirty buffer
/// when present, the store otherwise (that is what restores after a
/// crash; SPEC §2.A session resumption).
pub async fn get_draft(State(state): State<AppState>, Path(session_id): Path<String>) -> Response {
    if let Some(pending) = state.pending_draft(&session_id) {
        return Json(Draft {
            text: pending.text.expose_secret().to_owned(),
            caret: pending.caret,
        })
        .into_response();
    }
    match state.store().draft(session_id).await {
        Ok(Some(record)) => Json(Draft {
            text: record.text.expose_secret().to_owned(),
            caret: record.caret,
        })
        .into_response(),
        Ok(None) => problem_response(fluence_protocol::error::ErrorCode::SessionNotFound, None),
        Err(error) => {
            tracing::error!(%error, "draft read failed");
            problem_response(fluence_protocol::error::ErrorCode::Internal, None)
        }
    }
}
