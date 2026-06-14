// SPDX-License-Identifier: Apache-2.0

//! Device management (caregiver space, SPEC §7.C): revoke a paired device's
//! token. Listing arrives with the caregiver UI; revocation is the security
//! primitive — a lost or compromised device is cut off immediately.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use fluence_protocol::api::devices::{DeviceInfo, DeviceList};
use fluence_protocol::error::ErrorCode;

use crate::api::problem_response;
use crate::state::AppState;

/// `GET /api/v1/devices` (care scope): lists every paired device, revoked
/// included, oldest first. **No token, ever** — only metadata the caregiver
/// needs to recognise and, if needed, revoke a device.
pub async fn list(State(state): State<AppState>) -> Response {
    match state.store().list_devices().await {
        Ok(devices) => {
            let devices = devices
                .into_iter()
                .map(|d| DeviceInfo {
                    id: d.device_id,
                    name: d.name,
                    kind: d.kind,
                    scope: d.scope,
                    created_at: d.created_at,
                    revoked_at: d.revoked_at,
                })
                .collect();
            Json(DeviceList { devices }).into_response()
        }
        Err(error) => {
            tracing::error!(%error, "device list failed");
            problem_response(ErrorCode::Internal, None)
        }
    }
}

/// `DELETE /api/v1/devices/{id}` (care scope): revokes the device's token.
///
/// **Idempotent**: revoking an unknown or already-revoked device is a `204`
/// no-op, so the response leaks nothing about which device ids exist. The
/// revocation is journaled (the id is access metadata, not P0).
pub async fn revoke(State(state): State<AppState>, Path(device_id): Path<String>) -> Response {
    match state.store().revoke_device(device_id.clone()).await {
        Ok(()) => {
            state.journal("device.revoked", Some(device_id), None).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(error) => {
            tracing::error!(%error, "device revoke failed");
            problem_response(ErrorCode::Internal, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use fluence_protocol::api::pair::{DeviceKind, Scope};
    use fluence_store::{KeySource, NewDevice, Store, StoreConfig};

    use super::*;
    use crate::auth;
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
    async fn revoking_a_device_cuts_off_its_token() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;
        let token = auth::generate_token();
        state
            .store()
            .insert_device(NewDevice {
                device_id: "dev-1".to_owned(),
                token_hash: auth::token_hash(&token),
                name: "tablette".to_owned(),
                kind: DeviceKind::Tablet,
                scope: Scope::Control,
            })
            .await
            .expect("insert");
        // The token authenticates before revocation…
        assert!(
            state
                .store()
                .device_by_token_hash(auth::token_hash(&token))
                .await
                .expect("lookup")
                .is_some()
        );

        let status = revoke(State(state.clone()), Path("dev-1".to_owned())).await;
        assert_eq!(status.status(), StatusCode::NO_CONTENT);

        // …and is rejected after.
        assert!(
            state
                .store()
                .device_by_token_hash(auth::token_hash(&token))
                .await
                .expect("lookup")
                .is_none(),
            "a revoked token must no longer authenticate"
        );
    }

    #[tokio::test]
    async fn revoking_an_unknown_device_is_an_idempotent_no_op() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;
        let status = revoke(State(state), Path("does-not-exist".to_owned())).await;
        assert_eq!(status.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn list_returns_devices_with_metadata_and_revocation_state() {
        use axum::body::to_bytes;

        let dir = tempfile::tempdir().expect("tempdir");
        let state = test_state(&dir).await;
        for (id, name) in [("dev-1", "tablette"), ("dev-2", "portable")] {
            state
                .store()
                .insert_device(NewDevice {
                    device_id: id.to_owned(),
                    token_hash: auth::token_hash(&format!("tok-{id}")),
                    name: name.to_owned(),
                    kind: DeviceKind::Tablet,
                    scope: Scope::Control,
                })
                .await
                .expect("insert");
        }
        state
            .store()
            .revoke_device("dev-2".to_owned())
            .await
            .expect("revoke");

        let response = list(State(state)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body");
        let list: DeviceList = serde_json::from_slice(&bytes).expect("DeviceList json");

        assert_eq!(list.devices.len(), 2);
        let revoked: Vec<_> = list
            .devices
            .iter()
            .filter(|d| d.revoked_at.is_some())
            .collect();
        assert_eq!(revoked.len(), 1, "the revoked device stays listed");
        assert_eq!(revoked[0].id, "dev-2");
        // No token field exists on the wire type — only metadata is exposed.
        let raw = String::from_utf8(bytes.to_vec()).expect("utf8");
        assert!(!raw.contains("token"), "no token in the device list");
    }
}
