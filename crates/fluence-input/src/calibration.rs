// SPDX-License-Identifier: Apache-2.0

//! Gaze→screen calibration (SPEC §4.D): the few-shot mapping from sensor
//! features to a normalized surface point, per context profile.
//!
//! v0 is a **ridge regression** (simple, debuggable — SPEC §4.C: the ONNX model
//! of the ML-gaze track only replaces it once it wins on our datasets). The
//! solver is a tiny dependency-free Gauss-Jordan elimination, so the whole thing
//! is pure, deterministic and replayable. The [`Calibrator`] collects pursuit
//! samples (initial/express calibration), fits the model, refines it **smoothly**
//! from uncorrected commits (continuous implicit calibration), and flags drift —
//! all of D-4.4 except the UI animation, which lives in the client.

use std::collections::VecDeque;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Default ridge penalty: enough to stay well-posed with few samples, small
/// enough not to bias an honest mapping.
pub const DEFAULT_LAMBDA: f64 = 1e-3;
/// Sliding window of uncorrected-commit pairs feeding continuous calibration.
const IMPLICIT_CAPACITY: usize = 256;
/// How much each smoothed update moves toward a fresh fit (no perceptible jump).
const UPDATE_SMOOTHING: f64 = 0.2;
/// Drift is judged over commits from the last 30 s (SPEC §4.D).
const DRIFT_WINDOW: Duration = Duration::from_secs(30);
/// Minimum commits in the window before drift can be judged.
const MIN_DRIFT_SAMPLES: usize = 8;
/// Drift threshold: median error above this multiple of the key size (SPEC §4.D).
const DRIFT_KEY_MULTIPLE: f64 = 1.5;

/// Solves `a · x = b` (a: n×n, b: n×m) by Gauss-Jordan with partial pivoting.
/// Returns the n×m solution, or `None` if `a` is singular or shapes mismatch.
fn solve(a: &[Vec<f64>], b: &[Vec<f64>]) -> Option<Vec<Vec<f64>>> {
    let n = a.len();
    if n == 0 || a.iter().any(|row| row.len() != n) || b.len() != n {
        return None;
    }
    let m = b[0].len();
    if b.iter().any(|row| row.len() != m) {
        return None;
    }
    let mut aug: Vec<Vec<f64>> = a
        .iter()
        .zip(b)
        .map(|(arow, brow)| [arow.as_slice(), brow.as_slice()].concat())
        .collect();

    for col in 0..n {
        let pivot = (col..n).max_by(|&r, &s| aug[r][col].abs().total_cmp(&aug[s][col].abs()))?;
        if aug[pivot][col].abs() < 1e-12 {
            return None; // singular
        }
        aug.swap(col, pivot);
        let pivot_value = aug[col][col];
        for value in &mut aug[col][col..] {
            *value /= pivot_value;
        }
        for row in 0..n {
            if row != col {
                let factor = aug[row][col];
                if factor != 0.0 {
                    for j in col..n + m {
                        aug[row][j] -= factor * aug[col][j];
                    }
                }
            }
        }
    }
    Some(aug.into_iter().map(|row| row[n..n + m].to_vec()).collect())
}

/// A fitted ridge mapping from a `feature_dim`-vector to a normalized point.
/// Weights are over the bias-augmented features (`weights[0]` is the bias).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RidgeModel {
    /// `[wx, wy]` per augmented feature; length is `feature_dim + 1`.
    weights: Vec<[f64; 2]>,
    /// Ridge penalty used for the fit (recorded for reproducibility).
    lambda: f64,
    /// Raw feature dimension (without the bias term).
    feature_dim: usize,
}

