// SPDX-License-Identifier: Apache-2.0

//! Pairing handlers (SPEC §2.A): explicit window, single-use 8-digit
//! code, brute-force lockout, per-device scoped tokens.

use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use fluence_protocol::api::pair::{
    PairInfo, PairRequest, PairResponse, PairWindowRequest, PairWindowResponse,
};
use fluence_protocol::error::ErrorCode;
use tokio::time::Instant;

use crate::api::problem_response;
use crate::auth;
use crate::state::{AppState, PAIRING_MAX_ATTEMPTS, PAIRING_WINDOW_TTL, PairingWindow};

/// Constant-time equality of two pairing codes: the comparison does not
/// short-circuit on the first differing byte, so its timing leaks nothing about
/// how many leading digits matched (defence in depth on top of the 5-attempt
/// lockout). The code length is fixed (8 digits), so the length check leaks no
/// secret.
#[must_use]
fn codes_match(expected: &str, candidate: &str) -> bool {
    let (expected, candidate) = (expected.as_bytes(), candidate.as_bytes());
    if expected.len() != candidate.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in expected.iter().zip(candidate) {
        diff |= a ^ b;
    }
    diff == 0
}

/// `POST /api/v1/pair/window` (system scope): opens the 2-minute window.
/// Reopening replaces any previous window (one window at a time).
pub async fn open_window(
    State(state): State<AppState>,
    Json(request): Json<PairWindowRequest>,
) -> Response {
    let code = generate_code();
    // 120 s always fits a TimeDelta; the fallback is unreachable but keeps
    // the handler panic-free (clippy: no `# Panics` to document).
    let ttl = chrono::TimeDelta::from_std(PAIRING_WINDOW_TTL)
        .unwrap_or_else(|_| chrono::TimeDelta::seconds(120));
    let expires_at_utc = chrono::Utc::now() + ttl;
    let response = PairWindowResponse {
        code: code.clone(),
        expires_at: expires_at_utc,
    };

    *state.pairing() = Some(PairingWindow {
        code,
        expires_at: Instant::now() + PAIRING_WINDOW_TTL,
        expires_at_utc,
        attempts: 0,
        scope: request.scope,
    });
    state
        .journal(
            "pair.window_opened",
            None,
            Some(&format!("scope={:?}", request.scope)),
        )
        .await;
    Json(response).into_response()
}

/// `GET /pair/info` (public): what the pairing screen shows.
pub async fn pair_info(State(state): State<AppState>) -> Json<PairInfo> {
    let pairing_open = {
        let window = state.pairing();
        window
            .as_ref()
            .is_some_and(|w| Instant::now() < w.expires_at)
    };
    Json(PairInfo {
        api_version: 1,
        household_name: state.config().household_name.clone(),
        // Loopback embedded mode has no TLS; home mode fills this (PR C).
        ca_fingerprint: None,
        pairing_open,
    })
}

/// `POST /pair` (public, but only meaningful while the window is open):
/// exchanges the code for a scoped device token.
pub async fn pair_device(
    State(state): State<AppState>,
    Json(request): Json<PairRequest>,
) -> Response {
    // Decide under the lock; journal/store after releasing it.
    enum Decision {
        Closed,
        WrongCode {
            lockout: bool,
        },
        Accept {
            scope: fluence_protocol::api::pair::Scope,
        },
    }

    let decision = {
        let mut slot = state.pairing();
        match slot.as_mut() {
            None => Decision::Closed,
            Some(window) if Instant::now() >= window.expires_at => {
                *slot = None;
                Decision::Closed
            }
            Some(window) if !codes_match(&window.code, &request.code) => {
                window.attempts += 1;
                let lockout = window.attempts >= PAIRING_MAX_ATTEMPTS;
                if lockout {
                    // Burn the window: brute force costs a re-open.
                    *slot = None;
                }
                Decision::WrongCode { lockout }
            }
            Some(window) => {
                let scope = window.scope;
                // Single-use code: success closes the window.
                *slot = None;
                Decision::Accept { scope }
            }
        }
    };

    match decision {
        Decision::Closed => {
            state
                .journal("pair.rejected", None, Some("window closed"))
                .await;
            problem_response(ErrorCode::PairingWindowClosed, None)
        }
        Decision::WrongCode { lockout } => {
            if lockout {
                state
                    .journal(
                        "pair.lockout",
                        None,
                        Some("too many wrong codes; window burned"),
                    )
                    .await;
                problem_response(ErrorCode::RateLimited, None)
            } else {
                state
                    .journal("pair.rejected", None, Some("wrong code"))
                    .await;
                problem_response(ErrorCode::PairingCodeInvalid, None)
            }
        }
        Decision::Accept { scope } => {
            let token = auth::generate_token();
            let device_id = uuid::Uuid::new_v4().to_string();
            let inserted = state
                .store()
                .insert_device(fluence_store::NewDevice {
                    device_id: device_id.clone(),
                    token_hash: auth::token_hash(&token),
                    name: request.device_name.clone(),
                    kind: request.device_kind,
                    scope,
                })
                .await;
            match inserted {
                Ok(_) => {
                    state
                        .journal(
                            "device.paired",
                            Some(device_id),
                            Some(&format!("kind={:?} scope={scope:?}", request.device_kind)),
                        )
                        .await;
                    Json(PairResponse {
                        ca_cert: None,
                        device_token: token,
                        scope,
                    })
                    .into_response()
                }
                Err(error) => {
                    tracing::error!(%error, "device insert failed during pairing");
                    problem_response(ErrorCode::Internal, None)
                }
            }
        }
    }
}

/// Eight decimal digits, zero-padded, single use.
fn generate_code() -> String {
    let value: u32 = rand::random_range(0..100_000_000);
    format!("{value:08}")
}

#[cfg(test)]
mod tests {
    use super::{codes_match, generate_code};

    #[test]
    fn codes_match_accepts_equal_and_rejects_different() {
        assert!(codes_match("12345678", "12345678"));
        assert!(!codes_match("12345678", "12345679")); // last digit differs
        assert!(!codes_match("12345678", "02345678")); // first digit differs
        assert!(!codes_match("12345678", "1234567")); // shorter
        assert!(!codes_match("12345678", "123456789")); // longer
        assert!(codes_match("", ""));
    }

    #[test]
    fn generate_code_is_eight_digits() {
        for _ in 0..100 {
            let code = generate_code();
            assert_eq!(code.len(), 8, "code {code} is not 8 chars");
            assert!(
                code.chars().all(|c| c.is_ascii_digit()),
                "code {code} has non-digits"
            );
        }
    }
}
