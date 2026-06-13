// SPDX-License-Identifier: Apache-2.0

//! Device tokens and scope enforcement (SPEC §2.A, D-2.4).
//!
//! - Tokens: 32 random bytes, `flt_` + base64url. The store keeps only
//!   their SHA-256 (ADR-0005 §6).
//! - Every route outside `/pair`, `/pair/info` requires `X-Fluence-Token`
//!   (`/ws` reads the `token` query parameter instead — browser
//!   `WebSocket` cannot set headers, ADR-0004 §1).
//! - Scope model: `system` passes everywhere; other scopes must be listed
//!   by the route (the route table is asserted against the
//!   `fluence-protocol` registry by test).

use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use base64::Engine;
use fluence_protocol::api::pair::Scope;
use sha2::{Digest, Sha256};

use crate::state::AppState;

/// Authenticated request context, inserted into request extensions by
/// [`require_token`].
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// The authenticated device.
    pub device_id: String,
    /// Its granted scope.
    pub scope: Scope,
}

/// Generates a fresh bearer token (returned to the device once, at
/// pairing; never stored).
#[must_use]
pub fn generate_token() -> String {
    let bytes: [u8; 32] = rand::random();
    format!(
        "flt_{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

/// SHA-256 of a presented token — the only form the store ever sees.
#[must_use]
pub fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// Checks whether `scope` may call a route allowing `allowed`.
/// `system` is implicitly allowed everywhere (SPEC §2.A scope table).
#[must_use]
pub fn scope_allows(scope: Scope, allowed: &[Scope]) -> bool {
    scope == Scope::System || allowed.contains(&scope)
}

/// Extracts and verifies the device token; inserts [`AuthContext`].
/// Uniform `401` (`token_missing` / `token_invalid`) otherwise — the
/// response never reveals whether a token exists (SPEC §2.A).
pub async fn require_token(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let presented = bearer_from_headers(request.headers());
    let Some(presented) = presented else {
        return crate::api::problem_response(
            fluence_protocol::error::ErrorCode::TokenMissing,
            None,
        );
    };
    match state
        .store()
        .device_by_token_hash(token_hash(&presented))
        .await
    {
        Ok(Some(device)) => {
            request.extensions_mut().insert(AuthContext {
                device_id: device.device_id,
                scope: device.scope,
            });
            next.run(request).await
        }
        Ok(None) => {
            state
                .journal("auth.rejected", None, Some("invalid token"))
                .await;
            crate::api::problem_response(fluence_protocol::error::ErrorCode::TokenInvalid, None)
        }
        Err(error) => {
            tracing::error!(%error, "store unavailable during auth");
            crate::api::problem_response(fluence_protocol::error::ErrorCode::Internal, None)
        }
    }
}

/// Reads `X-Fluence-Token` (the only accepted header form).
fn bearer_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("X-Fluence-Token")
        .and_then(|v| v.to_str().ok())
        .map(ToOwned::to_owned)
}

/// Builds a middleware enforcing `allowed` scopes on a sub-router.
/// Must run *after* [`require_token`] (it reads the [`AuthContext`]).
pub fn require_scope(
    allowed: &'static [Scope],
) -> impl Fn(Request, Next) -> std::pin::Pin<Box<dyn Future<Output = Response> + Send>>
+ Clone
+ Send
+ 'static {
    move |request: Request, next: Next| {
        Box::pin(async move {
            let context = request.extensions().get::<AuthContext>();
            match context {
                Some(context) if scope_allows(context.scope, allowed) => next.run(request).await,
                Some(context) => {
                    let detail = format!(
                        "route requires one of {allowed:?}; token has scope {:?}",
                        context.scope
                    );
                    crate::api::problem_response(
                        fluence_protocol::error::ErrorCode::ScopeInsufficient,
                        Some(detail),
                    )
                }
                None => crate::api::problem_response(
                    fluence_protocol::error::ErrorCode::TokenMissing,
                    None,
                ),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_and_prefixed() {
        let a = generate_token();
        let b = generate_token();
        assert_ne!(a, b);
        assert!(a.starts_with("flt_"));
        assert!(a.len() > 40, "32 bytes of entropy expected");
    }

    #[test]
    fn hash_is_stable_and_token_dependent() {
        let token = generate_token();
        assert_eq!(token_hash(&token), token_hash(&token));
        assert_ne!(token_hash(&token), token_hash("flt_other"));
    }

    #[test]
    fn system_scope_passes_everywhere_others_only_where_listed() {
        // SPEC §2.A scope table, exhaustively.
        for scope in [Scope::Display, Scope::Control, Scope::Care] {
            assert!(scope_allows(scope, &[scope]), "{scope:?} on its own route");
            assert!(
                !scope_allows(scope, &[]),
                "{scope:?} on a system-only route"
            );
        }
        assert!(scope_allows(Scope::System, &[]));
        assert!(scope_allows(Scope::System, &[Scope::Display]));
        assert!(!scope_allows(
            Scope::Display,
            &[Scope::Control, Scope::Care]
        ));
    }
}
