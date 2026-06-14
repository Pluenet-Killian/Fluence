// SPDX-License-Identifier: Apache-2.0

//! HTTP API of the hub (SPEC §5.A) — the mounted surface grows phase by phase.
//!
//! The router is built from [`MOUNTED`], a local table of (method, path,
//! scopes); a test asserts it against the `fluence-protocol` registry
//! (same path ⇒ same scopes, and mounted ⊆ declared) so the implementation
//! cannot drift from the contract. Routes of unbuilt capabilities are simply
//! not mounted yet: a 404 is honest — the capability does not exist.

pub mod devices;
pub mod input;
pub mod pair;
pub mod sessions;
pub mod suggest;
pub mod system;
pub mod voice;
pub mod ws;

use axum::Router;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use fluence_protocol::api::pair::Scope;
use fluence_protocol::error::{ErrorCode, Problem};

use crate::auth;
use crate::state::AppState;

/// One mounted route (the implementation-side mirror of `RouteSpec`).
pub struct MountedRoute {
    /// HTTP method (lowercase, as in the registry).
    pub method: &'static str,
    /// Path with `{param}` placeholders, registry style.
    pub path: &'static str,
    /// Allowed non-system scopes (`&[]` = system-only).
    pub scopes: &'static [Scope],
}

/// The mounted surface. Kept in sync with [`build_router`] by proximity and
/// asserted against the registry by test.
pub const MOUNTED: &[MountedRoute] = &[
    MountedRoute {
        method: "post",
        path: "/pair",
        scopes: &[],
    },
    MountedRoute {
        method: "get",
        path: "/pair/info",
        scopes: &[],
    },
    MountedRoute {
        method: "post",
        path: "/api/v1/pair/window",
        scopes: &[],
    },
    MountedRoute {
        method: "post",
        path: "/api/v1/sessions",
        scopes: &[Scope::Control],
    },
    MountedRoute {
        method: "delete",
        path: "/api/v1/sessions/{id}",
        scopes: &[Scope::Control],
    },
    MountedRoute {
        method: "put",
        path: "/api/v1/sessions/{id}/draft",
        scopes: &[Scope::Control],
    },
    MountedRoute {
        method: "get",
        path: "/api/v1/sessions/{id}/draft",
        scopes: &[Scope::Control],
    },
    MountedRoute {
        method: "get",
        path: "/api/v1/sessions/{id}/next-chars",
        scopes: &[Scope::Control],
    },
    MountedRoute {
        method: "post",
        path: "/api/v1/sessions/{id}/suggest",
        scopes: &[Scope::Control],
    },
    MountedRoute {
        method: "post",
        path: "/api/v1/system/emergency",
        scopes: &[Scope::Control],
    },
    MountedRoute {
        method: "put",
        path: "/api/v1/input/targets",
        scopes: &[Scope::Control],
    },
    MountedRoute {
        method: "post",
        path: "/api/v1/voice/speak",
        scopes: &[Scope::Control],
    },
    MountedRoute {
        method: "get",
        path: "/api/v1/voice/voices",
        scopes: &[Scope::Control, Scope::Care],
    },
    MountedRoute {
        method: "get",
        path: "/api/v1/system/health",
        scopes: &[Scope::Display, Scope::Control, Scope::Care],
    },
    MountedRoute {
        method: "get",
        path: "/api/v1/system/capabilities",
        scopes: &[Scope::Display, Scope::Control, Scope::Care],
    },
    MountedRoute {
        method: "get",
        path: "/api/v1/system/journal",
        scopes: &[Scope::Care],
    },
    MountedRoute {
        method: "get",
        path: "/api/v1/devices",
        scopes: &[Scope::Care],
    },
    MountedRoute {
        method: "delete",
        path: "/api/v1/devices/{id}",
        scopes: &[Scope::Care],
    },
    MountedRoute {
        method: "get",
        path: "/ws",
        scopes: &[Scope::Display, Scope::Control, Scope::Care],
    },
];

