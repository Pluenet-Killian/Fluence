// SPDX-License-Identifier: Apache-2.0

//! The assembled webcam-gaze selection pipeline (SPEC §4.C/§4.D).
//!
//! One method, [`GazePipeline::on_gaze`], runs the full path for a raw sensor
//! sample: calibration (features → normalized point), head-affine fusion, One
//! Euro smoothing, I-VT fixation gating (the dwell only progresses on fixations,
//! and saccades/losses *hold* the gauge, never cancel it), capped magnetism, and
//! the dwell engine. It is clock-free and deterministic, so a recorded session
//! replays exactly — the basis of the T4 gaze-accuracy test.

use std::time::Duration;

use fluence_protocol::TargetId;
use fluence_protocol::input::{HeadPose, TargetMap};

use crate::{
    Calibrator, DwellConfig, FusionConfig, GazeState, IvtClassifier, IvtConfig, Magnet,
    MagnetismConfig, NoiseModel, OneEuro2D, OneEuroConfig, SelectionEngine, SelectionUpdate,
    apply_magnetism, head_affine,
};

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Tuning for every stage of the gaze pipeline (per profile, SPEC §4.C).
#[derive(Debug, Clone, Copy, Default)]
pub struct GazeConfig {
    /// One Euro smoothing.
    pub one_euro: OneEuroConfig,
    /// I-VT fixation/saccade thresholds.
    pub ivt: IvtConfig,
    /// Head-affine fusion.
    pub fusion: FusionConfig,
    /// Capped linguistic magnetism.
    pub magnetism: MagnetismConfig,
    /// Dwell selection.
    pub dwell: DwellConfig,
}

/// The full gaze→selection pipeline for one surface and context profile.
#[derive(Debug)]
pub struct GazePipeline {
    calibrator: Calibrator,
    smoother: OneEuro2D,
    ivt: IvtClassifier,
    noise: NoiseModel,
    fusion: FusionConfig,
    magnetism: MagnetismConfig,
    engine: SelectionEngine,
}

impl GazePipeline {
    /// Builds a pipeline around a (possibly already fitted) calibrator, with
    /// default stage tuning.
    #[must_use]
    pub fn new(calibrator: Calibrator) -> Self {
        Self::with_config(calibrator, GazeConfig::default())
    }

    /// Builds a pipeline with explicit per-stage tuning.
    #[must_use]
    pub fn with_config(calibrator: Calibrator, config: GazeConfig) -> Self {
        Self {
            calibrator,
            smoother: OneEuro2D::new(config.one_euro),
            ivt: IvtClassifier::new(config.ivt),
            noise: NoiseModel::default(),
            fusion: config.fusion,
            magnetism: config.magnetism,
            engine: SelectionEngine::new(config.dwell),
        }
    }

    /// Declares the selectable targets for this surface (SPEC §4.A).
    pub fn set_targets(&mut self, map: &TargetMap) -> Vec<SelectionUpdate> {
        self.engine.set_targets(map)
    }

    /// The calibrator (read-only) — exposes the live calibration quality (§4.D).
    #[must_use]
    pub fn calibrator(&self) -> &Calibrator {
        &self.calibrator
    }

    /// The calibrator (mutable) — for pursuit samples and continuous calibration.
    pub fn calibrator_mut(&mut self) -> &mut Calibrator {
        &mut self.calibrator
    }

    /// The per-user noise model (sizes the effective targets, §4.C).
    #[must_use]
    pub fn noise(&self) -> &NoiseModel {
        &self.noise
    }

    /// Processes one raw gaze sample at monotonic time `now`: sensor `features`
    /// (the client's geometric features), an optional head `pose`, and the source
    /// `conf`idence. Returns the resulting selection updates.
    ///
    /// Until the profile is calibrated the pipeline cannot map features to the
    /// screen, so it holds the dwell and emits nothing. On a saccade or a signal
    /// loss the dwell is held (not cancelled). On a fixation the smoothed,
    /// magnetized point drives the dwell engine; a commit folds its error into
    /// the noise model.
    pub fn on_gaze(
        &mut self,
        features: &[f64],
        pose: Option<HeadPose>,
        conf: f64,
        now: Duration,
    ) -> Vec<SelectionUpdate> {
        let Some(raw) = self.calibrator.predict(features) else {
            self.engine.hold(now);
            return Vec::new();
        };
        let fused = head_affine(raw, pose, &self.fusion);

        // I-VT gates on the (raw) fused point, and **only fixations** advance the
        // One Euro smoother. This refines the literal §4.C order (filter → I-VT)
        // for the same intent: smoothing can never mask a saccade, and a held
        // saccade leaves no smoother lag to drift across cells on return.
        match self.ivt.classify(fused.0, fused.1, conf, now) {
            GazeState::Fixation => {
                let (sx, sy) = self.smoother.filter(fused.0, fused.1, now);
                let magnets: Vec<Magnet> = self.engine.magnets();
                let (mx, my) = apply_magnetism((sx, sy), &magnets, &self.magnetism);
                let updates = self.engine.on_pointer(mx, my, now);
                self.fold_commit_into_noise(&updates, (sx, sy));
                updates
            }
            // A saccade holds the gauge; a loss pauses it softly (SPEC §4.C).
            GazeState::Saccade | GazeState::Lost => {
                self.engine.hold(now);
                Vec::new()
            }
        }
    }

    /// On a commit, folds the fixation error (gaze point vs target centre) into
    /// the per-user noise model.
    fn fold_commit_into_noise(&mut self, updates: &[SelectionUpdate], gaze: (f64, f64)) {
        for update in updates {
            if let SelectionUpdate::Commit { target, .. } = update
                && let Some(center) = self.engine.target_center(target)
            {
                self.noise.observe_commit_error(distance(gaze, center));
            }
        }
    }

