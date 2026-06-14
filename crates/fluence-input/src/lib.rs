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
//! v0 ([`SelectionEngine`]) covers target hit-testing and fixed/adaptive dwell
//! driven by the `mouse` source. It is **clock-free**: every method takes a
//! monotonic `now`, so the loop replays deterministically (PLAN 5.1). The hub
//! stamps the emitted [`SelectionUpdate`]s into the protocol's `SelectionEvent`
//! when relaying. Fixation/saccade gating (I-VT), multi-source fusion and
//! calibration arrive with webcam gaze (Phase 6); for now every on-target
//! sample counts as a fixation.

mod fixation;
mod fusion;
mod geometry;
mod one_euro;

use std::time::Duration;

use fluence_protocol::Normalized;
use fluence_protocol::input::{CommitMethod, Target, TargetMap, TargetMapPatch, Viewport};

pub use fixation::{GazeState, IvtClassifier, IvtConfig};
pub use fusion::{
    FusionConfig, Magnet, MagnetismConfig, NoiseModel, apply_magnetism, fuse_confidence_weighted,
    head_affine,
};
pub use geometry::hit_test;
pub use one_euro::{OneEuro, OneEuro2D, OneEuroConfig};

/// Dwell selection parameters (SPEC §4.A, §4.C).
#[derive(Debug, Clone, Copy)]
pub struct DwellConfig {
    /// Base dwell duration at a neutral prior.
    pub base: Duration,
    /// After a commit, no new selection until this elapses (anti-retrigger).
    pub cooldown: Duration,
    /// Adaptive modulation as a fraction of `base` (`0.4` = ±40%, SPEC §4.C):
    /// a high-prior target commits faster, a low-prior one slower.
    pub adapt_fraction: f64,
    /// Safety floor: the adaptive duration is never shorter than this, so a
    /// confident prior can never make a key impossible to dwell deliberately
    /// (agency over magnetism, SPEC §4.C).
    pub floor: Duration,
}

impl Default for DwellConfig {
    fn default() -> Self {
        Self {
            base: Duration::from_millis(800),
            cooldown: Duration::from_millis(300),
            adapt_fraction: 0.4,
            floor: Duration::from_millis(250),
        }
    }
}

/// The dwell duration for a target with linguistic `prior` in `[0, 1]`
/// (`None` = neutral `0.5`). Higher prior → shorter dwell, bounded to
/// ±`adapt_fraction` of `base` and never below `floor` (SPEC §4.C).
#[must_use]
pub fn adaptive_dwell(config: &DwellConfig, prior: Option<f64>) -> Duration {
    let prior = prior.unwrap_or(0.5).clamp(0.0, 1.0);
    // prior 1 → factor (1 - adapt) (fastest); prior 0 → (1 + adapt) (slowest);
    // prior 0.5 → 1.0 (base).
    let factor = 1.0 - config.adapt_fraction * (2.0 * prior - 1.0);
    let scaled = (config.base.as_secs_f64() * factor).max(config.floor.as_secs_f64());
    // `scaled` is finite and non-negative; the fallback keeps us total anyway.
    Duration::try_from_secs_f64(scaled).unwrap_or(config.floor)
}

/// A selection decision the engine emits — timestamp-free. The hub stamps these
/// and maps them onto the protocol's `SelectionEvent` (SPEC §4.A) before
/// relaying to UIs.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectionUpdate {
    /// Focus entered a target.
    Focus {
        /// The newly focused target.
        target: fluence_protocol::TargetId,
    },
    /// Dwell progress on the focused target (drives the UI gauge).
    Dwell {
        /// Target being dwelled on.
        target: fluence_protocol::TargetId,
        /// Progress in `[0, 1)`.
        progress: f64,
        /// Estimated time remaining before commit.
        eta: Duration,
    },
    /// A selection committed.
    Commit {
        /// Committed target.
        target: fluence_protocol::TargetId,
        /// How it was committed.
        method: CommitMethod,
    },
    /// Focus/dwell was cancelled (left the target, signal lost, targets changed).
    Cancel,
}

/// Where the engine's attention currently is.
enum FocusState {
    /// Nothing focused.
    Idle,
    /// Accumulating fixation time on `target`.
    Dwelling {
        target: fluence_protocol::TargetId,
        required: Duration,
        accumulated: Duration,
    },
    /// Suppressing input until `until` (post-commit anti-retrigger).
    Cooldown { until: Duration },
}

