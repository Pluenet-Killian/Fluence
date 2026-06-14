// SPDX-License-Identifier: Apache-2.0

//! Declarative route registry — the API surface of SPEC §5.A as data.
//!
//! One place declares every route: method, path, allowed scopes, stability
//! level, request/response schemas. The `OpenAPI` document is generated from
//! it (`cargo xtask check-contracts`), and the hub (Phase 2) will assert
//! its router matches it — the registry cannot silently drift from either.

use crate::api::pair::Scope;

/// HTTP method of a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// GET — reads.
    Get,
    /// POST — creations and actions.
    Post,
    /// PUT — idempotent replacements.
    Put,
    /// DELETE — removals.
    Delete,
}

impl HttpMethod {
    /// Lowercase name, as used in `OpenAPI` path items.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "get",
            Self::Post => "post",
            Self::Put => "put",
            Self::Delete => "delete",
        }
    }
}

/// Contract stability level (PLAN task 1.3bis).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stability {
    /// A1 core — frozen early, breaking changes need a SPEC amendment.
    Stable,
    /// P2 domain — defined now, may still change while being built.
    Experimental,
}

impl Stability {
    /// Value of the `x-fluence-stability` `OpenAPI` extension.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Experimental => "experimental",
        }
    }
}

/// Response shape of a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseSpec {
    /// `application/json` body, schema by component name.
    Json(&'static str),
    /// `text/event-stream`; events follow the named schema.
    Sse(&'static str),
    /// Streamed audio (`audio/ogg; codecs=opus`).
    AudioStream,
    /// `204 No Content`.
    NoContent,
    /// WebSocket upgrade (documented; frames are in components).
    WebSocketUpgrade,
}

/// A query parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryParam {
    /// Parameter name.
    pub name: &'static str,
    /// Whether it must be present.
    pub required: bool,
    /// Human description.
    pub description: &'static str,
}

/// Authentication requirement of a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteAuth {
    /// No token at all — the two pairing routes only (SPEC §2.A).
    Public,
    /// Device token required. `system` always passes; the listed scopes
    /// pass too. `Scoped(&[])` therefore means **system-only**.
    Scoped(&'static [Scope]),
}

impl RouteAuth {
    /// Non-system scopes allowed on this route (`None` = public).
    #[must_use]
    pub fn allowed_scopes(self) -> Option<&'static [Scope]> {
        match self {
            Self::Public => None,
            Self::Scoped(scopes) => Some(scopes),
        }
    }
}

/// One declared route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteSpec {
    /// HTTP method.
    pub method: HttpMethod,
    /// Path under the hub root, `/api/v1` prefix included, `{param}` style.
    pub path: &'static str,
    /// One-line summary (`OpenAPI` `summary`).
    pub summary: &'static str,
    /// Authentication requirement.
    pub auth: RouteAuth,
    /// Stability level (`x-fluence-stability`).
    pub stability: Stability,
    /// Request-body schema (component name), if any.
    pub request: Option<&'static str>,
    /// Response shape.
    pub response: ResponseSpec,
    /// Query parameters.
    pub query: &'static [QueryParam],
}

