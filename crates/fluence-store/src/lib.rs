// SPDX-License-Identifier: Apache-2.0

//! Encrypted persistence for Fluence (SPEC §2.B, §9.A, D-9.1).
//!
//! Owns everything that must survive a restart: profiles, drafts
//! (write-ahead autosave, ≤ 1 s guaranteed loss bound — D-2.6), personal
//! memory items (D-5.6), and the access journal. Data classes drive the
//! design: **P0 intimate** data (conversations, memory, voice) is encrypted
//! at rest (`SQLCipher` / `age`), never leaves the household, and never
//! appears in logs, errors, or fixtures.
//!
//! PLAN Phase 2 builds the real store (`SQLCipher`, OS-keyring master key,
//! migrations, draft autosave). This crate intentionally stays empty until
//! then.

#[cfg(test)]
mod tests {
    /// D-10.1: the store is a reusable brick, licensed Apache-2.0.
    #[test]
    fn crate_license_follows_d_10_1() {
        assert_eq!(env!("CARGO_PKG_LICENSE"), "Apache-2.0");
    }
}
