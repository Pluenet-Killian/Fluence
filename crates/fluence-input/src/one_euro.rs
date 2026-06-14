// SPDX-License-Identifier: Apache-2.0

//! One Euro filter (Casiez, Roussel & Vogel, CHI 2012) — the real-time pointing
//! standard: smooth at rest, responsive in motion (SPEC §4.C step 2).
//!
//! A speed-adaptive low-pass: the cutoff frequency rises with the signal's
//! speed, so slow movements are heavily smoothed (jitter removed) while fast
//! ones keep low lag. Two parameters tune it, exposed per profile (SPEC §4.C):
//! `min_cutoff` (`f_cmin`, the floor cutoff at rest) and `beta` (`β`, how much
//! speed raises the cutoff). The filter is **clock-free**: every sample carries
//! its own monotonic timestamp, so a replay is deterministic (PLAN 5.1).

use std::f64::consts::PI;
use std::time::Duration;

/// One Euro parameters (per context profile, SPEC §4.C).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OneEuroConfig {
    /// Minimum cutoff frequency (Hz). Lower ⇒ smoother at rest, more lag.
    pub min_cutoff: f64,
    /// Speed coefficient `β`. Higher ⇒ less lag during fast motion.
    pub beta: f64,
    /// Cutoff frequency (Hz) for the speed (derivative) low-pass.
    pub d_cutoff: f64,
}

impl Default for OneEuroConfig {
    fn default() -> Self {
        // CHI 2012 defaults, tuned for a normalized [0, 1] pointing surface.
        Self {
            min_cutoff: 1.0,
            beta: 0.007,
            d_cutoff: 1.0,
        }
    }
}

/// Smoothing factor for a first-order low-pass at `cutoff` Hz over step `dt` (s).
fn smoothing_alpha(cutoff: f64, dt: f64) -> f64 {
    let tau = 1.0 / (2.0 * PI * cutoff);
    1.0 / (1.0 + tau / dt)
}

/// Exponential low-pass holding its last output.
#[derive(Debug, Default)]
struct LowPass {
    last: Option<f64>,
}

impl LowPass {
    fn filter(&mut self, value: f64, alpha: f64) -> f64 {
        let filtered = match self.last {
            Some(prev) => alpha * value + (1.0 - alpha) * prev,
            None => value,
        };
        self.last = Some(filtered);
        filtered
    }
}

/// One Euro filter for a single scalar channel.
#[derive(Debug)]
pub struct OneEuro {
    config: OneEuroConfig,
    value: LowPass,
    speed: LowPass,
    last_t: Option<Duration>,
    last_value: Option<f64>,
}

impl OneEuro {
    /// A fresh filter; the first sample passes through unchanged.
    #[must_use]
    pub fn new(config: OneEuroConfig) -> Self {
        Self {
            config,
            value: LowPass::default(),
            speed: LowPass::default(),
            last_t: None,
            last_value: None,
        }
    }

    /// Filters `value` sampled at monotonic time `t`. The first sample (and any
    /// sample whose time does not advance) is returned unchanged and seeds the
    /// state, so the filter is total and never divides by a zero `dt`.
    #[must_use]
    pub fn filter(&mut self, value: f64, t: Duration) -> f64 {
        let Some(dt) = self.last_t.and_then(|prev| t.checked_sub(prev)) else {
            return self.seed(value, t);
        };
        let dt = dt.as_secs_f64();
        if dt <= 0.0 {
            return self.seed(value, t);
        }
        let prev_value = self.last_value.unwrap_or(value);
        self.last_t = Some(t);
        self.last_value = Some(value);

        let speed = (value - prev_value) / dt;
        let smoothed_speed = self
            .speed
            .filter(speed, smoothing_alpha(self.config.d_cutoff, dt));
        let cutoff = self.config.min_cutoff + self.config.beta * smoothed_speed.abs();
        self.value.filter(value, smoothing_alpha(cutoff, dt))
    }

    /// Seeds the state with a pass-through sample (first sample / stalled clock).
    fn seed(&mut self, value: f64, t: Duration) -> f64 {
        self.last_t = Some(t);
        self.last_value = Some(value);
        self.value.last = Some(value);
        self.speed.last = Some(0.0);
        value
    }
}

