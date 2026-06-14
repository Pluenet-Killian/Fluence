// SPDX-License-Identifier: Apache-2.0

//! Gaze-accuracy replay (PLAN §1 T4): a recorded session — a target map, a set
//! of calibration samples and a set of held-out test samples (sensor features
//! labelled with the target the user looked at) — is replayed through the real
//! [`crate::GazePipeline`] and scored as the **fraction of correctly resolved
//! targets**. Deterministic, so the number is a stable CI regression gate.
//!
//! The shipped datasets are *synthetic* (a controlled noise model): they prove
//! the pipeline recovers targets when calibrated and gate regressions — they are
//! **not** a real-precision claim. Real precision needs real capture
//! (`fluencectl record-gaze`), honestly separate (SPEC §6 pivot clause).

use serde::{Deserialize, Serialize};

use fluence_protocol::TargetId;
use fluence_protocol::input::{Rect, Target, TargetMap, TargetRole, Viewport};

use crate::{Calibrator, GazeConfig, GazePipeline, SelectionEngine};

/// Current schema version of a [`GazeSession`].
pub const GAZE_SESSION_VERSION: u32 = 1;

/// One labelled gaze sample: the sensor `features` observed while the user
/// looked at the target named `target`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GazeSample {
    /// Geometric sensor features (the client's, e.g. iris/pose/geometry).
    pub features: Vec<f64>,
    /// Id of the target the user was looking at (ground truth).
    pub target: String,
}

/// A recorded (or synthetic) gaze session for replay accuracy (PLAN §1 T4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GazeSession {
    /// Schema version.
    pub version: u32,
    /// Dataset name (e.g. "synthetic-grid-v0").
    pub name: String,
    /// Whether the data is synthetic (a pipeline-correctness gate, not a real
    /// precision claim).
    pub synthetic: bool,
    /// Surface viewport (pixels).
    pub viewport: Viewport,
    /// Selectable targets (the keyboard under test).
    pub targets: Vec<Target>,
    /// Samples used to calibrate before scoring.
    pub calibration: Vec<GazeSample>,
    /// Held-out samples scored after calibration.
    pub test: Vec<GazeSample>,
}

impl GazeSession {
    /// The target map (viewport + targets) of this session.
    #[must_use]
    pub fn target_map(&self) -> TargetMap {
        TargetMap {
            surface: "main".into(),
            viewport: self.viewport,
            targets: self.targets.clone(),
        }
    }
}

/// Result of a replay: how many test samples resolved to the right target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccuracyReport {
    /// Number of test samples scored.
    pub total: usize,
    /// Number that resolved to the labelled target.
    pub correct: usize,
}

impl AccuracyReport {
    /// Fraction in `[0, 1]` of correctly resolved targets (0 if no samples).
    #[must_use]
    pub fn accuracy(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        let correct = f64::from(u32::try_from(self.correct).unwrap_or(u32::MAX));
        let total = f64::from(u32::try_from(self.total).unwrap_or(u32::MAX));
        correct / total
    }
}

/// Replays a session through the gaze pipeline and scores spatial accuracy:
/// calibrate from `session.calibration`, then resolve every `session.test`
/// sample and count exact target matches.
#[must_use]
pub fn evaluate(session: &GazeSession, config: GazeConfig) -> AccuracyReport {
    let map = session.target_map();
    // Calibration pairs are (features → the centre of the looked-at target).
    let mut centers = SelectionEngine::new(config.dwell);
    let _ = centers.set_targets(&map);

    let mut calibrator = Calibrator::new(session.name.clone());
    for sample in &session.calibration {
        if let Some(center) = centers.target_center(&TargetId::from(sample.target.as_str())) {
            calibrator.add_sample(sample.features.clone(), center);
        }
    }
    if calibrator.fit().is_none() {
        return AccuracyReport {
            total: session.test.len(),
            correct: 0,
        };
    }

    let mut pipeline = GazePipeline::with_config(calibrator, config);
    let _ = pipeline.set_targets(&map);

    let correct = session
        .test
        .iter()
        .filter(|sample| {
            pipeline.resolve_target(&sample.features, None)
                == Some(TargetId::from(sample.target.as_str()))
        })
        .count();
    AccuracyReport {
        total: session.test.len(),
        correct,
    }
}