/// The hub-side selection engine for one surface (SPEC §4.A, stage 2): it holds
/// the declared targets, hit-tests normalized pointer samples, and runs the
/// dwell state machine. See the crate docs for the clock-free contract.
pub struct SelectionEngine {
    config: DwellConfig,
    viewport: Viewport,
    targets: Vec<Target>,
    focus: FocusState,
    last_now: Option<Duration>,
}

impl SelectionEngine {
    /// A fresh engine with no targets.
    #[must_use]
    pub fn new(config: DwellConfig) -> Self {
        Self {
            config,
            viewport: Viewport { w: 0, h: 0 },
            targets: Vec::new(),
            focus: FocusState::Idle,
            last_now: None,
        }
    }

    /// Replaces the surface's targets (`PUT /input/targets`). Any dwell in
    /// progress is cancelled, since the focused target may be gone.
    #[must_use]
    pub fn set_targets(&mut self, map: &TargetMap) -> Vec<SelectionUpdate> {
        self.viewport = map.viewport;
        self.targets.clone_from(&map.targets);
        let was_dwelling = matches!(self.focus, FocusState::Dwelling { .. });
        self.focus = FocusState::Idle;
        if was_dwelling {
            vec![SelectionUpdate::Cancel]
        } else {
            Vec::new()
        }
    }

    /// Applies an incremental patch (`upsert` then `remove`, SPEC §4.A). Cancels
    /// the dwell only if its target is removed.
    #[must_use]
    pub fn apply_patch(&mut self, patch: &TargetMapPatch) -> Vec<SelectionUpdate> {
        if let Some(viewport) = patch.viewport {
            self.viewport = viewport;
        }
        for target in &patch.upsert {
            if let Some(existing) = self.targets.iter_mut().find(|t| t.id == target.id) {
                *existing = target.clone();
            } else {
                self.targets.push(target.clone());
            }
        }
        if !patch.remove.is_empty() {
            self.targets.retain(|t| !patch.remove.contains(&t.id));
        }
        if let FocusState::Dwelling { target, .. } = &self.focus
            && !self.targets.iter().any(|t| &t.id == target)
        {
            self.focus = FocusState::Idle;
            return vec![SelectionUpdate::Cancel];
        }
        Vec::new()
    }

    /// Processes a pointer sample at monotonic time `now` (normalized `x`, `y`
    /// in `[0, 1]`), advancing the dwell state machine.
    #[must_use]
    pub fn on_pointer(&mut self, x: f64, y: f64, now: Duration) -> Vec<SelectionUpdate> {
        let dt = self.tick(now);

        // Cooldown gate: ignore everything until it elapses.
        let cooldown_until = match &self.focus {
            FocusState::Cooldown { until } => Some(*until),
            _ => None,
        };
        if let Some(until) = cooldown_until {
            if now < until {
                return Vec::new();
            }
            self.focus = FocusState::Idle;
        }

        let hit = hit_test(self.viewport, &self.targets, x, y);
        let hit_id = hit.map(|t| t.id.clone());
        let hit_prior = hit.and_then(|t| t.prior).map(Normalized::get);

        match std::mem::replace(&mut self.focus, FocusState::Idle) {
            FocusState::Idle | FocusState::Cooldown { .. } => match hit_id {
                None => Vec::new(),
                Some(id) => self.enter_dwell(id, hit_prior),
            },
            FocusState::Dwelling {
                target,
                required,
                accumulated,
            } => match hit_id {
                None => vec![SelectionUpdate::Cancel],
                Some(id) if id == target => {
                    let accumulated = (accumulated + dt).min(required);
                    if accumulated >= required {
                        self.focus = FocusState::Cooldown {
                            until: now + self.config.cooldown,
                        };
                        vec![SelectionUpdate::Commit {
                            target,
                            method: CommitMethod::Dwell,
                        }]
                    } else {
                        let eta = required - accumulated;
                        let progress = accumulated.as_secs_f64() / required.as_secs_f64();
                        self.focus = FocusState::Dwelling {
                            target: target.clone(),
                            required,
                            accumulated,
                        };
                        vec![SelectionUpdate::Dwell {
                            target,
                            progress,
                            eta,
                        }]
                    }
                }
                Some(id) => {
                    // Moved to a different target: cancel, then focus the new one.
                    let mut events = vec![SelectionUpdate::Cancel];
                    events.extend(self.enter_dwell(id, hit_prior));
                    events
                }
            },
        }
    }

