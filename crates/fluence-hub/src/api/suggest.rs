// SPDX-License-Identifier: Apache-2.0

//! The acceleration-engine endpoints (SPEC §5.A): `/suggest` (SSE, per-slot
//! cancellation) and `/next-chars`. Both call the injected LLM backend and
//! **degrade to the n-gram fallback** when it is unavailable, so the keyboard
//! always predicts (D-2.6 « le clavier parle toujours ») — never a 5xx.

use std::convert::Infallible;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use fluence_accel::{
    ContextParts, DEFAULT_BUDGET_TOKENS, StyleProfile, assemble, clean_suggestions,
};
use fluence_inference::{GenerateOutcome, GenerateRequest};
use fluence_ngram::NgramModel;
use fluence_protocol::Normalized;
use fluence_protocol::api::suggest::{
    AbortReason, CharProb, NextCharsResponse, SuggestAborted, SuggestDelta, SuggestEvent,
    SuggestFinal, SuggestRequest, Suggestion, SuggestionOrigin,
};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::{Stream, StreamExt};

use crate::state::AppState;

/// How many next-character candidates to return (a keyboard needs a handful).
const NEXT_CHARS_TOP_K: usize = 16;
/// Cap on tokens one suggestion generates (a sentence, not an essay).
const SUGGEST_MAX_TOKENS: u32 = 64;
/// Buffered SSE events before back-pressure (deltas are tiny).
const SUGGEST_CHANNEL_CAPACITY: usize = 32;

/// `POST /api/v1/sessions/{id}/suggest` — streams suggestions over SSE
/// (SPEC §5.A). A new request on the same slot cancels the previous one (the
/// debounce lives server-side); the backend runs off the async runtime and an
/// unavailable backend degrades to n-gram completions — never a 5xx.
pub async fn stream_suggest(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(request): Json<SuggestRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let slot = request.slot.to_string();
    let (generation, cancel) = state.supersede_slot(&session_id, &slot);

    // Assemble the prompt (v0: no stored conversation turns yet — draft only).
    let prompt = assemble(
        &ContextParts {
            mode: request.mode,
            style: StyleProfile::default(),
            turns: Vec::new(),
            draft: request.draft.clone(),
            constraints: request.constraints.clone(),
        },
        DEFAULT_BUDGET_TOKENS,
    )
    .text;

    let (tx, rx) = mpsc::channel::<SuggestEvent>(SUGGEST_CHANNEL_CAPACITY);
    let engine = state.engine().clone();
    let fallback = state.fallback().clone();
    let draft = request.draft.clone();
    let n = request.n;
    let clear = (state.clone(), session_id, slot);

    // The backend trait is synchronous; generate on a blocking thread and
    // forward deltas through the channel. Cancellation is observed between
    // tokens (a superseding request trips `cancel`).
    tokio::task::spawn_blocking(move || {
        let mut produced = String::new();
        let outcome = engine.generate(
            &GenerateRequest {
                prompt,
                max_tokens: SUGGEST_MAX_TOKENS,
            },
            &cancel,
            &mut |delta| {
                produced.push_str(delta);
                let _ = tx.blocking_send(SuggestEvent::Delta(SuggestDelta {
                    i: 0,
                    text: delta.to_owned(),
                }));
            },
        );
        let terminal = match outcome {
            Ok(GenerateOutcome::Completed) => SuggestEvent::Final(SuggestFinal {
                suggestions: clean_suggestions(&[produced], SuggestionOrigin::Model),
            }),
            Ok(GenerateOutcome::Cancelled) => SuggestEvent::Aborted(SuggestAborted {
                reason: AbortReason::Superseded,
            }),
            Err(error) => {
                tracing::debug!(%error, "llm suggest unavailable; n-gram fallback");
                SuggestEvent::Final(SuggestFinal {
                    suggestions: ngram_suggestions(&fallback, &draft, n),
                })
            }
        };
        let _ = tx.blocking_send(terminal);
        let (state, session_id, slot) = clear;
        state.clear_slot(&session_id, &slot, generation);
    });

    let stream = ReceiverStream::new(rx).map(|event| Ok(sse_event(&event)));
    Sse::new(stream)
}

/// Maps an internal [`SuggestEvent`] to an SSE event: the variant name is the
/// `event:` field, its payload the `data:` JSON (the contract's SSE mapping).
fn sse_event(event: &SuggestEvent) -> Event {
    match event {
        SuggestEvent::Delta(delta) => json_event("delta", delta),
        SuggestEvent::Final(final_list) => json_event("final", final_list),
        SuggestEvent::Aborted(aborted) => json_event("aborted", aborted),
        // `SuggestEvent` is non-exhaustive; a future variant is sent opaquely.
        _ => Event::default().event("unknown").data("{}"),
    }
}

/// Builds one SSE event named `name` carrying `payload` as JSON.
fn json_event<T: serde::Serialize>(name: &str, payload: &T) -> Event {
    let data = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_owned());
    Event::default().event(name).data(data)
}

/// n-gram fallback suggestions: completions of the draft's last word. The
/// rephrase/replies *quality* is unavailable without the LLM (SPEC §2.C: those
/// modes are masked on fallback), but basic prediction keeps working.
fn ngram_suggestions(model: &NgramModel, draft: &str, n: u8) -> Vec<Suggestion> {
    let last_word = draft.split_whitespace().last().unwrap_or("");
    model
        .complete(last_word, usize::from(n))
        .into_iter()
        .map(|completion| Suggestion {
            text: completion.word,
            score: completion.score,
            origin: Some(SuggestionOrigin::Ngram),
        })
        .collect()
}

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
