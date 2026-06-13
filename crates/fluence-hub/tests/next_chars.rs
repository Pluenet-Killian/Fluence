// SPDX-License-Identifier: Apache-2.0

//! T4 — `/next-chars`: it serves the LLM backend when available and **degrades
//! to the n-gram fallback** when the backend is unavailable (D-2.6 « le clavier
//! parle toujours »), always 200, never a 5xx. Driven through the real router
//! (no spawned server) with an injected backend.

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
use fluence_protocol::api::suggest::NextCharsResponse;
use fluence_store::{KeySource, NewDevice, Store, StoreConfig};
use tower::ServiceExt;

const TOKEN: &str = "flt_nextchars_test";

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
    AppState::new_with(
        config,
        store,
        EventBus::new(),
        engine,
        Arc::new(fallback),
        Arc::new(fluence_voice::UnavailableVoice),
    )
}

async fn next_chars(state: AppState, prefix: &str) -> (StatusCode, NextCharsResponse) {
    let uri = format!("/api/v1/sessions/s1/next-chars?prefix={prefix}");
    let request = Request::get(uri)
        .header("X-Fluence-Token", TOKEN)
        .body(Body::empty())
        .expect("request builds");
    let response = build_router(state)
        .oneshot(request)
        .await
        .expect("router responds");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body");
    let parsed = serde_json::from_slice(&bytes).expect("NextCharsResponse json");
    (status, parsed)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn next_chars_uses_the_llm_backend_when_available() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = hub_with(Arc::new(StubBackend::new("")), NgramModel::new(), &dir).await;
    let (status, response) = next_chars(state, "bonjou").await;

    assert_eq!(status, StatusCode::OK);
    // The stub serves its fixed distribution (e, a, s, t) — the LLM path won.
    assert!(
        response.dist.iter().any(|c| c.ch == 'e'),
        "stub distribution expected"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn next_chars_degrades_to_ngram_when_backend_unavailable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut fallback = NgramModel::new();
    fallback.train("bonjour bonjour bonsoir");
    let state = hub_with(Arc::new(UnavailableBackend), fallback, &dir).await;
    let (status, response) = next_chars(state, "bon").await;

    // Graceful degradation: 200 with the n-gram's distribution, never a 5xx.
    assert_eq!(status, StatusCode::OK);
    let chars: Vec<char> = response.dist.iter().map(|c| c.ch).collect();
    // After « bon »: « bonjour » → 'j', « bonsoir » → 's'.
    assert!(
        chars.contains(&'j') || chars.contains(&'s'),
        "expected the n-gram fallback distribution, got {chars:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn next_chars_is_never_5xx_even_with_an_empty_fallback() {
    // The default production state (no worker, empty fallback): the endpoint
    // still answers 200 with an empty distribution — the keyboard never breaks.
    let dir = tempfile::tempdir().expect("tempdir");
    let state = hub_with(Arc::new(UnavailableBackend), NgramModel::new(), &dir).await;
    let (status, response) = next_chars(state, "xyz").await;

    assert_eq!(status, StatusCode::OK);
    assert!(response.dist.is_empty());
}
