// SPDX-License-Identifier: Apache-2.0

//! Device management (caregiver space, SPEC §7.C): revoke a paired device's
//! token. Listing arrives with the caregiver UI; revocation is the security
//! primitive — a lost or compromised device is cut off immediately.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use fluence_protocol::error::ErrorCode;

use crate::api::problem_response;
use crate::state::AppState;

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
}
