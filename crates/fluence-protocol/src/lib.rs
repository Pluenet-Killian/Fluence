// SPDX-License-Identifier: Apache-2.0

//! ★ Single source of truth for Fluence types and schemas (SPEC §2.B, D-2.5).
//!
//! Every message that crosses a process or network boundary — input protocol
//! (SPEC §4.A), hub API (§5.A), memory subsystem (§5.B), pairing and security
//! (§2.A) — is defined here once, in Rust, then generated outward:
//! JSON Schema → `OpenAPI` 3.1 → TypeScript SDK types. CI fails when generated
//! artifacts drift from these definitions (`cargo xtask check-contracts`).
//!
//! # Module map
//!
//! - [`common`] — validated scalars ([`Normalized`], [`TimestampMicros`])
//!   and typed identifiers (re-exported at the crate root);
//! - [`input`] — `FluenceInput` v1 (SPEC §4.A): samples, targets, selection
//!   events;
//! - [`ws`] — WebSocket envelope and topics (SPEC §2.A);
//! - [`api`] — hub API domains (SPEC §5.A), one module per domain;
//! - [`error`] — RFC 9457 problem envelope with the stable code catalogue;
//! - [`routes`] — the declarative route registry (the API surface as data);
//! - [`contracts`] — JSON Schema / `OpenAPI` generation driven by
//!   `cargo xtask check-contracts`.
//!
//! # Conventions
//!
//! - **Invariants live in types**: a [`Normalized`] cannot hold `1.2`;
//!   deserialization rejects it (PLAN T2).
//! - **Forward compatibility**: server→client event enums are
//!   `#[non_exhaustive]`; presentation enums tolerate unknown variants
//!   (`#[serde(other)]`); security enums ([`api::pair::Scope`],
//!   [`api::sessions::Speaker`]) fail closed instead.
//! - **Stability levels** (PLAN task 1.3bis): each `api` module documents
//!   `stable` (A1 core) or `experimental` (P2 domains); the `OpenAPI` carries
//!   `x-fluence-stability` per operation.
//! - **P0 fields** (conversation text, memory content — SPEC §9.A) are
//!   marked in their doc and must never be logged.

pub mod api;
pub mod common;
pub mod contracts;
pub mod error;
pub mod input;
pub mod routes;
pub mod ws;

pub use common::{
    DeviceId, MemoryItemId, ModelId, Normalized, OutOfRange, ProfileId, SessionId, SlotId,
    SourceId, SurfaceId, TargetId, TimestampMicros, VoiceId,
};

/// Version of the `FluenceInput` protocol (SPEC §4.A, D-4.1).
///
/// Sent as the `v` query parameter when a client opens the WebSocket and
/// negotiated in the `system.hello` frame; the hub rejects samples whose
/// protocol version it does not understand.
///
/// ```
/// assert_eq!(fluence_protocol::INPUT_PROTOCOL_VERSION, 1);
/// ```
pub const INPUT_PROTOCOL_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    /// D-10.1: the protocol is a reusable brick, licensed Apache-2.0.
    #[test]
    fn crate_license_follows_d_10_1() {
        assert_eq!(env!("CARGO_PKG_LICENSE"), "Apache-2.0");
    }
}
