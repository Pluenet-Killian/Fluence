// SPDX-License-Identifier: Apache-2.0

//! Fluence hub: HTTP/WS API, worker supervision, selection engine (SPEC §2.B).
//!
//! The hub is the always-alive core of the platform. It owns the cardinal
//! reliability rule — *composing and speaking NEVER depend on AI component
//! health* (SPEC §2.C, D-2.6): inference workers run as supervised child
//! processes, and every failure degrades explicitly (embedded n-gram
//! fallback, OS voice fallback) instead of breaking input.
//!
//! PLAN Phase 2 builds the real hub (bootstrap, IPC layer, supervisor,
//! pairing & scoped tokens, `/system/health`). The binary entry point lands
//! there; this crate intentionally stays empty until then.

#[cfg(test)]
mod tests {
    /// D-10.1: hub internals are reusable bricks, licensed Apache-2.0
    /// (the assembled application in `apps/` is AGPL-3.0-only).
    #[test]
    fn crate_license_follows_d_10_1() {
        assert_eq!(env!("CARGO_PKG_LICENSE"), "Apache-2.0");
    }
}