/// Builds the complete Phase 2 router.
pub fn build_router(state: AppState) -> Router {
    // Public: the only tokenless routes (SPEC §2.A).
    let public = Router::new()
        .route("/pair", post(pair::pair_device))
        .route("/pair/info", get(pair::pair_info));

    // System-only (the embedded UI / local CLI).
    let system_only = Router::new()
        .route("/api/v1/pair/window", post(pair::open_window))
        .route_layer(axum::middleware::from_fn(auth::require_scope(&[])));

    // Control scope: composing.
    let control = Router::new()
        .route("/api/v1/sessions", post(sessions::create_session))
        .route("/api/v1/sessions/{id}", delete(sessions::delete_session))
        .route(
            "/api/v1/sessions/{id}/draft",
            put(sessions::put_draft).get(sessions::get_draft),
        )
        .route("/api/v1/sessions/{id}/next-chars", get(suggest::next_chars))
        .route(
            "/api/v1/sessions/{id}/suggest",
            post(suggest::stream_suggest),
        )
        .route("/api/v1/system/emergency", post(system::emergency))
        .route("/api/v1/input/targets", put(input::put_targets))
        .route("/api/v1/voice/speak", post(voice::speak))
        .route_layer(axum::middleware::from_fn(auth::require_scope(&[
            Scope::Control,
        ])));

    // Control + caregiver: the installed-voice list (SPEC §5.A scope table).
    let voice_list = Router::new()
        .route("/api/v1/voice/voices", get(voice::voices))
        .route_layer(axum::middleware::from_fn(auth::require_scope(&[
            Scope::Control,
            Scope::Care,
        ])));

    // Read-only system surface: every authenticated scope.
    let observers = Router::new()
        .route("/api/v1/system/health", get(system::health))
        .route("/api/v1/system/capabilities", get(system::capabilities))
        .route_layer(axum::middleware::from_fn(auth::require_scope(&[
            Scope::Display,
            Scope::Control,
            Scope::Care,
        ])));

    // Caregiver-only surface (the access journal + device revocation —
    // SPEC §2.A/§7.C).
    let care = Router::new()
        .route("/api/v1/system/journal", get(system::journal))
        .route("/api/v1/devices", get(devices::list))
        .route("/api/v1/devices/{id}", delete(devices::revoke))
        .route_layer(axum::middleware::from_fn(auth::require_scope(&[
            Scope::Care,
        ])));

    let authed = Router::new()
        .merge(system_only)
        .merge(control)
        .merge(voice_list)
        .merge(observers)
        .merge(care)
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::require_token,
        ));

    // /ws authenticates inside the handler (token travels as a query
    // parameter — ADR-0004 §1).
    let websocket = Router::new().route("/ws", get(ws::upgrade));

    let mut router = Router::new().merge(public).merge(authed).merge(websocket);

    // Serve the web composer as a same-origin PWA when configured (PLAN 5.3):
    // a fallback under `/`, after the API routes, with an SPA fallback to
    // `index.html` so client-side routes resolve. Same origin ⇒ no CORS.
    if let Some(web_dir) = state.config().web_dir.clone() {
        let index = web_dir.join("index.html");
        let serve = tower_http::services::ServeDir::new(web_dir)
            .fallback(tower_http::services::ServeFile::new(index));
        router = router.fallback_service(serve);
    }

    router
        // Explicit request-body ceiling (G7): do not depend on axum's
        // implicit default. Generous for a JSON draft (text itself is
        // capped far lower in `put_draft`, F09) yet bounds any single
        // request a local device can send.
        .layer(axum::extract::DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        // CORS: strict allowlist — empty (the composer is same-origin), so any
        // cross-origin browser call is refused (SPEC §2.A).
        .layer(tower_http::cors::CorsLayer::new())
        .with_state(state)
}

/// Explicit hub-wide cap on a single request body (G7). Comfortably above
/// a JSON-encoded draft at the text limit (`state::MAX_DRAFT_TEXT_BYTES`,
/// 64 KiB) even when every character escapes, while replacing axum's
/// implicit default with a documented, stable bound.
const MAX_REQUEST_BODY_BYTES: usize = 512 * 1024;

/// Builds the RFC 9457 response for `code` (uniform error shape).
#[must_use]
pub fn problem_response(code: ErrorCode, detail: Option<String>) -> Response {
    let mut problem = Problem::from_code(code);
    if let Some(detail) = detail {
        problem = problem.with_detail(detail);
    }
    let status = StatusCode::from_u16(problem.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = serde_json::to_string(&problem).unwrap_or_else(|_| "{}".to_owned());
    Response::builder()
        .status(status)
        .header("content-type", "application/problem+json")
        .body(body.into())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

#[cfg(test)]
mod tests {
    use fluence_protocol::routes::{RouteAuth, routes};

    use super::*;

    /// The mounted surface is a subset of the contract registry, with
    /// identical auth requirements route by route.
    #[test]
    fn mounted_routes_match_the_registry() {
        for mounted in MOUNTED {
            let declared = routes()
                .iter()
                .find(|r| r.method.as_str() == mounted.method && r.path == mounted.path)
                .unwrap_or_else(|| {
                    panic!(
                        "{} {} is mounted but not declared",
                        mounted.method, mounted.path
                    )
                });
            match declared.auth {
                RouteAuth::Public => {
                    assert!(
                        mounted.scopes.is_empty(),
                        "{}: public route must mount scopeless",
                        mounted.path
                    );
                }
                RouteAuth::Scoped(declared_scopes) => {
                    assert_eq!(
                        mounted.scopes, declared_scopes,
                        "{}: mounted scopes differ from the registry",
                        mounted.path
                    );
                }
            }
        }
    }
}
