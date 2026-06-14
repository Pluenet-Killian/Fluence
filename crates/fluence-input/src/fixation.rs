// SPDX-License-Identifier: Apache-2.0

//! I-VT fixation/saccade detection (SPEC §4.C step 3).
//!
//! A velocity-threshold classifier: the point-to-point speed of the gaze, in
//! normalized-surface units per second, separates **fixations** (slow, the eye
//! resting on a target) from **saccades** (fast jumps between targets). The
//! dwell only progresses during fixations, and a saccade does **not** cancel the
//! gauge — micro-losses are tolerated (SPEC §4.C). When no usable sample arrives
//! for longer than `lost_after`, the signal is reported [`GazeState::Lost`]
//! (SPEC §4.C: 800 ms ⇒ soft dwell pause). Clock-free and deterministic.

use std::time::Duration;

/// Classification of a gaze sample (SPEC §4.C output `état`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GazeState {
    /// The eye is resting (slow): the dwell may progress.
    Fixation,
    /// A fast jump between targets: the dwell holds, it is not cancelled.
    Saccade,
    /// The signal was lost (a gap longer than `lost_after`, or a low-confidence
    /// sample): the dwell pauses softly and the UI shows an indicator.
    Lost,
}

/// I-VT parameters (per profile, SPEC §4.C).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IvtConfig {
    /// Velocity threshold in normalized-surface units per second. At or below it
    /// a sample is a fixation; above it, a saccade. (Normalized rather than
    /// degrees/s: the physical mapping is per-surface and lives in calibration.)
    pub velocity_threshold: f64,
    /// Below this confidence a sample is treated as a loss (face lost, glare).
    pub min_confidence: f64,
    /// No usable sample for longer than this ⇒ [`GazeState::Lost`] (SPEC: 800 ms).
    pub lost_after: Duration,
}

impl Default for IvtConfig {
    fn default() -> Self {
        Self {
            velocity_threshold: 1.5,
            min_confidence: 0.2,
            lost_after: Duration::from_millis(800),
        }
    }
}

/// Stateful I-VT classifier over a stream of timestamped samples.
#[derive(Debug)]
pub struct IvtClassifier {
    config: IvtConfig,
    last: Option<Sample>,
}

#[derive(Debug, Clone, Copy)]
struct Sample {
    x: f64,
    y: f64,
    t: Duration,
}

impl IvtClassifier {
    /// A fresh classifier (no prior sample).
    #[must_use]
    pub fn new(config: IvtConfig) -> Self {
        Self { config, last: None }
    }

    /// Classifies the sample `(x, y, conf)` at monotonic time `t`.
    ///
    /// A low-confidence sample is [`GazeState::Lost`] and does not become the
    /// reference for the next velocity (so the gauge resumes cleanly when the
    /// signal returns). The first usable sample is a fixation (the eye has to be
    /// somewhere before it can jump).
    #[must_use]
    pub fn classify(&mut self, x: f64, y: f64, conf: f64, t: Duration) -> GazeState {
        if conf < self.config.min_confidence {
            return GazeState::Lost;
        }
        let state = match self.last {
            Some(prev) => match t.checked_sub(prev.t) {
                Some(gap) if gap > self.config.lost_after => GazeState::Lost,
                Some(gap) if gap > Duration::ZERO => {
                    let dt = gap.as_secs_f64();
                    let dist = ((x - prev.x).powi(2) + (y - prev.y).powi(2)).sqrt();
                    if dist / dt <= self.config.velocity_threshold {
                        GazeState::Fixation
                    } else {
                        GazeState::Saccade
                    }
                }
                // Same or backward timestamp: hold the eye where it was.
                _ => GazeState::Fixation,
            },
            None => GazeState::Fixation,
        };
        self.last = Some(Sample { x, y, t });
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    #[test]
    fn first_usable_sample_is_a_fixation() {
        let mut ivt = IvtClassifier::new(IvtConfig::default());
        assert_eq!(ivt.classify(0.5, 0.5, 1.0, ms(0)), GazeState::Fixation);
    }

    #[test]
    fn slow_movement_is_a_fixation_fast_is_a_saccade() {
        let mut ivt = IvtClassifier::new(IvtConfig::default());
        let _ = ivt.classify(0.5, 0.5, 1.0, ms(0));
        // 0.01 units in 16 ms ⇒ 0.625 u/s ≤ 1.5 ⇒ fixation.
        assert_eq!(ivt.classify(0.51, 0.5, 1.0, ms(16)), GazeState::Fixation);
        // 0.4 units in 16 ms ⇒ 25 u/s ≫ 1.5 ⇒ saccade.
        assert_eq!(ivt.classify(0.91, 0.5, 1.0, ms(32)), GazeState::Saccade);
    }

    #[test]
    fn a_long_gap_is_a_loss() {
        let mut ivt = IvtClassifier::new(IvtConfig::default());
        let _ = ivt.classify(0.5, 0.5, 1.0, ms(0));
        assert_eq!(ivt.classify(0.5, 0.5, 1.0, ms(1000)), GazeState::Lost);
    }

    #[test]
    fn a_low_confidence_sample_is_a_loss_and_does_not_anchor_velocity() {
        let mut ivt = IvtClassifier::new(IvtConfig::default());
        let _ = ivt.classify(0.10, 0.5, 1.0, ms(0));
        // Glare: ignored as a loss, not used as the velocity reference.
        assert_eq!(ivt.classify(0.90, 0.5, 0.0, ms(16)), GazeState::Lost);
        // Back near the original point: still a fixation (velocity measured from
        // the last *usable* sample at 0.10, not the dropped 0.90).
        assert_eq!(ivt.classify(0.11, 0.5, 1.0, ms(32)), GazeState::Fixation);
    }

    #[test]
    fn segments_a_synthetic_fixation_saccade_fixation_signal_exactly() {
        let mut ivt = IvtClassifier::new(IvtConfig::default());
        let mut states = Vec::new();
        let mut t = 0u64;
        // Fixation: tiny jitter around (0.3, 0.3).
        for i in 0..5 {
            let jitter = f64::from(i % 2) * 0.002;
            states.push(ivt.classify(0.3 + jitter, 0.3, 1.0, ms(t)));
            t += 16;
        }
        // One big jump to (0.8, 0.8) — a saccade.
        states.push(ivt.classify(0.8, 0.8, 1.0, ms(t)));
        t += 16;
        // Fixation again around (0.8, 0.8).
        for i in 0..5 {
            let jitter = f64::from(i % 2) * 0.002;
            states.push(ivt.classify(0.8 + jitter, 0.8, 1.0, ms(t)));
            t += 16;
        }
        let saccades = states.iter().filter(|s| **s == GazeState::Saccade).count();
        assert_eq!(saccades, 1, "expected exactly one saccade, got {states:?}");
        assert_eq!(
            states[5],
            GazeState::Saccade,
            "the jump sample must be the saccade"
        );
    }
}
