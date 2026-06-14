// SPDX-License-Identifier: Apache-2.0

//! 7.7 — adversarial-input robustness of the input math (SPEC §9.A, §2.C).
//!
//! Pointer coordinates, confidences and head pose arrive over the WebSocket
//! `ptr`/pose frames from a *client* — untrusted. JSON cannot carry `NaN`, but
//! it can carry ±∞ (huge exponents) and extreme magnitudes. The hub-side math
//! must never panic on any `f64`, and must keep finite inputs finite and
//! bounded, so a malformed or hostile stream can at worst miss a target — never
//! crash the always-alive keyboard (« le clavier parle toujours », SPEC §2.C).
//!
//! Property-based, cross-platform, runs on every CI build — the substantiation
//! of SECURITY.md's "robustness tests of the input parsers".

use std::time::Duration;

use fluence_input::{
    FusionConfig, Magnet, MagnetismConfig, OneEuro2D, OneEuroConfig, apply_magnetism,
    fuse_confidence_weighted, head_affine,
};
use fluence_protocol::input::HeadPose;
use proptest::prelude::*;

/// Every pathological `f64` an attacker (or a glitching sensor) might inject,
/// mixed with the normal pointing range.
fn hostile_f64() -> impl Strategy<Value = f64> {
    prop_oneof![
        Just(f64::NAN),
        Just(f64::INFINITY),
        Just(f64::NEG_INFINITY),
        Just(0.0),
        Just(-0.0),
        Just(f64::MAX),
        Just(f64::MIN),
        Just(f64::MIN_POSITIVE),
        -1.0e12f64..=1.0e12,
        -2.0f64..=2.0,
    ]
}

/// Plain finite values for the finite-preservation properties.
fn finite_f64() -> impl Strategy<Value = f64> {
    prop_oneof![-1.0e6f64..=1.0e6, -2.0f64..=2.0]
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// One Euro never panics on a hostile stream, whatever the timestamps.
    #[test]
    fn one_euro_never_panics(
        stream in proptest::collection::vec((hostile_f64(), hostile_f64(), 0u64..5_000), 1..64)
    ) {
        let mut filter = OneEuro2D::new(OneEuroConfig::default());
        let mut t = 0u64;
        for (x, y, dt) in stream {
            t = t.saturating_add(dt);
            let _ = filter.filter(x, y, Duration::from_millis(t));
        }
    }

    /// Finite inputs stay finite through One Euro (no NaN/∞ invented).
    #[test]
    fn one_euro_keeps_finite_finite(
        stream in proptest::collection::vec((finite_f64(), finite_f64(), 1u64..200), 1..64)
    ) {
        let mut filter = OneEuro2D::new(OneEuroConfig::default());
        let mut t = 0u64;
        for (x, y, dt) in stream {
            t = t.saturating_add(dt);
            let (fx, fy) = filter.filter(x, y, Duration::from_millis(t));
            prop_assert!(fx.is_finite() && fy.is_finite(), "{fx},{fy} from {x},{y}");
        }
    }

    /// Confidence-weighted fusion never panics on hostile sources.
    #[test]
    fn fusion_never_panics(
        sources in proptest::collection::vec((hostile_f64(), hostile_f64(), hostile_f64()), 0..16)
    ) {
        let _ = fuse_confidence_weighted(&sources);
    }

    /// With finite sources and a positive confidence, the fused point is finite.
    #[test]
    fn fusion_of_finite_sources_is_finite(
        sources in proptest::collection::vec((finite_f64(), finite_f64(), 0.001f64..1.0e6), 1..16)
    ) {
        let fused = fuse_confidence_weighted(&sources).expect("positive confidence fuses");
        prop_assert!(fused.0.is_finite() && fused.1.is_finite() && fused.2.is_finite());
    }

    /// Head-affine never panics; finite inputs land in [0, 1] (it clamps).
    #[test]
    fn head_affine_never_panics_and_clamps(
        gx in hostile_f64(), gy in hostile_f64(),
        yaw in hostile_f64(), pitch in hostile_f64(), roll in hostile_f64(),
        with_pose in any::<bool>(),
    ) {
        let pose = with_pose.then_some(HeadPose { yaw, pitch, roll });
        let (x, y) = head_affine((gx, gy), pose, &FusionConfig::default());
        match pose {
            // Contract: with no pose the gaze is returned unchanged — the caller
            // supplies an already-normalized point, so this is a pure pass-through
            // (finite in ⇒ same finite out), not a clamping boundary.
            None if gx.is_finite() && gy.is_finite() => {
                prop_assert!((x - gx).abs() < f64::EPSILON && (y - gy).abs() < f64::EPSILON);
            }
            // A finite head offset is bounded to the zone AND clamped to the
            // surface, so finite gaze + finite pose ⇒ a point in [0, 1].
            Some(p) if p.yaw.is_finite() && p.pitch.is_finite() && gx.is_finite() && gy.is_finite() => {
                prop_assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y), "{x},{y}");
            }
            // Non-finite gaze/pose: only the no-panic guarantee is asserted.
            _ => {}
        }
    }

    /// Magnetism never panics, and finite inputs yield a finite, capped pull —
    /// the SPEC §4.C agency guarantee can never be turned into a crash.
    #[test]
    fn magnetism_never_panics_and_stays_finite(
        px in hostile_f64(), py in hostile_f64(),
        layout in proptest::collection::vec((hostile_f64(), hostile_f64(), hostile_f64()), 0..12),
    ) {
        let magnets: Vec<Magnet> = layout
            .into_iter()
            .map(|(cx, cy, prior)| Magnet { center: (cx, cy), prior })
            .collect();
        let point = (px, py);
        let pulled = apply_magnetism(point, &magnets, &MagnetismConfig::default());
        if point.0.is_finite()
            && point.1.is_finite()
            && magnets
                .iter()
                .all(|m| m.center.0.is_finite() && m.center.1.is_finite() && m.prior.is_finite())
        {
            prop_assert!(
                pulled.0.is_finite() && pulled.1.is_finite(),
                "finite layout produced {pulled:?}"
            );
            // A finite displacement, however small the layout, never explodes.
            prop_assert!(distance(point, pulled).is_finite());
        }
    }
}