impl RidgeModel {
    /// Fits a ridge model from `(features, target)` pairs. `None` if empty, if
    /// feature lengths disagree, or if the normal equations are singular.
    #[must_use]
    pub fn fit(samples: &[(Vec<f64>, (f64, f64))], lambda: f64) -> Option<Self> {
        let feature_dim = samples.first()?.0.len();
        if samples.iter().any(|(f, _)| f.len() != feature_dim) {
            return None;
        }
        let dim = feature_dim + 1; // + bias
        // Normal equations: gram = XᵀX (+ λI), rhs = XᵀY.
        let mut gram = vec![vec![0.0; dim]; dim];
        let mut rhs = vec![vec![0.0; 2]; dim];
        for (features, (tx, ty)) in samples {
            let mut x = Vec::with_capacity(dim);
            x.push(1.0);
            x.extend_from_slice(features);
            for i in 0..dim {
                for j in 0..dim {
                    gram[i][j] += x[i] * x[j];
                }
                rhs[i][0] += x[i] * tx;
                rhs[i][1] += x[i] * ty;
            }
        }
        for (i, row) in gram.iter_mut().enumerate() {
            row[i] += lambda;
        }
        let weights = solve(&gram, &rhs)?
            .into_iter()
            .map(|row| [row[0], row[1]])
            .collect();
        Some(Self {
            weights,
            lambda,
            feature_dim,
        })
    }

    /// Maps a feature vector to a normalized point. `None` if the dimension
    /// differs from the fitted one. The output is **not** clamped (the caller
    /// hit-tests; an off-surface prediction is meaningful for drift).
    #[must_use]
    pub fn predict(&self, features: &[f64]) -> Option<(f64, f64)> {
        if features.len() != self.feature_dim {
            return None;
        }
        let mut x = self.weights[0][0];
        let mut y = self.weights[0][1];
        for (weight, &feature) in self.weights[1..].iter().zip(features) {
            x += weight[0] * feature;
            y += weight[1] * feature;
        }
        Some((x, y))
    }

    /// A convex blend `(1-η)·self + η·other`, for smoothed updates. Returns
    /// `self` unchanged if the dimensions differ (no perceptible jump either way).
    #[must_use]
    fn blend(&self, other: &Self, eta: f64) -> Self {
        if other.feature_dim != self.feature_dim {
            return self.clone();
        }
        let weights = self
            .weights
            .iter()
            .zip(&other.weights)
            .map(|(a, b)| {
                [
                    (1.0 - eta) * a[0] + eta * b[0],
                    (1.0 - eta) * a[1] + eta * b[1],
                ]
            })
            .collect();
        Self {
            weights,
            lambda: self.lambda,
            feature_dim: self.feature_dim,
        }
    }
}

/// Current schema version of a persisted [`CalibrationProfile`].
pub const CALIBRATION_PROFILE_VERSION: u32 = 1;

/// A named, versioned, persistable calibration (per context: bed/chair/…,
/// SPEC §4.D). The hub stores these and switches between them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CalibrationProfile {
    /// Schema version (forward-compatible persistence).
    pub version: u32,
    /// Human name of the context ("lit", "fauteuil", …).
    pub name: String,
    /// The fitted mapping.
    pub model: RidgeModel,
    /// Estimated error at fit time (normalized RMS) — the visible quality.
    pub rms_error: f64,
}

fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Collects calibration samples, fits and smoothly refines the mapping, and
/// flags drift — one per context profile (SPEC §4.D).
#[derive(Debug, Clone)]
pub struct Calibrator {
    name: String,
    lambda: f64,
    pursuit: Vec<(Vec<f64>, (f64, f64))>,
    implicit: VecDeque<(Vec<f64>, (f64, f64))>,
    model: Option<RidgeModel>,
    rms_error: f64,
    recent_errors: VecDeque<(Duration, f64)>,
}

