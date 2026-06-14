// SPDX-License-Identifier: Apache-2.0

//! Contract generation: JSON Schema goldens and the `OpenAPI` 3.1 document
//! (SPEC §2.B, PLAN task 1.3).
//!
//! `cargo xtask check-contracts` drives this module: it writes
//! `schemas/<Type>.json` (one self-contained schema per root type, the
//! reviewable goldens) and `schemas/openapi.json` (the assembled document),
//! then fails CI when the committed files differ from what the code
//! generates — contract drift becomes a build error.

use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, SchemaGenerator};
use serde_json::{Map, Value, json};

use crate::routes::{ResponseSpec, RouteSpec, routes};

/// Applies a callback macro to every root type of the contract — the single
/// authoritative list. Standalone goldens and `OpenAPI` components both
/// iterate it, so they can never disagree on coverage.
macro_rules! for_each_root_type {
    ($apply:ident) => {
        $apply!(crate::input::InputClientMessage);
        $apply!(crate::input::SelectionEvent);
        $apply!(crate::input::TargetMap);
        $apply!(crate::ws::ClientFrame);
        $apply!(crate::ws::ServerFrame);
        $apply!(crate::error::Problem);
        $apply!(crate::api::pair::PairRequest);
        $apply!(crate::api::pair::PairResponse);
        $apply!(crate::api::pair::PairInfo);
        $apply!(crate::api::pair::PairWindowRequest);
        $apply!(crate::api::pair::PairWindowResponse);
        $apply!(crate::api::sessions::CreateSessionResponse);
        $apply!(crate::api::sessions::Turn);
        $apply!(crate::api::sessions::Draft);
        $apply!(crate::api::suggest::SuggestRequest);
        $apply!(crate::api::suggest::SuggestEvent);
        $apply!(crate::api::suggest::NextCharsResponse);
        $apply!(crate::api::memory::MemoryItem);
        $apply!(crate::api::memory::CreateMemoryItem);
        $apply!(crate::api::memory::MemorySearchResponse);
        $apply!(crate::api::memory::PendingResponse);
        $apply!(crate::api::memory::ForgetRequest);
        $apply!(crate::api::memory::ForgetCandidates);
        $apply!(crate::api::voice::SpeakRequest);
        $apply!(crate::api::voice::VoicesResponse);
        $apply!(crate::api::asr::ConsentResponse);
        $apply!(crate::api::asr::ListeningRequest);
        $apply!(crate::api::profiles::Profile);
        $apply!(crate::api::system::HealthResponse);
        $apply!(crate::api::system::CapabilitiesResponse);
        $apply!(crate::api::system::AccessJournalResponse);
        $apply!(crate::api::system::EmergencyRequest);
        $apply!(crate::api::devices::DeviceList);
    };
}

/// One self-contained JSON Schema (2020-12) per root type — the goldens
/// committed under `schemas/`. Each file is readable on its own (`$defs`
/// inlined), which is what makes contract diffs reviewable.
#[must_use]
pub fn standalone_schemas() -> Vec<(String, Value)> {
    fn one<T: JsonSchema>(out: &mut Vec<(String, Value)>) {
        let mut generator = SchemaSettings::draft2020_12().into_generator();
        let schema = generator.root_schema_for::<T>();
        out.push((T::schema_name().into_owned(), schema.to_value()));
    }

    let mut out = Vec::new();
    macro_rules! apply {
        ($ty:ty) => {
            one::<$ty>(&mut out)
        };
    }
    for_each_root_type!(apply);
    out
}

/// All component schemas, generated with `#/components/schemas/...`
/// references — the `components.schemas` section of the `OpenAPI` document.
#[must_use]
pub fn component_schemas() -> Map<String, Value> {
    let settings = SchemaSettings::draft2020_12()
        .with(|s| s.definitions_path = "#/components/schemas/".into());
    let mut generator = SchemaGenerator::new(settings);

    macro_rules! apply {
        ($ty:ty) => {
            let _ = generator.subschema_for::<$ty>();
        };
    }
    for_each_root_type!(apply);

    generator.take_definitions(true).into_iter().collect()
}

/// Global `OpenAPI` tags — every [`domain_tag`] must be declared here
/// (enforced by test).
const TAGS: &[(&str, &str)] = &[
    ("pair", "Device pairing and scoped tokens (SPEC §2.A)"),
    (
        "sessions",
        "Conversation sessions, turns, draft, suggestions (SPEC §5.A)",
    ),
    (
        "memory",
        "Personal memory — experimental P2 domain (SPEC §5.B)",
    ),
    ("voice", "Text-to-speech (SPEC §6)"),
    (
        "asr",
        "Consented partner-speech listening — experimental (SPEC §5.A)",
    ),
    ("profiles", "User profiles — experimental (SPEC §7.B)"),
    ("input", "FluenceInput v1 target declaration (SPEC §4.A)"),
    (
        "devices",
        "Paired device management — caregiver space (SPEC §7.C)",
    ),
    (
        "system",
        "Health, capabilities, degradation events (SPEC §2.C)",
    ),
    ("ws", "WebSocket multiplexed topics (SPEC §2.A)"),
];

