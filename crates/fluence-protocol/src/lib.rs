// SPDX-License-Identifier: Apache-2.0

//! ★ Single source of truth for Fluence types and schemas (SPEC §2.B, D-2.5).
//!
//! Every message that crosses a process or network boundary — input protocol
//! (SPEC §4.A), hub API (§5.A), memory subsystem (§5.B), pairing and security
//! (§2.A) — is defined here once, in Rust, then generated outward:
//! JSON Schema → `OpenAPI` 3.1 → TypeScript SDK types. CI fails when generated
//! artifacts drift from these definitions (`cargo xtask check-contracts`).
//!
//! PLAN Phase 1 populates this crate. Until then it only pins the
//! input-protocol version negotiated on the `input` WebSocket topic.

/// Version of the `FluenceInput` protocol (SPEC §4.A, D-4.1).
///
/// Sent as the `v` field when a client opens the `input` WebSocket topic;
/// the hub rejects samples whose protocol version it does not understand.
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