impl Calibrator {
    /// A fresh, uncalibrated context profile.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            lambda: DEFAULT_LAMBDA,
            pursuit: Vec::new(),
            implicit: VecDeque::new(),
            model: None,
            rms_error: f64::INFINITY,
            recent_errors: VecDeque::new(),
        }
    }

    /// Adds one labelled calibration sample (smooth-pursuit / express points):
    /// the sensor `features` observed while the user looked at `target`.
    pub fn add_sample(&mut self, features: Vec<f64>, target: (f64, f64)) {
        self.pursuit.push((features, target));
    }

    /// Fits the mapping from the collected pursuit samples and returns the
    /// estimated error (normalized RMS over the samples), or `None` if it could
    /// not fit (too few samples / singular).
    pub fn fit(&mut self) -> Option<f64> {
        let model = RidgeModel::fit(&self.pursuit, self.lambda)?;
        self.rms_error = rms_error(&model, &self.pursuit);
        self.model = Some(model);
        Some(self.rms_error)
    }

    /// Maps features to a normalized point, if calibrated.
    #[must_use]
    pub fn predict(&self, features: &[f64]) -> Option<(f64, f64)> {
        self.model.as_ref()?.predict(features)
    }

    /// The current estimated error (normalized RMS) — the quality shown to the
    /// caregiver in real time (SPEC §4.D). `None` until first fit.
    #[must_use]
    pub fn quality(&self) -> Option<f64> {
        self.model.is_some().then_some(self.rms_error)
    }

    /// Folds a `sel.commit` into continuous calibration (SPEC §4.D): records the
    /// prediction error (for drift), and — unless the user corrected it (an erase
    /// within ~3 s) — adds `(features → target_center)` to a sliding window and
    /// **smoothly** re-fits, so the mapping never jumps perceptibly.
    pub fn observe_commit(
        &mut self,
        features: &[f64],
        target_center: (f64, f64),
        corrected: bool,
        now: Duration,
    ) {
        if let Some(prediction) = self.predict(features) {
            self.recent_errors
                .push_back((now, distance(prediction, target_center)));
            self.prune_errors(now);
        }
        if corrected {
            return; // a corrected commit is not a clean training pair
        }
        self.implicit.push_back((features.to_vec(), target_center));
        while self.implicit.len() > IMPLICIT_CAPACITY {
            self.implicit.pop_front();
        }
        self.refit_smoothed();
    }

    /// Re-fits from pursuit + implicit samples and blends toward the fresh fit.
    fn refit_smoothed(&mut self) {
        let mut samples = self.pursuit.clone();
        samples.extend(self.implicit.iter().cloned());
        let Some(fresh) = RidgeModel::fit(&samples, self.lambda) else {
            return;
        };
        let blended = match &self.model {
            Some(old) => old.blend(&fresh, UPDATE_SMOOTHING),
            None => fresh,
        };
        self.rms_error = rms_error(&blended, &samples);
        self.model = Some(blended);
    }

    fn prune_errors(&mut self, now: Duration) {
        while let Some(&(t, _)) = self.recent_errors.front() {
            if now.saturating_sub(t) > DRIFT_WINDOW {
                self.recent_errors.pop_front();
            } else {
                break;
            }
        }
    }

    /// Whether the mapping has drifted (SPEC §4.D): the median commit error over
    /// the last 30 s exceeds `1.5 ×` the key size, with enough samples spanning a
    /// near-full window. A discreet express-recalibration is then proposed — never
    /// an authoritarian interruption.
    #[must_use]
    pub fn drift_suspected(&self, now: Duration, key_size: f64) -> bool {
        let window: Vec<f64> = self
            .recent_errors
            .iter()
            .filter(|(t, _)| now.saturating_sub(*t) <= DRIFT_WINDOW)
            .map(|(_, e)| *e)
            .collect();
        if window.len() < MIN_DRIFT_SAMPLES {
            return false;
        }
        let Some((oldest, _)) = self.recent_errors.front() else {
            return false;
        };
        if now.saturating_sub(*oldest) < DRIFT_WINDOW.mul_f64(0.8) {
            return false; // the high error has not persisted long enough yet
        }
        median(&window) > DRIFT_KEY_MULTIPLE * key_size
    }

    /// Exports the current calibration as a versioned, persistable profile.
    #[must_use]
    pub fn profile(&self) -> Option<CalibrationProfile> {
        Some(CalibrationProfile {
            version: CALIBRATION_PROFILE_VERSION,
            name: self.name.clone(),
            model: self.model.clone()?,
            rms_error: self.rms_error,
        })
    }

    /// Restores a calibrator from a persisted profile (model only; sample
    /// buffers start empty and continuous calibration resumes from here).
    #[must_use]
    pub fn from_profile(profile: CalibrationProfile) -> Self {
        Self {
            name: profile.name,
            lambda: profile.model.lambda,
            pursuit: Vec::new(),
            implicit: VecDeque::new(),
            model: Some(profile.model),
            rms_error: profile.rms_error,
            recent_errors: VecDeque::new(),
        }
    }
}