/// Assembles the `OpenAPI` 3.1 document from the route registry
/// ([`routes()`]) and the component schemas.
///
/// # Panics
///
/// Panics if a [`crate::api::pair::Scope`] fails to serialize — impossible
/// for the fixed enum; the `expect` documents the assumption.
#[must_use]
pub fn openapi_document() -> Value {
    let mut paths = Map::new();
    for route in routes() {
        let item = paths
            .entry(route.path.to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        item.as_object_mut()
            .expect("path items are objects")
            .insert(route.method.as_str().to_owned(), operation(route));
    }

    let tags: Vec<Value> = TAGS
        .iter()
        .map(|(name, description)| json!({ "name": name, "description": description }))
        .collect();

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Fluence hub API",
            "summary": "Local communication hub: input, acceleration, voice (SPEC §5.A)",
            "description": "API of the Fluence hub — a local-first communication \
                platform for people with severe motor disabilities. One session = one \
                conversation with a warm KV-cache; suggestions stream over SSE with \
                per-slot cancellation; input flows through the FluenceInput protocol \
                (WebSocket topics). Authentication: per-device revocable tokens with \
                scopes (display/control/care/system). Operations are marked \
                `x-fluence-stability: stable` (A1 core, frozen) or `experimental` \
                (P2 domains, may change). Errors follow RFC 9457 problem+json with a \
                stable machine-readable code catalogue.",
            "version": format!("{}", crate::INPUT_PROTOCOL_VERSION),
            "license": { "name": "Apache-2.0", "identifier": "Apache-2.0" },
            "contact": {
                "name": "Fluence",
                "url": "https://github.com/Pluenet-Killian/Fluence",
            },
        },
        "tags": tags,
        "servers": [
            { "url": "http://127.0.0.1:7411", "description": "Embedded mode (loopback, no TLS)" },
            { "url": "https://{host}:7411", "description": "Home mode (LAN, local CA)",
              "variables": { "host": { "default": "fluence.local" } } },
        ],
        "paths": paths,
        "components": {
            "schemas": component_schemas(),
            "securitySchemes": {
                "fluenceToken": {
                    "type": "apiKey",
                    "in": "header",
                    "name": "X-Fluence-Token",
                    "description": "Per-device revocable token with a scope (SPEC §2.A)",
                },
            },
        },
    })
}

/// Builds the `OpenAPI` operation object for one route.
fn operation(route: &RouteSpec) -> Value {
    let mut op = Map::new();
    op.insert("summary".into(), json!(route.summary));
    op.insert("operationId".into(), json!(operation_id(route)));
    op.insert("tags".into(), json!([domain_tag(route.path)]));
    op.insert(
        "x-fluence-stability".into(),
        json!(route.stability.as_str()),
    );

    match route.auth.allowed_scopes() {
        None => {
            op.insert("security".into(), json!([]));
        }
        Some(allowed) => {
            op.insert("security".into(), json!([{ "fluenceToken": [] }]));
            // `system` passes everywhere; an empty list = system-only.
            let scopes: Vec<Value> = allowed
                .iter()
                .map(|s| serde_json::to_value(s).expect("scopes serialize"))
                .collect();
            op.insert("x-fluence-scopes".into(), Value::Array(scopes));
        }
    }

    let parameters = parameters(route);
    if !parameters.is_empty() {
        op.insert("parameters".into(), Value::Array(parameters));
    }

    if let Some(request) = route.request {
        op.insert(
            "requestBody".into(),
            json!({
                "required": true,
                "content": { "application/json": {
                    "schema": { "$ref": format!("#/components/schemas/{request}") },
                } },
            }),
        );
    }

    op.insert("responses".into(), responses(route));
    Value::Object(op)
}

/// Derives a stable `operationId` from method and path
/// (`post_sessions_id_suggest`).
fn operation_id(route: &RouteSpec) -> String {
    let path = route
        .path
        .trim_start_matches("/api/v1")
        .trim_matches('/')
        .replace(['/', '-'], "_")
        .replace(['{', '}'], "");
    format!("{}_{}", route.method.as_str(), path)
}

/// Groups operations by their first meaningful path segment.
fn domain_tag(path: &str) -> &str {
    let trimmed = path.trim_start_matches("/api/v1").trim_start_matches('/');
    let first = trimmed.split('/').next().unwrap_or_default();
    if first.is_empty() { "system" } else { first }
}

