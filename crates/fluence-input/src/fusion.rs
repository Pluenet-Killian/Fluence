// SPDX-License-Identifier: Apache-2.0

//! Multi-source fusion, head-affine pointing, capped linguistic magnetism, and
//! the per-user noise model (SPEC §4.C steps 4-5).
//!
//! All functions are pure and deterministic. Points are normalized-surface
//! `(x, y)` in `[0, 1]`; the physical mapping lives in calibration.

use fluence_protocol::input::HeadPose;

/// Confidence-weighted fusion of several pointing sources (SPEC §4.C step 4):
/// the inverse-variance optimal estimate, approximated by weighting each source
/// by its confidence. Sources with non-positive confidence are ignored. Returns
/// the fused point and the summed confidence, or `None` if nothing is usable.
#[must_use]
pub fn fuse_confidence_weighted(sources: &[(f64, f64, f64)]) -> Option<(f64, f64, f64)> {
    let mut wx = 0.0;
    let mut wy = 0.0;
    let mut sum = 0.0;
    for &(x, y, conf) in sources {
        if conf > 0.0 {
            wx += x * conf;
            wy += y * conf;
            sum += conf;
        }
    }
    (sum > 0.0).then(|| (wx / sum, wy / sum, sum))
}

/// Head-affine pointing parameters (« regard désigne, tête affine », SPEC §4.C).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FusionConfig {
    /// Normalized offset applied per degree of head rotation, inside the zone.
    pub head_gain: f64,
    /// Radius (normalized) of the gaze zone of interest the head refines (~3°).
    pub zone_radius: f64,
}

impl Default for FusionConfig {
    fn default() -> Self {
        Self {
            head_gain: 0.01,
            zone_radius: 0.06,
        }
    }
}

/// Refines a gaze point with a head-pose offset, bounded to the zone of interest
/// (SPEC §4.C step 4): the gaze designates a zone, the head nudges a fine offset
/// within it. With no pose, the gaze point is returned unchanged.
#[must_use]
pub fn head_affine(gaze: (f64, f64), pose: Option<HeadPose>, config: &FusionConfig) -> (f64, f64) {
    let Some(pose) = pose else {
        return gaze;
    };
    let clamp_zone = |v: f64| v.clamp(-config.zone_radius, config.zone_radius);
    // Yaw (look right) nudges +x; pitch (look up) nudges -y (screen y grows down).
    let dx = clamp_zone(pose.yaw * config.head_gain);
    let dy = clamp_zone(-pose.pitch * config.head_gain);
    ((gaze.0 + dx).clamp(0.0, 1.0), (gaze.1 + dy).clamp(0.0, 1.0))
}

/// Capped-magnetism parameters (SPEC §4.C step 5).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MagnetismConfig {
    /// Hard cap on the magnetic displacement, as a fraction of the
    /// nearest-neighbour inter-target distance. Never above 0.4 (SPEC §4.C): the
    /// user must always be able to reach an improbable key (agency over magnetism).
    pub max_fraction: f64,
}

impl Default for MagnetismConfig {
    fn default() -> Self {
        Self { max_fraction: 0.4 }
    }
}