/// The complete route registry (SPEC §5.A, order of the spec).
// One declarative table, not logic — its length is its completeness.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn routes() -> &'static [RouteSpec] {
    use HttpMethod::{Delete, Get, Post, Put};
    use ResponseSpec::{AudioStream, Json, NoContent, Sse, WebSocketUpgrade};
    use Scope::{Care, Control, Display};
    use Stability::{Experimental, Stable};

    &[
        RouteSpec {
            method: Post,
            path: "/pair",
            summary: "Pair a device (only while a pairing window is open)",
            auth: RouteAuth::Public,
            stability: Stable,
            request: Some("PairRequest"),
            response: Json("PairResponse"),
            query: &[],
        },
        RouteSpec {
            method: Get,
            path: "/pair/info",
            summary: "Pairing screen information",
            auth: RouteAuth::Public,
            stability: Stable,
            request: None,
            response: Json("PairInfo"),
            query: &[],
        },
        RouteSpec {
            method: Post,
            path: "/api/v1/pair/window",
            summary: "Open the pairing window (main UI only; 2 min, single-use code)",
            auth: RouteAuth::Scoped(&[]), // system-only (SPEC §2.A)
            stability: Stable,
            request: Some("PairWindowRequest"),
            response: Json("PairWindowResponse"),
            query: &[],
        },
        RouteSpec {
            method: Post,
            path: "/api/v1/sessions",
            summary: "Open a conversation session (warm KV-cache)",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Stable,
            request: None,
            response: Json("CreateSessionResponse"),
            query: &[],
        },
        RouteSpec {
            method: Delete,
            path: "/api/v1/sessions/{id}",
            summary: "Close a session",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Stable,
            request: None,
            response: NoContent,
            query: &[],
        },
        RouteSpec {
            method: Post,
            path: "/api/v1/sessions/{id}/turns",
            summary: "Ingest a conversation turn",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Stable,
            request: Some("Turn"),
            response: NoContent,
            query: &[],
        },
        RouteSpec {
            method: Post,
            path: "/api/v1/sessions/{id}/suggest",
            summary: "Stream suggestions (SSE, per-slot cancellation)",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Stable,
            request: Some("SuggestRequest"),
            response: Sse("SuggestEvent"),
            query: &[],
        },
        RouteSpec {
            method: Get,
            path: "/api/v1/sessions/{id}/next-chars",
            summary: "Next-character distribution on the warm KV",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Stable,
            request: None,
            response: Json("NextCharsResponse"),
            query: &[QueryParam {
                name: "prefix",
                required: true,
                description: "Draft prefix to condition the distribution on",
            }],
        },
        RouteSpec {
            method: Put,
            path: "/api/v1/sessions/{id}/draft",
            summary: "Synchronize the draft (continuous autosave)",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Stable,
            request: Some("Draft"),
            response: NoContent,
            query: &[],
        },
        RouteSpec {
            method: Get,
            path: "/api/v1/sessions/{id}/draft",
            summary: "Read the draft back (session resumption after a cut, SPEC §2.A)",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Stable,
            request: None,
            response: Json("Draft"),
            query: &[],
        },
        RouteSpec {
            method: Post,
            path: "/api/v1/memory/items",
            summary: "Create a memory item",
            auth: RouteAuth::Scoped(&[Control, Care]),
            stability: Experimental,
            request: Some("CreateMemoryItem"),
            response: Json("MemoryItem"),
            query: &[],
        },
        RouteSpec {
            method: Get,
            path: "/api/v1/memory/search",
            summary: "Search personal memory (ACL-filtered)",
            auth: RouteAuth::Scoped(&[Control, Care]),
            stability: Experimental,
            request: None,
            response: Json("MemorySearchResponse"),
            query: &[QueryParam {
                name: "q",
                required: true,
                description: "Free-text query (hybrid BM25 + vector)",
            }],
        },
        RouteSpec {
            method: Delete,
            path: "/api/v1/memory/items/{id}",
            summary: "Delete a memory item",
            auth: RouteAuth::Scoped(&[Control, Care]),
            stability: Experimental,
            request: None,
            response: NoContent,
            query: &[],
        },
        RouteSpec {
            method: Get,
            path: "/api/v1/memory/pending",
            summary: "Validation queue of learned candidates",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Experimental,
            request: None,
            response: Json("PendingResponse"),
            query: &[],
        },
        RouteSpec {
            method: Post,
            path: "/api/v1/memory/pending/{id}/accept",
            summary: "Accept a learned candidate into memory",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Experimental,
            request: None,
            response: NoContent,
            query: &[],
        },
        RouteSpec {
            method: Post,
            path: "/api/v1/memory/pending/{id}/reject",
            summary: "Reject a learned candidate",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Experimental,
            request: None,
            response: NoContent,
            query: &[],
        },
        RouteSpec {
            method: Post,
            path: "/api/v1/memory/forget",
            summary: "Semantic forgetting: list candidates to confirm",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Experimental,
            request: Some("ForgetRequest"),
            response: Json("ForgetCandidates"),
            query: &[],
        },
        RouteSpec {
            method: Post,
            path: "/api/v1/voice/speak",
            summary: "Vocalize text (P0 scheduler priority, streamed Opus)",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Stable,
            request: Some("SpeakRequest"),
            response: AudioStream,
            query: &[],
        },
        RouteSpec {
            method: Get,
            path: "/api/v1/voice/voices",
            summary: "List installed voices",
            auth: RouteAuth::Scoped(&[Control, Care]),
            stability: Stable,
            request: None,
            response: Json("VoicesResponse"),
            query: &[],
        },
        RouteSpec {
            method: Post,
            path: "/api/v1/asr/consent",
            summary: "Obtain a journaled ASR consent token (explicit UI action)",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Experimental,
            request: None,
            response: Json("ConsentResponse"),
            query: &[],
        },
        RouteSpec {
            method: Post,
            path: "/api/v1/asr/listening",
            summary: "Start/stop partner-speech listening (consent required)",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Experimental,
            request: Some("ListeningRequest"),
            response: NoContent,
            query: &[],
        },
        RouteSpec {
            method: Get,
            path: "/api/v1/profiles/{id}",
            summary: "Read a profile",
            auth: RouteAuth::Scoped(&[Control, Care]),
            stability: Experimental,
            request: None,
            response: Json("Profile"),
            query: &[],
        },
        RouteSpec {
            method: Put,
            path: "/api/v1/profiles/{id}",
            summary: "Replace a profile",
            auth: RouteAuth::Scoped(&[Control, Care]),
            stability: Experimental,
            request: Some("Profile"),
            response: NoContent,
            query: &[],
        },
        RouteSpec {
            method: Put,
            path: "/api/v1/input/targets",
            summary: "Declare the full target map of a surface",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Stable,
            request: Some("TargetMap"),
            response: NoContent,
            query: &[],
        },
        RouteSpec {
            method: Get,
            path: "/api/v1/system/health",
            summary: "Worker states, models, rolling latencies",
            auth: RouteAuth::Scoped(&[Display, Control, Care]),
            stability: Stable,
            request: None,
            response: Json("HealthResponse"),
            query: &[],
        },
        RouteSpec {
            method: Get,
            path: "/api/v1/system/capabilities",
            summary: "Installation tier and available features",
            auth: RouteAuth::Scoped(&[Display, Control, Care]),
            stability: Stable,
            request: None,
            response: Json("CapabilitiesResponse"),
            query: &[],
        },
        RouteSpec {
            method: Get,
            path: "/api/v1/system/journal",
            summary: "Local access journal (caregiver space; metadata only)",
            auth: RouteAuth::Scoped(&[Care]),
            stability: Stable,
            request: None,
            response: Json("AccessJournalResponse"),
            query: &[QueryParam {
                name: "limit",
                required: false,
                description: "Maximum entries to return (newest first; default 100)",
            }],
        },
        RouteSpec {
            method: Delete,
            path: "/api/v1/devices/{id}",
            summary: "Revoke a paired device's token (caregiver space; SPEC §7.C)",
            auth: RouteAuth::Scoped(&[Care]),
            stability: Stable,
            request: None,
            response: NoContent,
            query: &[],
        },
        RouteSpec {
            method: Post,
            path: "/api/v1/system/emergency",
            summary: "Raise or clear the emergency alert (double-confirmed; D-7.4)",
            auth: RouteAuth::Scoped(&[Control]),
            stability: Stable,
            request: Some("EmergencyRequest"),
            response: NoContent,
            query: &[],
        },
        RouteSpec {
            method: Get,
            path: "/ws",
            summary: "WebSocket upgrade — topics via ?topics=…&v=1 (scope-filtered)",
            auth: RouteAuth::Scoped(&[Display, Control, Care]),
            stability: Stable,
            request: None,
            response: WebSocketUpgrade,
            query: &[
                QueryParam {
                    name: "topics",
                    required: true,
                    description: "Comma-separated topic list (input,asr,suggest,voice,system)",
                },
                QueryParam {
                    name: "v",
                    required: true,
                    description: "Input protocol version (currently 1)",
                },
                QueryParam {
                    name: "token",
                    required: true,
                    description: "Device token — browser WebSocket cannot set the \
                                  X-Fluence-Token header (ADR-0004)",
                },
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn paths_are_unique_per_method() {
        let mut seen = HashSet::new();
        for route in routes() {
            assert!(
                seen.insert((route.method.as_str(), route.path)),
                "duplicate route: {} {}",
                route.method.as_str(),
                route.path
            );
        }
    }

    #[test]
    fn only_the_two_pairing_routes_are_tokenless() {
        // SPEC §2.A: every other route requires a scoped token.
        let tokenless: Vec<_> = routes()
            .iter()
            .filter(|r| r.auth == RouteAuth::Public)
            .map(|r| r.path)
            .collect();
        assert_eq!(tokenless, ["/pair", "/pair/info"]);
    }

    #[test]
    fn pair_window_is_system_only() {
        let window = routes()
            .iter()
            .find(|r| r.path == "/api/v1/pair/window")
            .expect("declared");
        assert_eq!(window.auth, RouteAuth::Scoped(&[]));
    }

    #[test]
    fn a1_core_routes_are_stable_and_p2_domains_experimental() {
        // PLAN task 1.3bis: freeze early what we build early.
        for route in routes() {
            let expected = if route.path.contains("/memory/")
                || route.path.contains("/asr/")
                || route.path.contains("/profiles/")
            {
                Stability::Experimental
            } else {
                Stability::Stable
            };
            assert_eq!(route.stability, expected, "{}", route.path);
        }
    }

    #[test]
    fn api_routes_live_under_the_versioned_prefix() {
        // SPEC §5.A: `/api/v1` prefix; only pairing and the WS upgrade
        // live outside it.
        for route in routes() {
            let exempt = route.path.starts_with("/pair") || route.path == "/ws";
            assert!(
                route.path.starts_with("/api/v1/") || exempt,
                "{} must be under /api/v1",
                route.path
            );
        }
    }
}