/// Path (`{id}` style) and query parameters of a route.
fn parameters(route: &RouteSpec) -> Vec<Value> {
    let mut out = Vec::new();
    for segment in route.path.split('/') {
        if let Some(name) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            out.push(json!({
                "name": name,
                "in": "path",
                "required": true,
                "schema": { "type": "string" },
            }));
        }
    }
    for query in route.query {
        out.push(json!({
            "name": query.name,
            "in": "query",
            "required": query.required,
            "description": query.description,
            "schema": { "type": "string" },
        }));
    }
    out
}

/// Response object for a route, including the problem+json default.
fn responses(route: &RouteSpec) -> Value {
    let success = match route.response {
        ResponseSpec::Json(schema) => json!({
            "200": {
                "description": "Success",
                "content": { "application/json": {
                    "schema": { "$ref": format!("#/components/schemas/{schema}") },
                } },
            },
        }),
        ResponseSpec::Sse(schema) => json!({
            "200": {
                "description": "Server-sent event stream. The `event:` field is the \
                                variant tag, `data:` is the JSON payload (canonical \
                                form documented by the schema).",
                "content": { "text/event-stream": {
                    "schema": { "$ref": format!("#/components/schemas/{schema}") },
                } },
            },
        }),
        ResponseSpec::AudioStream => json!({
            "200": {
                "description": "Streamed audio (WAV, 16-bit mono PCM — ADR-0009; \
                                Opus/Ogg for LAN/home mode is deferred to Phase 7)",
                "content": { "audio/wav": { "schema": { "type": "string", "format": "binary" } } },
            },
        }),
        ResponseSpec::NoContent => json!({ "204": { "description": "No content" } }),
        ResponseSpec::WebSocketUpgrade => json!({
            "101": { "description": "WebSocket upgrade — frames are ServerFrame / ClientFrame \
                                     (see components), first frame is system.hello" },
        }),
    };

    let mut responses = success.as_object().expect("responses are objects").clone();
    responses.insert(
        "default".into(),
        json!({
            "description": "Error (RFC 9457)",
            "content": { "application/problem+json": {
                "schema": { "$ref": "#/components/schemas/Problem" },
            } },
        }),
    );
    Value::Object(responses)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn standalone_schemas_cover_every_root_type_uniquely() {
        let schemas = standalone_schemas();
        let names: HashSet<_> = schemas.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names.len(), schemas.len(), "duplicate schema names");
        assert!(names.contains("SuggestRequest"));
        assert!(names.contains("ServerFrame"));
    }

    /// Every schema named by the route registry must exist in components —
    /// a typo in the registry breaks here, not in a downstream consumer.
    #[test]
    fn route_schema_references_resolve() {
        let components = component_schemas();
        for route in routes() {
            let mut referenced: Vec<&str> = Vec::new();
            if let Some(request) = route.request {
                referenced.push(request);
            }
            match route.response {
                ResponseSpec::Json(name) | ResponseSpec::Sse(name) => referenced.push(name),
                _ => {}
            }
            for name in referenced {
                assert!(
                    components.contains_key(name),
                    "route {} references unknown schema {name}",
                    route.path
                );
            }
        }
    }

    #[test]
    fn openapi_document_is_3_1_and_internally_consistent() {
        let document = openapi_document();
        assert_eq!(document["openapi"], "3.1.0");

        // Collect every $ref in the document and check it resolves.
        let components = document["components"]["schemas"]
            .as_object()
            .expect("components present");
        let mut refs = Vec::new();
        collect_refs(&document, &mut refs);
        for reference in refs {
            let name = reference
                .strip_prefix("#/components/schemas/")
                .unwrap_or_else(|| panic!("non-component $ref: {reference}"));
            // Nested $defs inside a component resolve within it; top-level
            // names must exist directly.
            let top = name.split('/').next().expect("non-empty ref");
            assert!(components.contains_key(top), "dangling $ref: {reference}");
        }
    }

    fn collect_refs(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, nested) in map {
                    if key == "$ref"
                        && let Some(reference) = nested.as_str()
                    {
                        out.push(reference.to_owned());
                    }
                    collect_refs(nested, out);
                }
            }
            Value::Array(items) => {
                for item in items {
                    collect_refs(item, out);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn every_domain_tag_is_declared() {
        let declared: HashSet<&str> = TAGS.iter().map(|(name, _)| *name).collect();
        for route in routes() {
            let tag = domain_tag(route.path);
            assert!(
                declared.contains(tag),
                "tag `{tag}` ({}) not in TAGS",
                route.path
            );
        }
    }

    #[test]
    fn experimental_routes_are_marked_in_openapi() {
        let document = openapi_document();
        let op = &document["paths"]["/api/v1/memory/forget"]["post"];
        assert_eq!(op["x-fluence-stability"], "experimental");
        let op = &document["paths"]["/api/v1/sessions/{id}/suggest"]["post"];
        assert_eq!(op["x-fluence-stability"], "stable");
    }
}
