// SPDX-License-Identifier: Apache-2.0

//! Input engine for Fluence (SPEC §2.B, §4).
//!
//! Implements the three-stage `FluenceInput` architecture (§4.A, D-4.1):
//! normalized sensor samples → selection engine (fusion, One Euro filtering,
//! fixation detection, hit-testing, dwell/scan — all hub-side) → selection
//! events to UIs. Language-model priors modulate adaptive dwell inside the
//! hub loop, which is what makes the gaze→target→language loop testable by
//! replay, independent of any UI.
//!
//! Budgets (§4.A): sample processing < 5 ms; commit → UI event < 20 ms.
//!
//! PLAN Phase 5 builds targets/hit-testing/dwell with the `mouse` source;
//! Phase 6 adds webcam gaze, fusion and calibration. This crate
//! intentionally stays empty until then.

#[cfg(test)]
mod tests {
    /// D-10.1: the input engine is a reusable brick, licensed Apache-2.0.
    #[test]
    fn crate_license_follows_d_10_1() {
        assert_eq!(env!("CARGO_PKG_LICENSE"), "Apache-2.0");
    }
}