/// Builds a deterministic synthetic session: a `cols × rows` grid of adjacent
/// cells, with features a fixed skew of the true normalized centre plus a
/// reproducible per-sample jitter (the ridge map must learn the inverse), a
/// calibration set (6 samples/cell) and a held-out test set (4 samples/cell).
/// Reproducible and versioned in code — the basis of the nightly accuracy gate.
#[must_use]
pub fn synthetic_grid(cols: u32, rows: u32, jitter: f64) -> GazeSession {
    let (vw, vh) = (1000u32, 1000u32);
    let cw = 1.0 / f64::from(cols.max(1));
    let ch = 1.0 / f64::from(rows.max(1));
    let mut targets = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            targets.push(Target {
                id: TargetId::from(format!("k{r}_{c}")),
                rect: Rect {
                    x: f64::from(c) * cw * f64::from(vw),
                    y: f64::from(r) * ch * f64::from(vh),
                    w: cw * f64::from(vw),
                    h: ch * f64::from(vh),
                },
                role: TargetRole::Key,
                label: None,
                prior: None,
            });
        }
    }
    let sample = |r: u32, c: u32, k: u32| {
        let cx = (f64::from(c) + 0.5) * cw;
        let cy = (f64::from(r) + 0.5) * ch;
        let jx = (f64::from((k * 7 + 1) % 5) / 5.0 - 0.4) * jitter;
        let jy = (f64::from((k * 3 + 2) % 5) / 5.0 - 0.4) * jitter;
        GazeSample {
            features: vec![
                0.3 + 0.5 * cx + 0.1 * cy + jx,
                0.2 + 0.05 * cx + 0.6 * cy + jy,
            ],
            target: format!("k{r}_{c}"),
        }
    };
    let mut calibration = Vec::new();
    let mut test = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            for k in 0..6 {
                calibration.push(sample(r, c, k));
            }
            for k in 6..10 {
                test.push(sample(r, c, k));
            }
        }
    }
    GazeSession {
        version: GAZE_SESSION_VERSION,
        name: format!("synthetic-grid-{cols}x{rows}"),
        synthetic: true,
        viewport: Viewport { w: vw, h: vh },
        targets,
        calibration,
        test,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic(cols: u32, rows: u32, jitter: f64) -> GazeSession {
        synthetic_grid(cols, rows, jitter)
    }

    #[test]
    fn pipeline_recovers_targets_on_a_clean_synthetic_session() {
        let session = synthetic(4, 3, 0.0);
        let report = evaluate(&session, GazeConfig::default());
        assert!(
            (report.accuracy() - 1.0).abs() < 1e-9,
            "clean data must resolve perfectly, got {}",
            report.accuracy()
        );
    }

    #[test]
    fn pipeline_stays_accurate_under_moderate_jitter() {
        // Moderate within-cell jitter: calibration averages it in the fit and
        // accuracy stays high (heavier jitter degrades gracefully, by design).
        let session = synthetic(4, 3, 0.08);
        let report = evaluate(&session, GazeConfig::default());
        assert!(
            report.accuracy() >= 0.9,
            "accuracy {} under jitter",
            report.accuracy()
        );
    }

    #[test]
    fn an_uncalibratable_session_scores_zero() {
        let mut session = synthetic(3, 1, 0.0);
        session.calibration.clear();
        let report = evaluate(&session, GazeConfig::default());
        assert_eq!(report.correct, 0);
        assert!(report.total > 0);
    }

    #[test]
    fn session_round_trips_through_serde() {
        let session = synthetic(3, 2, 0.1);
        let json = serde_json::to_string(&session).unwrap();
        let restored: GazeSession = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, session);
    }
}
