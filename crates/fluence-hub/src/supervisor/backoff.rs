// SPDX-License-Identifier: Apache-2.0

//! Restart backoff: exponential with jitter, pure and deterministic in
//! tests (the jitter source is injected — ADR-0005 §8).

use std::time::Duration;

/// Exponential backoff: `base × factor^attempt`, capped, ±25 % jitter.
#[derive(Debug, Clone)]
pub struct Backoff {
    base: Duration,
    factor: f64,
    max: Duration,
    attempt: u32,
}

/// Jitter fraction applied around the raw delay.
const JITTER_FRACTION: f64 = 0.25;

impl Backoff {
    /// Supervisor profile (SPEC §2.C restart chain): 200 ms, ×2, cap 10 s.
    #[must_use]
    pub fn supervisor() -> Self {
        Self {
            base: Duration::from_millis(200),
            factor: 2.0,
            max: Duration::from_secs(10),
            attempt: 0,
        }
    }

    /// Next delay. `jitter` must be in `[-1, 1]` (clamped): inject a real
    /// random in production, a constant in tests.
    pub fn next_delay(&mut self, jitter: f64) -> Duration {
        let jitter = jitter.clamp(-1.0, 1.0);
        let exponent = i32::try_from(self.attempt).unwrap_or(i32::MAX);
        let raw = self.base.as_secs_f64() * self.factor.powi(exponent);
        let capped = raw.min(self.max.as_secs_f64());
        let jittered = capped * JITTER_FRACTION.mul_add(jitter, 1.0);
        self.attempt = self.attempt.saturating_add(1);
        Duration::from_secs_f64(jittered.max(0.0))
    }

    /// Number of delays handed out since the last reset (the restart
    /// counter exposed in `/system/health`).
    #[must_use]
    pub fn attempts(&self) -> u32 {
        self.attempt
    }

    /// Resets after a sustained healthy period.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_from_base_and_caps() {
        let mut backoff = Backoff::supervisor();
        // PLAN T1: « backoff calculé » — exact values, no jitter.
        assert_eq!(backoff.next_delay(0.0), Duration::from_millis(200));
        assert_eq!(backoff.next_delay(0.0), Duration::from_millis(400));
        assert_eq!(backoff.next_delay(0.0), Duration::from_millis(800));
        for _ in 0..10 {
            let _ = backoff.next_delay(0.0);
        }
        assert_eq!(backoff.next_delay(0.0), Duration::from_secs(10), "capped");
    }

    #[test]
    fn jitter_spreads_within_25_percent() {
        let mut up = Backoff::supervisor();
        let mut down = Backoff::supervisor();
        assert_eq!(up.next_delay(1.0), Duration::from_millis(250));
        assert_eq!(down.next_delay(-1.0), Duration::from_millis(150));
    }

    #[test]
    fn out_of_range_jitter_is_clamped() {
        let mut backoff = Backoff::supervisor();
        assert_eq!(backoff.next_delay(50.0), Duration::from_millis(250));
    }

    #[test]
    fn reset_restarts_the_progression() {
        let mut backoff = Backoff::supervisor();
        let _ = backoff.next_delay(0.0);
        let _ = backoff.next_delay(0.0);
        assert_eq!(backoff.attempts(), 2);
        backoff.reset();
        assert_eq!(backoff.attempts(), 0);
        assert_eq!(backoff.next_delay(0.0), Duration::from_millis(200));
    }
}