/// Root-mean-square prediction error of `model` over `samples` (normalized).
fn rms_error(model: &RidgeModel, samples: &[(Vec<f64>, (f64, f64))]) -> f64 {
    if samples.is_empty() {
        return f64::INFINITY;
    }
    let sum: f64 = samples
        .iter()
        .filter_map(|(f, target)| model.predict(f).map(|p| distance(p, *target).powi(2)))
        .sum();
    let count = f64::from(u32::try_from(samples.len()).unwrap_or(u32::MAX));
    (sum / count).sqrt()
}

/// Median of a non-empty slice (the caller guarantees non-empty).
fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        f64::midpoint(sorted[mid - 1], sorted[mid])
    } else {
        sorted[mid]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// Deterministic pseudo-features so the tests are reproducible without RNG.
    fn features(i: usize) -> Vec<f64> {
        let a = f64::from(u16::try_from(i % 17).unwrap_or(0)) / 17.0;
        let b = f64::from(u16::try_from((i * 7) % 23).unwrap_or(0)) / 23.0;
        vec![a, b]
    }

    /// Ground-truth affine map features → screen, the regression should recover.
    fn truth(f: &[f64]) -> (f64, f64) {
        (
            0.1 + 0.8 * f[0] + 0.05 * f[1],
            0.2 + 0.1 * f[0] + 0.7 * f[1],
        )
    }

    #[test]
    fn solve_recovers_a_known_system() {
        // [[2,1],[1,3]] x = [[1],[2]] ⇒ x = [[0.2],[0.6]].
        let x = solve(&[vec![2.0, 1.0], vec![1.0, 3.0]], &[vec![1.0], vec![2.0]]).unwrap();
        assert!((x[0][0] - 0.2).abs() < 1e-9 && (x[1][0] - 0.6).abs() < 1e-9);
    }

    #[test]
    fn solve_rejects_a_singular_matrix() {
        assert!(solve(&[vec![1.0, 2.0], vec![2.0, 4.0]], &[vec![1.0], vec![2.0]]).is_none());
    }

    #[test]
    fn ridge_recovers_an_affine_mapping() {
        let samples: Vec<_> = (0..40)
            .map(|i| (features(i), truth(&features(i))))
            .collect();
        let model = RidgeModel::fit(&samples, 1e-6).unwrap();
        for i in 100..110 {
            let (px, py) = model.predict(&features(i)).unwrap();
            let (tx, ty) = truth(&features(i));
            assert!(
                (px - tx).abs() < 1e-2 && (py - ty).abs() < 1e-2,
                "off at {i}"
            );
        }
    }

    #[test]
    fn a_larger_lambda_shrinks_the_mapping_toward_its_mean() {
        let samples: Vec<_> = (0..40)
            .map(|i| (features(i), truth(&features(i))))
            .collect();
        let slope = |lambda: f64| {
            let m = RidgeModel::fit(&samples, lambda).unwrap();
            m.predict(&[1.0, 0.0]).unwrap().0 - m.predict(&[0.0, 0.0]).unwrap().0
        };
        assert!(
            slope(10.0).abs() < slope(1e-6).abs(),
            "ridge did not shrink the slope"
        );
    }

    #[test]
    fn calibrator_fits_and_predicts() {
        let mut cal = Calibrator::new("lit");
        assert!(cal.quality().is_none());
        for i in 0..40 {
            cal.add_sample(features(i), truth(&features(i)));
        }
        let rms = cal.fit().unwrap();
        assert!(rms < 1e-2, "fit quality too poor: {rms}");
        let (px, _) = cal.predict(&features(200)).unwrap();
        assert!((px - truth(&features(200)).0).abs() < 1e-2);
        assert!(cal.quality().unwrap() < 1e-2);
    }

    #[test]
    fn continuous_calibration_improves_a_biased_mapping_smoothly() {
        // Fit a deliberately biased model, then feed clean commits: the error to
        // ground truth shrinks, and each step is a bounded (smooth) change.
        let mut cal = Calibrator::new("fauteuil");
        for i in 0..40 {
            let (tx, ty) = truth(&features(i));
            cal.add_sample(features(i), (tx + 0.1, ty)); // biased by +0.1 in x
        }
        cal.fit().unwrap();
        let before = (cal.predict(&features(500)).unwrap().0 - truth(&features(500)).0).abs();
        let mut t = 0;
        for i in 0..200 {
            cal.observe_commit(&features(i), truth(&features(i)), false, ms(t));
            t += 50;
        }
        let after = (cal.predict(&features(500)).unwrap().0 - truth(&features(500)).0).abs();
        assert!(
            after < before,
            "continuous calibration did not improve ({after} !< {before})"
        );
    }

    #[test]
    fn a_corrected_commit_does_not_train_the_mapping() {
        let mut cal = Calibrator::new("lit");
        for i in 0..40 {
            cal.add_sample(features(i), truth(&features(i)));
        }
        cal.fit().unwrap();
        let before = cal.predict(&features(7)).unwrap();
        // A corrected commit at a wild target must not move the mapping.
        cal.observe_commit(&features(7), (0.99, 0.99), true, ms(0));
        assert_eq!(cal.predict(&features(7)).unwrap(), before);
    }

    #[test]
    fn drift_is_flagged_only_after_sustained_large_error() {
        let mut cal = Calibrator::new("lit");
        for i in 0..40 {
            cal.add_sample(features(i), truth(&features(i)));
        }
        cal.fit().unwrap();
        // Small errors over 30 s ⇒ no drift.
        let mut t = 0u64;
        for i in 0..40 {
            cal.observe_commit(&features(i), truth(&features(i)), true, ms(t));
            t += 800;
        }
        assert!(!cal.drift_suspected(ms(t), 0.05));

        // Now sustained large errors (the prediction is far from the target) for
        // > 30 s ⇒ drift suspected.
        let mut drifted = Calibrator::new("lit");
        for i in 0..40 {
            drifted.add_sample(features(i), truth(&features(i)));
        }
        drifted.fit().unwrap();
        let mut t = 0u64;
        for i in 0..40 {
            let (tx, ty) = truth(&features(i));
            // Report the commit as landing 0.2 away from where the model predicts.
            drifted.observe_commit(&features(i), (tx + 0.2, ty + 0.2), true, ms(t));
            t += 1000;
        }
        assert!(
            drifted.drift_suspected(ms(t), 0.05),
            "sustained error should flag drift"
        );
    }

    #[test]
    fn profile_round_trips_through_serde() {
        let mut cal = Calibrator::new("lit");
        for i in 0..40 {
            cal.add_sample(features(i), truth(&features(i)));
        }
        cal.fit().unwrap();
        let profile = cal.profile().unwrap();
        let json = serde_json::to_string(&profile).unwrap();
        let restored: CalibrationProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, profile);
        assert_eq!(restored.version, CALIBRATION_PROFILE_VERSION);
        // A calibrator restored from the profile predicts identically.
        let back = Calibrator::from_profile(restored);
        assert_eq!(back.predict(&features(3)), cal.predict(&features(3)));
    }
}