/// A magnet: a target centre and its linguistic prior in `[0, 1]` (SPEC §4.A).
#[derive(Debug, Clone, Copy)]
pub struct Magnet {
    /// Target centre, normalized surface coordinates.
    pub center: (f64, f64),
    /// Linguistic prior (higher ⇒ stronger attraction).
    pub prior: f64,
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Smallest distance between any two distinct magnet centres — the inter-target
/// spacing the cap is expressed against. `None` with fewer than two magnets.
fn inter_target_distance(magnets: &[Magnet]) -> Option<f64> {
    let mut min = f64::INFINITY;
    for (i, a) in magnets.iter().enumerate() {
        for b in &magnets[i + 1..] {
            min = min.min(distance(a.center, b.center));
        }
    }
    min.is_finite().then_some(min)
}

/// Pulls `point` toward the most likely nearby target, **hard-capped** at
/// `max_fraction` of the inter-target spacing (SPEC §4.C step 5).
///
/// Among magnets within one inter-target distance, the highest-prior one
/// attracts; the displacement is `prior`-scaled and clamped to the cap, so a
/// deliberate point on a low-prior key (cap < half a cell) is never stolen by a
/// high-prior neighbour. With < 2 magnets there is no spacing, so no magnetism.
#[must_use]
pub fn apply_magnetism(
    point: (f64, f64),
    magnets: &[Magnet],
    config: &MagnetismConfig,
) -> (f64, f64) {
    let Some(spacing) = inter_target_distance(magnets) else {
        return point;
    };
    let cap = config.max_fraction.clamp(0.0, 0.4) * spacing;

    // The most likely magnet within capture range (one inter-target distance).
    let chosen = magnets
        .iter()
        .filter(|m| distance(point, m.center) <= spacing && m.prior > 0.0)
        .max_by(|a, b| a.prior.total_cmp(&b.prior));
    let Some(magnet) = chosen else {
        return point;
    };

    let to_target = (magnet.center.0 - point.0, magnet.center.1 - point.1);
    let dist = distance(point, magnet.center);
    if dist <= f64::EPSILON {
        return point;
    }
    // Move `prior` of the way to the target, but never more than the cap.
    let pull = (magnet.prior.clamp(0.0, 1.0) * dist).min(cap);
    let scale = pull / dist;
    (point.0 + to_target.0 * scale, point.1 + to_target.1 * scale)
}

/// Per-user noise model (SPEC §4.C): the running spread of fixation error,
/// estimated on successful commits, that sizes the effective target, the
/// magnetism threshold and the fusion radius — the loop that personalizes
/// precision. An exponentially-weighted variance keeps it adaptive and cheap.
#[derive(Debug, Clone, Copy)]
pub struct NoiseModel {
    variance: f64,
    weight: f64,
    samples: u32,
}

impl Default for NoiseModel {
    fn default() -> Self {
        // Seeded with a small spread (≈ a fifth of a normalized surface jitter),
        // refined as real commits arrive.
        Self {
            variance: 0.0004,
            weight: 0.1,
            samples: 0,
        }
    }
}

impl NoiseModel {
    /// Folds in the squared error (normalized distance) between the gaze point at
    /// commit and the committed target centre. Only successful commits feed this.
    pub fn observe_commit_error(&mut self, error_distance: f64) {
        let squared = error_distance.max(0.0).powi(2);
        self.variance = self.weight * squared + (1.0 - self.weight) * self.variance;
        self.samples = self.samples.saturating_add(1);
    }

    /// Standard deviation of the fixation error (normalized units).
    #[must_use]
    pub fn std_dev(&self) -> f64 {
        self.variance.sqrt()
    }

    /// Effective target radius: never below `base`, grown to cover the user's
    /// spread (`base ⊔ k·σ`) so a noisier user gets larger effective targets.
    #[must_use]
    pub fn effective_target_radius(&self, base: f64) -> f64 {
        base.max(2.0 * self.std_dev())
    }