    /// Which target the calibrated pipeline would select for `features` right now
    /// (calibrate → fuse → magnetism → hit-test), independent of dwell timing.
    /// This is the spatial accuracy the T4 replay measures. `None` if not
    /// calibrated or the point hits no target.
    #[must_use]
    pub fn resolve_target(&self, features: &[f64], pose: Option<HeadPose>) -> Option<TargetId> {
        let raw = self.calibrator.predict(features)?;
        let (fx, fy) = head_affine(raw, pose, &self.fusion);
        let magnets = self.engine.magnets();
        let (mx, my) = apply_magnetism((fx, fy), &magnets, &self.magnetism);
        self.engine.hit(mx, my).map(|target| target.id.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluence_protocol::input::{Rect, Target, TargetRole, Viewport};
    use fluence_protocol::{Normalized, SurfaceId};

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// A 3×1 keyboard of adjacent 0.3-wide cells centred at x ∈ {0.2, 0.5, 0.8}
    /// (width = spacing, like real keys, so the 40%-of-spacing magnetism cap
    /// stays inside a half-cell — the agency guarantee).
    fn keyboard() -> TargetMap {
        let cell = |id: &str, cx: f64| Target {
            id: TargetId::from(id),
            rect: Rect {
                x: (cx - 0.15) * 100.0,
                y: 0.35 * 100.0,
                w: 0.3 * 100.0,
                h: 0.3 * 100.0,
            },
            role: TargetRole::Key,
            label: None,
            prior: None,
        };
        TargetMap {
            surface: SurfaceId::from("main"),
            viewport: Viewport { w: 100, h: 100 },
            targets: vec![cell("a", 0.2), cell("b", 0.5), cell("c", 0.8)],
        }
    }

    /// An identity-calibrated pipeline: features `[x, y]` map straight to the
    /// normalized point `(x, y)`, so we can drive it with screen coordinates.
    fn identity_pipeline() -> GazePipeline {
        let mut cal = Calibrator::new("test");
        for i in 0..=10 {
            for j in 0..=10 {
                let (x, y) = (f64::from(i) / 10.0, f64::from(j) / 10.0);
                cal.add_sample(vec![x, y], (x, y));
            }
        }
        cal.fit().expect("identity fits");
        let mut pipeline = GazePipeline::new(cal);
        let _ = pipeline.set_targets(&keyboard());
        pipeline
    }

    #[test]
    fn uncalibrated_pipeline_emits_nothing() {
        let mut pipeline = GazePipeline::new(Calibrator::new("blank"));
        let _ = pipeline.set_targets(&keyboard());
        assert!(pipeline.on_gaze(&[0.5, 0.5], None, 1.0, ms(0)).is_empty());
    }

    #[test]
    fn a_sustained_fixation_composes_a_key_by_dwell() {
        let mut pipeline = identity_pipeline();
        let mut committed = None;
        let mut t = 0;
        // Look steadily at key "b" (centre 0.5, 0.5) past the base dwell.
        for _ in 0..80 {
            for update in pipeline.on_gaze(&[0.5, 0.5], None, 1.0, ms(t)) {
                if let SelectionUpdate::Commit { target, .. } = update {
                    committed = Some(target);
                }
            }
            t += 16;
        }
        assert_eq!(
            committed,
            Some(TargetId::from("b")),
            "dwell should commit key b"
        );
    }

    #[test]
    fn a_saccade_holds_the_gauge_and_does_not_cancel() {
        let mut pipeline = identity_pipeline();
        // Build up dwell on "b" with several fixations.
        let mut t = 0;
        for _ in 0..10 {
            let _ = pipeline.on_gaze(&[0.5, 0.5], None, 1.0, ms(t));
            t += 16;
        }
        // A single fast jump (saccade) to "c": it must be held, not a Cancel.
        let during = pipeline.on_gaze(&[0.8, 0.5], None, 1.0, ms(t));
        assert!(
            !during.iter().any(|u| matches!(u, SelectionUpdate::Cancel)),
            "a saccade must not cancel the dwell"
        );
        t += 16;
        // Back to "b": the dwell resumes and still commits b (not stolen by c).
        let mut committed = None;
        for _ in 0..80 {
            for update in pipeline.on_gaze(&[0.5, 0.5], None, 1.0, ms(t)) {
                if let SelectionUpdate::Commit { target, .. } = update {
                    committed = Some(target);
                }
            }
            t += 16;
        }
        assert_eq!(committed, Some(TargetId::from("b")));
    }

    #[test]
    fn resolve_target_measures_spatial_accuracy() {
        let mut pipeline = identity_pipeline();
        // Add a strong prior to "b" — magnetism must still not steal a deliberate
        // point on "a" (the agency guarantee, exercised end-to-end).
        let mut map = keyboard();
        map.targets[1].prior = Some(Normalized::new(1.0).unwrap());
        let _ = pipeline.set_targets(&map);
        assert_eq!(
            pipeline.resolve_target(&[0.2, 0.5], None),
            Some(TargetId::from("a"))
        );
        assert_eq!(
            pipeline.resolve_target(&[0.8, 0.5], None),
            Some(TargetId::from("c"))
        );
        // Off-surface gaze hits nothing.
        assert_eq!(pipeline.resolve_target(&[0.5, 0.95], None), None);
    }

    #[test]
    fn commits_refine_the_noise_model() {
        let mut pipeline = identity_pipeline();
        let mut t = 0;
        for _ in 0..80 {
            let _ = pipeline.on_gaze(&[0.5, 0.5], None, 1.0, ms(t));
            t += 16;
        }
        assert!(
            pipeline.noise().samples() >= 1,
            "a commit should feed the noise model"
        );
    }
}
