// SPDX-License-Identifier: Apache-2.0

//! The acceleration-engine endpoints (SPEC §5.A). `/next-chars` lands here
//! first; `/suggest` (SSE, per-slot cancellation) follows. Both call the
//! injected LLM backend and **degrade to the n-gram fallback** when it is
//! unavailable, so the keyboard always predicts (D-2.6 « le clavier parle
//! toujours ») — never a 5xx on this path.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use fluence_ngram::NgramModel;
use fluence_protocol::Normalized;
use fluence_protocol::api::suggest::{CharProb, NextCharsResponse};
use serde::Deserialize;

use crate::state::AppState;

/// How many next-character candidates to return (a keyboard needs a handful).
const NEXT_CHARS_TOP_K: usize = 16;

/// Query of `GET /sessions/{id}/next-chars`.
#[derive(Debug, Deserialize)]
pub struct NextCharsQuery {
    /// Draft prefix to condition the distribution on.
    prefix: String,
}

/// `GET /api/v1/sessions/{id}/next-chars?prefix=` — the next-character
/// distribution on the warm KV (SPEC §5.A). It reads logits only, never a full
/// generation; an unavailable backend degrades to the n-gram fallback so the
/// adaptive dwell / weighted scanning always has a distribution (D-2.6).
pub async fn next_chars(
    State(state): State<AppState>,
    Path(_session_id): Path<String>,
    Query(query): Query<NextCharsQuery>,
) -> Response {
    let engine = state.engine().clone();
    let prefix = query.prefix.clone();
    // The backend trait is synchronous (it reads model logits); run it off the
    // async runtime so a slow read never stalls the hub's keyboard path.
    let from_engine =
        tokio::task::spawn_blocking(move || engine.next_chars(&prefix, NEXT_CHARS_TOP_K)).await;

    let dist = match from_engine {
        Ok(Ok(dist)) => dist,
        Ok(Err(error)) => {
            tracing::debug!(%error, "llm next-chars unavailable; n-gram fallback");
            fallback_dist(state.fallback(), &query.prefix)
        }
        Err(join_error) => {
            tracing::error!(%join_error, "next-chars task failed; n-gram fallback");
            fallback_dist(state.fallback(), &query.prefix)
        }
    };
    Json(NextCharsResponse { dist }).into_response()
}

/// The n-gram fallback distribution, mapped into the contract's [`CharProb`].
fn fallback_dist(model: &NgramModel, prefix: &str) -> Vec<CharProb> {
    model
        .next_char_dist(prefix)
        .into_iter()
        .take(NEXT_CHARS_TOP_K)
        .filter_map(|(ch, p)| Normalized::new(p).ok().map(|p| CharProb { ch, p }))
        .collect()
}
