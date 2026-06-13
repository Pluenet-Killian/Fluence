// SPDX-License-Identifier: Apache-2.0

//! T1 — every fixture under `tests/fixtures/` deserializes into its type
//! and serializes back to the exact same JSON (PLAN Phase 1 tests).
//!
//! The fixtures are the SPEC's own wire examples (§4.A, §5.A, §5.B),
//! verbatim where the SPEC gives one — they double as living documentation
//! of the canonical wire format.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use fluence_protocol::api::memory::MemoryItem;
use fluence_protocol::api::suggest::{SuggestEvent, SuggestRequest};
use fluence_protocol::api::system::HealthResponse;
use fluence_protocol::error::Problem;
use fluence_protocol::input::{PointerSample, SelectionEvent, SwitchEvent, TargetMap};
use fluence_protocol::ws::ServerFrame;

/// Loads a fixture, deserializes it as `T`, serializes it back, and
/// asserts semantic JSON equality (key order does not matter; values and
/// presence do).
fn assert_roundtrip<T: Serialize + DeserializeOwned>(fixture: &str) {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let original: Value = serde_json::from_str(&raw).expect("fixture is valid JSON");
    let typed: T = serde_json::from_value(original.clone())
        .unwrap_or_else(|e| panic!("{fixture} does not deserialize: {e}"));
    let back = serde_json::to_value(&typed).expect("serializes back");
    assert_eq!(original, back, "{fixture} round-trip changed the JSON");
}

/// Like [`assert_roundtrip`] but for fixtures holding an array of `T`.
fn assert_roundtrip_each<T: Serialize + DeserializeOwned>(fixture: &str) {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let items: Vec<Value> = serde_json::from_str(&raw).expect("fixture is a JSON array");
    for (index, original) in items.into_iter().enumerate() {
        let typed: T = serde_json::from_value(original.clone())
            .unwrap_or_else(|e| panic!("{fixture}[{index}] does not deserialize: {e}"));
        let back = serde_json::to_value(&typed).expect("serializes back");
        assert_eq!(
            original, back,
            "{fixture}[{index}] round-trip changed the JSON"
        );
    }
}

// The `k`-tagged sensor messages are fixtures of the *enveloped* form; the
// bare structs are exercised through `InputClientMessage`.
#[test]
fn pointer_sample_spec_example() {
    assert_roundtrip::<fluence_protocol::input::InputClientMessage>("pointer_sample.json");
    // The same fixture, read as the bare sample (no `k`), must also work:
    // serde's internally-tagged enums ignore the tag field on inner types.
    assert_roundtrip_inner::<PointerSample>("pointer_sample.json");
}

/// Reads a `k`-tagged fixture as its bare payload type (dropping `k`).
fn assert_roundtrip_inner<T: Serialize + DeserializeOwned>(fixture: &str) {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let mut original: Value = serde_json::from_str(&raw).expect("fixture is valid JSON");
    original
        .as_object_mut()
        .expect("object fixture")
        .remove("k");
    let typed: T = serde_json::from_value(original.clone())
        .unwrap_or_else(|e| panic!("{fixture} (bare) does not deserialize: {e}"));
    let back = serde_json::to_value(&typed).expect("serializes back");
    assert_eq!(
        original, back,
        "{fixture} (bare) round-trip changed the JSON"
    );
}

#[test]
fn switch_event_spec_example() {
    assert_roundtrip::<fluence_protocol::input::InputClientMessage>("switch_event.json");
    assert_roundtrip_inner::<SwitchEvent>("switch_event.json");
}

#[test]
fn target_map_spec_example() {
    assert_roundtrip::<TargetMap>("target_map.json");
}

#[test]
fn selection_events_spec_examples() {
    assert_roundtrip_each::<SelectionEvent>("selection_events.json");
}

#[test]
fn suggest_request_spec_example() {
    assert_roundtrip::<SuggestRequest>("suggest_request.json");
}

#[test]
fn suggest_sse_events() {
    assert_roundtrip_each::<SuggestEvent>("suggest_events.json");
}

#[test]
fn memory_item_spec_example() {
    assert_roundtrip::<MemoryItem>("memory_item.json");
}

#[test]
fn problem_document() {
    assert_roundtrip::<Problem>("problem.json");
}

#[test]
fn server_frames() {
    assert_roundtrip_each::<ServerFrame>("server_frames.json");
}

#[test]
fn health_response() {
    assert_roundtrip::<HealthResponse>("health_response.json");
}