    /// A switch press commits the focused target immediately (SPEC §4.A); with
    /// nothing focused it does nothing.
    #[must_use]
    pub fn on_switch(&mut self, now: Duration) -> Vec<SelectionUpdate> {
        let _ = self.tick(now);
        match std::mem::replace(&mut self.focus, FocusState::Idle) {
            FocusState::Dwelling { target, .. } => {
                self.focus = FocusState::Cooldown {
                    until: now + self.config.cooldown,
                };
                vec![SelectionUpdate::Commit {
                    target,
                    method: CommitMethod::Switch,
                }]
            }
            // Idle / Cooldown: restore unchanged, nothing to commit.
            other => {
                self.focus = other;
                Vec::new()
            }
        }
    }

    /// Starts dwelling on `id`, emitting Focus and a zero-progress Dwell.
    fn enter_dwell(
        &mut self,
        id: fluence_protocol::TargetId,
        prior: Option<f64>,
    ) -> Vec<SelectionUpdate> {
        let required = adaptive_dwell(&self.config, prior);
        self.focus = FocusState::Dwelling {
            target: id.clone(),
            required,
            accumulated: Duration::ZERO,
        };
        vec![
            SelectionUpdate::Focus { target: id.clone() },
            SelectionUpdate::Dwell {
                target: id,
                progress: 0.0,
                eta: required,
            },
        ]
    }

    /// Records `now`, returning the delta since the previous sample (zero on the
    /// first sample, or if time appears to go backwards).
    fn tick(&mut self, now: Duration) -> Duration {
        let dt = self
            .last_now
            .map_or(Duration::ZERO, |prev| now.saturating_sub(prev));
        self.last_now = Some(now);
        dt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fluence_protocol::input::TargetRole;
    use fluence_protocol::{SurfaceId, TargetId};
    use proptest::prelude::*;

    /// D-10.1: the input engine is a reusable brick, licensed Apache-2.0.
    #[test]
    fn crate_license_follows_d_10_1() {
        assert_eq!(env!("CARGO_PKG_LICENSE"), "Apache-2.0");
    }

    /// One target covering the whole 100×100 surface, with an optional prior.
    fn full_surface(id: &str, prior: Option<f64>) -> TargetMap {
        TargetMap {
            surface: SurfaceId::from("main"),
            viewport: Viewport { w: 100, h: 100 },
            targets: vec![Target {
                id: TargetId::from(id),
                rect: fluence_protocol::input::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 100.0,
                },
                role: TargetRole::Key,
                label: None,
                prior: prior.map(|p| Normalized::new(p).expect("test prior in range")),
            }],
        }
    }

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// Fixed-dwell config (no adaptation) for predictable timing assertions.
    fn fixed_config() -> DwellConfig {
        DwellConfig {
            base: ms(100),
            cooldown: ms(50),
            adapt_fraction: 0.0,
            floor: ms(10),
        }
    }

    #[test]
    fn focus_then_dwell_then_commit_after_required_fixation() {
        let mut engine = SelectionEngine::new(fixed_config());
        let _ = engine.set_targets(&full_surface("k", None));

        // First sample: Focus + zero-progress Dwell.
        let first = engine.on_pointer(0.5, 0.5, ms(0));
        assert!(matches!(first[0], SelectionUpdate::Focus { .. }));
        assert!(matches!(first[1], SelectionUpdate::Dwell { progress, .. } if progress < 1e-9));

        // Mid-dwell: progress strictly between 0 and 1, no commit yet.
        let mid = engine.on_pointer(0.5, 0.5, ms(60));
        assert!(
            matches!(&mid[..], [SelectionUpdate::Dwell { progress, .. }] if *progress > 0.0 && *progress < 1.0),
            "expected a single in-progress Dwell, got {mid:?}"
        );

        // Reaching the required fixation commits via dwell.
        let commit = engine.on_pointer(0.5, 0.5, ms(110));
        assert_eq!(
            commit,
            vec![SelectionUpdate::Commit {
                target: TargetId::from("k"),
                method: CommitMethod::Dwell,
            }]
        );
    }