/// One Euro filter for a 2-D point (independent X/Y channels, shared config).
#[derive(Debug)]
pub struct OneEuro2D {
    x: OneEuro,
    y: OneEuro,
}

impl OneEuro2D {
    /// A fresh 2-D filter.
    #[must_use]
    pub fn new(config: OneEuroConfig) -> Self {
        Self {
            x: OneEuro::new(config),
            y: OneEuro::new(config),
        }
    }

    /// Filters point `(x, y)` sampled at monotonic time `t`.
    #[must_use]
    pub fn filter(&mut self, x: f64, y: f64, t: Duration) -> (f64, f64) {
        (self.x.filter(x, t), self.y.filter(y, t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    #[test]
    fn first_sample_passes_through() {
        let mut filter = OneEuro::new(OneEuroConfig::default());
        assert!((filter.filter(0.42, ms(0)) - 0.42).abs() < 1e-12);
    }

    #[test]
    fn a_constant_signal_stays_constant() {
        // At rest, speed is 0, cutoff is min_cutoff, and a constant in/constant
        // out: no jitter is invented (the whole point of the floor cutoff).
        let mut filter = OneEuro::new(OneEuroConfig::default());
        let mut t = 0;
        for _ in 0..50 {
            let out = filter.filter(0.5, ms(t));
            assert!((out - 0.5).abs() < 1e-9, "constant signal drifted to {out}");
            t += 16;
        }
    }

    #[test]
    fn a_step_is_approached_monotonically_without_overshoot() {
        // Settled at 0, then the input jumps to 1: the output rises monotonically
        // toward 1 and never exceeds it (a low-pass cannot overshoot a step).
        let mut filter = OneEuro::new(OneEuroConfig::default());
        let mut t = 0;
        for _ in 0..5 {
            let _ = filter.filter(0.0, ms(t));
            t += 16;
        }
        let mut prev = 0.0;
        for _ in 0..40 {
            let out = filter.filter(1.0, ms(t));
            assert!(out >= prev - 1e-9, "non-monotonic: {out} < {prev}");
            assert!(out <= 1.0 + 1e-9, "overshoot: {out} > 1");
            prev = out;
            t += 16;
        }
        assert!(prev > 0.9, "step never converged (reached {prev})");
    }

    #[test]
    fn higher_beta_tracks_fast_motion_with_less_lag() {
        // On a fast ramp, a larger beta raises the cutoff with speed, so it lags
        // the true value less than a small beta does.
        let ramp = |beta: f64| {
            let mut filter = OneEuro::new(OneEuroConfig {
                min_cutoff: 1.0,
                beta,
                d_cutoff: 1.0,
            });
            let mut out = 0.0;
            let mut t = 0;
            for step in 0..20 {
                out = filter.filter(f64::from(step) * 0.05, ms(t));
                t += 16;
            }
            out
        };
        let truth = 19.0 * 0.05;
        let lag_small = (truth - ramp(0.0)).abs();
        let lag_large = (truth - ramp(1.0)).abs();
        assert!(
            lag_large < lag_small,
            "higher beta did not reduce lag ({lag_large} !< {lag_small})"
        );
    }

    #[test]
    fn non_increasing_time_is_total_and_passes_through() {
        let mut filter = OneEuro::new(OneEuroConfig::default());
        let _ = filter.filter(0.1, ms(10));
        // Same timestamp again: no division by zero, value passes through.
        assert!((filter.filter(0.9, ms(10)) - 0.9).abs() < 1e-12);
    }

    proptest! {
        /// The filtered value never leaves the convex hull of the inputs seen so
        /// far (a convex low-pass cannot exceed its inputs' range).
        #[test]
        fn output_stays_within_input_range(
            samples in proptest::collection::vec(0.0f64..=1.0, 1..50)
        ) {
            let mut filter = OneEuro2D::new(OneEuroConfig::default());
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            for (i, &s) in samples.iter().enumerate() {
                lo = lo.min(s);
                hi = hi.max(s);
                let (fx, _) = filter.filter(s, s, ms(i as u64 * 16));
                prop_assert!(fx >= lo - 1e-9 && fx <= hi + 1e-9, "{fx} outside [{lo}, {hi}]");
            }
        }
    }
}
