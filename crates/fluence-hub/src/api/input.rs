// SPDX-License-Identifier: Apache-2.0

//! Input surface: `PUT /input/targets` (SPEC §4.A, D-4.1).
//!
//! A control UI declares the full target map of its surface here; the hub
//! stores it and seeds every new `/ws` input connection's selection engine
//! from it (the engine runs in [`crate::api::ws`]). Incremental updates flow as
//! `targets.patch` messages over the `input` WebSocket topic.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use fluence_protocol::input::TargetMap;

use crate::state::AppState;

/// `PUT /api/v1/input/targets` (control scope): declare a surface's full target
/// map. Replaces any previous declaration (v0 holds a single active surface).
/// Returns `204`.
pub async fn put_targets(State(state): State<AppState>, Json(map): Json<TargetMap>) -> StatusCode {
    state.set_input_targets(map);
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use fluence_protocol::input::{Target, TargetRole, Viewport};
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

    fn one_target_map() -> TargetMap {
        TargetMap {
            surface: "main".into(),
            viewport: Viewport { w: 100, h: 100 },
            targets: vec![Target {
                id: "key_e".into(),
                rect: serde_json::from_str("[0, 0, 100, 100]").expect("valid rect"),
                role: TargetRole::Key,
                label: Some("e".to_owned()),
                prior: None,
            }],
        }
    }

    #[tokio::test]
    async fn put_targets_stores_the_map_and_returns_204() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;

        let status = put_targets(State(state.clone()), Json(one_target_map())).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(state.input_targets(), Some(one_target_map()));
    }

    #[tokio::test]
    async fn a_later_put_replaces_the_previous_map() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;

        let _ = put_targets(State(state.clone()), Json(one_target_map())).await;
        let mut second = one_target_map();
        second.surface = "panel".into();
        let _ = put_targets(State(state.clone()), Json(second.clone())).await;

        assert_eq!(state.input_targets(), Some(second));
    }
}