    /// How many commits have refined the model.
    #[must_use]
    pub fn samples(&self) -> u32 {
        self.samples
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn pose(yaw: f64, pitch: f64) -> HeadPose {
        HeadPose {
            yaw,
            pitch,
            roll: 0.0,
        }
    }

    #[test]
    fn fusion_weights_by_confidence() {
        // Two sources, the confident one near 0.8 dominates the fused point.
        let fused = fuse_confidence_weighted(&[(0.2, 0.5, 0.1), (0.8, 0.5, 0.9)]).unwrap();
        assert!(
            fused.0 > 0.7,
            "confident source did not dominate: {}",
            fused.0
        );
        assert!((fused.2 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn fusion_ignores_zero_confidence_and_empty() {
        assert!(fuse_confidence_weighted(&[]).is_none());
        assert!(fuse_confidence_weighted(&[(0.5, 0.5, 0.0)]).is_none());
        let fused = fuse_confidence_weighted(&[(0.1, 0.1, 0.0), (0.9, 0.9, 1.0)]).unwrap();
        assert!((fused.0 - 0.9).abs() < 1e-9 && (fused.1 - 0.9).abs() < 1e-9);
    }

    #[test]
    fn head_offset_is_bounded_to_the_zone() {
        let config = FusionConfig::default();
        // A huge yaw cannot move the point beyond the zone radius.
        let (x, _) = head_affine((0.5, 0.5), Some(pose(1000.0, 0.0)), &config);
        assert!(
            (x - 0.5).abs() <= config.zone_radius + 1e-9,
            "escaped the zone: {x}"
        );
        // No pose ⇒ unchanged.
        assert_eq!(head_affine((0.3, 0.7), None, &config), (0.3, 0.7));
    }

    #[test]
    fn magnetism_displacement_never_exceeds_the_cap() {
        // A 0.1 grid: nearest spacing 0.1, cap = 0.4 * 0.1 = 0.04.
        let magnets = vec![
            Magnet {
                center: (0.4, 0.5),
                prior: 1.0,
            },
            Magnet {
                center: (0.5, 0.5),
                prior: 1.0,
            },
            Magnet {
                center: (0.6, 0.5),
                prior: 0.0,
            },
        ];
        let config = MagnetismConfig::default();
        let point = (0.55, 0.5);
        let pulled = apply_magnetism(point, &magnets, &config);
        let moved = distance(point, pulled);
        assert!(
            moved <= 0.04 + 1e-9,
            "displacement {moved} exceeded cap 0.04"
        );
    }

    #[test]
    fn an_improbable_key_stays_reachable() {
        // Aiming dead-centre on a low-prior key with a high-prior neighbour: the
        // pull (≤ 40% of spacing) leaves the point inside the low-prior cell
        // (half-cell = 50% of spacing), so hit-testing still picks it.
        let spacing = 0.1;
        let magnets = vec![
            Magnet {
                center: (0.5, 0.5),
                prior: 0.01,
            }, // the improbable key
            Magnet {
                center: (0.6, 0.5),
                prior: 1.0,
            }, // a likely neighbour
        ];
        let pulled = apply_magnetism((0.5, 0.5), &magnets, &MagnetismConfig::default());
        let drift_to_neighbour = pulled.0 - 0.5;
        assert!(
            drift_to_neighbour < spacing * 0.5,
            "point drifted {drift_to_neighbour} into the neighbour's half-cell"
        );
    }

    #[test]
    fn magnetism_is_a_noop_without_spacing() {
        let one = vec![Magnet {
            center: (0.5, 0.5),
            prior: 1.0,
        }];
        assert_eq!(
            apply_magnetism((0.3, 0.3), &one, &MagnetismConfig::default()),
            (0.3, 0.3)
        );
        assert_eq!(
            apply_magnetism((0.3, 0.3), &[], &MagnetismConfig::default()),
            (0.3, 0.3)
        );
    }

    #[test]
    fn noise_model_grows_effective_target_for_a_noisier_user() {
        let mut calm = NoiseModel::default();
        let mut shaky = NoiseModel::default();
        for _ in 0..50 {
            calm.observe_commit_error(0.005);
            shaky.observe_commit_error(0.05);
        }
        assert!(
            shaky.effective_target_radius(0.02) > calm.effective_target_radius(0.02),
            "a noisier user should get a larger effective target"
        );
        assert!(shaky.std_dev() > calm.std_dev());
        assert_eq!(shaky.samples(), 50);
    }

    proptest! {
        /// Whatever the magnets and the point, magnetism never displaces more
        /// than the cap (the core agency guarantee, SPEC §4.C).
        #[test]
        fn magnetism_cap_holds_for_any_layout(
            px in 0.0f64..=1.0, py in 0.0f64..=1.0,
            priors in proptest::collection::vec(0.0f64..=1.0, 2..8),
        ) {
            let magnets: Vec<Magnet> = priors
                .iter()
                .enumerate()
                .map(|(i, &prior)| {
                    let col = u8::try_from(i % 5).unwrap_or(0);
                    let row = u8::try_from(i / 5).unwrap_or(0);
                    Magnet {
                        center: (0.1 + 0.1 * f64::from(col), 0.2 + 0.1 * f64::from(row)),
                        prior,
                    }
                })
                .collect();
            let config = MagnetismConfig::default();
            let point = (px, py);
            let pulled = apply_magnetism(point, &magnets, &config);
            if let Some(spacing) = inter_target_distance(&magnets) {
                let cap = 0.4 * spacing;
                prop_assert!(distance(point, pulled) <= cap + 1e-9);
            }
        }
    }
}
