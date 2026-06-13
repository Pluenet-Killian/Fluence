// SPDX-License-Identifier: Apache-2.0

//! T4 — `/suggest` (SSE): streams the LLM backend's generation as `delta`/`final`
//! events, and **degrades to n-gram completions** when the backend is
//! unavailable (D-2.6) — never a 5xx. Per-slot cancellation is unit-tested at
//! the `AppState` level; here we check the streamed events end to end.

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use fluence_hub::api::build_router;
use fluence_hub::auth::token_hash;
use fluence_hub::config::HubConfig;
use fluence_hub::events::EventBus;
use fluence_hub::state::AppState;
use fluence_inference::{LlmBackend, StubBackend, UnavailableBackend};
use fluence_ngram::NgramModel;
use fluence_protocol::api::pair::{DeviceKind, Scope};
use fluence_protocol::api::suggest::{SuggestFinal, SuggestionOrigin};
use fluence_store::{KeySource, NewDevice, Store, StoreConfig};
use tower::ServiceExt;

const TOKEN: &str = "flt_suggest_test";

async fn hub_with(
    engine: Arc<dyn LlmBackend>,
    fallback: NgramModel,
    dir: &tempfile::TempDir,
) -> AppState {
    let store = Store::open(StoreConfig {
        path: dir.path().join("store.db"),
        key: KeySource::File(dir.path().join("store.key")),
    })
    .await
    .expect("store opens");
    store
        .insert_device(NewDevice {
            device_id: "dev-control".to_owned(),
            token_hash: token_hash(TOKEN),
            name: "test control device".to_owned(),
            kind: DeviceKind::Desktop,
            scope: Scope::Control,
        })
        .await
        .expect("device inserted");
    let config = HubConfig {
        data_dir: dir.path().to_owned(),
        store_key_file: Some(dir.path().join("store.key")),
        ..HubConfig::default()
    };
    AppState::new_with(config, store, EventBus::new(), engine, Arc::new(fallback))
}

/// POSTs a rephrase request and returns the (status, parsed SSE events).
async fn suggest(state: AppState, draft: &str) -> (StatusCode, Vec<(String, String)>) {
    let body = format!(r#"{{"mode":"rephrase","draft":"{draft}","n":3,"slot":"main"}}"#);
    let request = Request::post("/api/v1/sessions/s1/suggest")
        .header("X-Fluence-Token", TOKEN)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .expect("request builds");
    let response = build_router(state)
        .oneshot(request)
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 256 * 1024)
        .await
        .expect("body");
    (status, parse_sse(&String::from_utf8_lossy(&bytes)))
}

/// Parses an SSE body into `(event, data)` pairs.
fn parse_sse(body: &str) -> Vec<(String, String)> {
    let field = |frame: &str, name: &str| {
        frame
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .unwrap_or("")
            .trim()
            .to_owned()
    };
    body.split("\n\n")
        .filter(|frame| !frame.trim().is_empty())
        .map(|frame| (field(frame, "event:"), field(frame, "data:")))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn suggest_streams_the_llm_generation_to_a_final_event() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = hub_with(
        Arc::new(StubBackend::new("je voudrais de l'eau")),
        NgramModel::new(),
        &dir,
    )
    .await;
    let (status, events) = suggest(state, "veu eau frache").await;

    assert_eq!(status, StatusCode::OK);
    // At least one delta, then a final carrying the post-processed suggestion.
    assert!(events.iter().any(|(e, _)| e == "delta"), "expected deltas");
    let (_, final_data) = events
        .iter()
        .find(|(e, _)| e == "final")
        .expect("a final event");
    let parsed: SuggestFinal = serde_json::from_str(final_data).expect("SuggestFinal json");
    assert_eq!(parsed.suggestions.len(), 1);
    assert_eq!(parsed.suggestions[0].text, "Je voudrais de l'eau");
    assert_eq!(parsed.suggestions[0].origin, Some(SuggestionOrigin::Model));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn suggest_degrades_to_ngram_when_backend_unavailable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut fallback = NgramModel::new();
    fallback.train("frais fraises fraise");
    let state = hub_with(Arc::new(UnavailableBackend), fallback, &dir).await;
    let (status, events) = suggest(state, "j'ai soif fra").await;

    // Graceful degradation: 200, a final with n-gram completions, never a 5xx.
    assert_eq!(status, StatusCode::OK);
    let (_, final_data) = events
        .iter()
        .find(|(e, _)| e == "final")
        .expect("a final event");
    let parsed: SuggestFinal = serde_json::from_str(final_data).expect("SuggestFinal json");
    assert!(
        !parsed.suggestions.is_empty(),
        "n-gram completions expected"
    );
    assert!(
        parsed
            .suggestions
            .iter()
            .all(|s| s.origin == Some(SuggestionOrigin::Ngram)),
        "fallback suggestions must be tagged Ngram"
    );
}