    #[test]
    fn a_cooldown_follows_every_commit() {
        let mut engine = SelectionEngine::new(fixed_config());
        let _ = engine.set_targets(&full_surface("k", None));
        let _ = engine.on_pointer(0.5, 0.5, ms(0));
        let commit = engine.on_pointer(0.5, 0.5, ms(120));
        assert!(
            commit
                .iter()
                .any(|e| matches!(e, SelectionUpdate::Commit { .. }))
        );

        // Within the cooldown (until 120 + 50 = 170): no re-selection at all.
        assert!(engine.on_pointer(0.5, 0.5, ms(140)).is_empty());

        // After the cooldown: a fresh dwell may begin.
        let after = engine.on_pointer(0.5, 0.5, ms(180));
        assert!(
            matches!(after.first(), Some(SelectionUpdate::Focus { .. })),
            "after cooldown a new dwell should start, got {after:?}"
        );
    }

    #[test]
    fn leaving_the_target_cancels_the_dwell() {
        let mut engine = SelectionEngine::new(fixed_config());
        // One target on the left half only.
        let map = TargetMap {
            surface: SurfaceId::from("main"),
            viewport: Viewport { w: 100, h: 100 },
            targets: vec![Target {
                id: TargetId::from("k"),
                rect: fluence_protocol::input::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 50.0,
                    h: 100.0,
                },
                role: TargetRole::Key,
                label: None,
                prior: None,
            }],
        };
        let _ = engine.set_targets(&map);
        let _ = engine.on_pointer(0.2, 0.5, ms(0)); // on the target
        let off = engine.on_pointer(0.8, 0.5, ms(20)); // off the target
        assert_eq!(off, vec![SelectionUpdate::Cancel]);
    }

    #[test]
    fn a_switch_commits_the_focused_target_immediately() {
        let mut engine = SelectionEngine::new(fixed_config());
        let _ = engine.set_targets(&full_surface("k", None));
        let _ = engine.on_pointer(0.5, 0.5, ms(0)); // focus
        let commit = engine.on_switch(ms(5));
        assert_eq!(
            commit,
            vec![SelectionUpdate::Commit {
                target: TargetId::from("k"),
                method: CommitMethod::Switch,
            }]
        );
        // Nothing focused now: a second switch does nothing.
        assert!(engine.on_switch(ms(6)).is_empty());
    }

    proptest! {
        /// Never commit before the cumulative fixation reaches the requirement.
        #[test]
        fn never_commits_before_required_fixation(
            deltas in proptest::collection::vec(1u64..50, 1..20)
        ) {
            // required = base (adapt 0), 1000 ms. Accumulation counts deltas from
            // the second sample on, so a total under 1000 cannot commit.
            prop_assume!(deltas.iter().sum::<u64>() < 1000);
            let config = DwellConfig {
                base: ms(1000),
                cooldown: ms(100),
                adapt_fraction: 0.0,
                floor: ms(100),
            };
            let mut engine = SelectionEngine::new(config);
            let _ = engine.set_targets(&full_surface("k", None));
            let mut now = Duration::ZERO;
            let mut committed = false;
            for delta in deltas {
                now += ms(delta);
                if engine
                    .on_pointer(0.5, 0.5, now)
                    .iter()
                    .any(|e| matches!(e, SelectionUpdate::Commit { .. }))
                {
                    committed = true;
                }
            }
            prop_assert!(!committed, "committed with under {}ms of fixation", 1000);
        }

        /// Adaptive dwell stays within ±adapt_fraction of base and above floor.
        #[test]
        fn adaptive_dwell_is_bounded(prior in 0.0f64..=1.0) {
            let config = DwellConfig::default();
            let dwell = adaptive_dwell(&config, Some(prior));
            let upper = config.base.mul_f64(1.0 + config.adapt_fraction);
            let lower = config
                .base
                .mul_f64(1.0 - config.adapt_fraction)
                .max(config.floor);
            prop_assert!(dwell <= upper, "{dwell:?} exceeds upper bound {upper:?}");
            prop_assert!(dwell >= config.floor, "{dwell:?} below floor");
            // Tiny slack for the seconds-f64 round-trip.
            prop_assert!(dwell + Duration::from_micros(1) >= lower, "{dwell:?} below lower {lower:?}");
        }
    }
}
