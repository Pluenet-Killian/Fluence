// SPDX-License-Identifier: Apache-2.0

//! `latency` — measure the §5.A latency budgets and gate them (PLAN 7.6, A1).
//!
//! Tiers (PLAN §0.8): `provisional` multiplies the contractual budgets by 2.5
//! (CI runners are slower and shared); `contractual` uses them as-is on the
//! FLU-REF reference machine. Budgets are read from the contract
//! (`LatencyClass::budget_ms`) — the single source of truth, never re-typed here.
//!
//! What is measured: the **input decision** (sample → selection), the only
//! model-free realtime class, timed engine-level through the real
//! `SelectionEngine`. The model-dependent classes (warm-KV next-chars, suggest,
//! speak, turns) need the assembled hub + the real model and are run on FLU-REF
//! (`--contractual`); this harness lists them with their budgets rather than
//! pretending to measure them without a model.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use fluence_input::{DwellConfig, SelectionEngine};
use fluence_protocol::api::system::LatencyClass;
use fluence_protocol::input::{Rect, Target, TargetMap, TargetRole, Viewport};
use fluence_protocol::{Normalized, SurfaceId, TargetId};

/// Provisional budgets are the contractual ones ×2.5 (CI runners, PLAN §0.8).
const PROVISIONAL_MULTIPLIER: f64 = 2.5;
/// Measured samples (after warm-up).
const SAMPLES: usize = 20_000;
/// Warm-up samples (not measured).
const WARMUP: usize = 1_000;

/// Runs the latency gate for the given tier. Exit code is the gate verdict.
pub fn run(contractual: bool) -> ExitCode {
    let multiplier = if contractual {
        1.0
    } else {
        PROVISIONAL_MULTIPLIER
    };
    let tier = if contractual {
        "contractual"
    } else {
        "provisional"
    };
    println!("latency: tier={tier} (budget × {multiplier})\n");

    let (p50, p95) = measure_input_decision();
    let (b50, b95) = LatencyClass::InputDecision.budget_ms();
    let (g50, g95) = (b50 * multiplier, b95 * multiplier);
    let pass = p50 <= g50 && p95 <= g95;

    println!(
        "{:<22}{:>12}{:>12}{:>16}",
        "class", "p50 (ms)", "p95 (ms)", "budget p50/p95"
    );
    println!(
        "{:<22}{p50:>12.4}{p95:>12.4}{:>16}",
        "input_decision",
        format!("{g50:.1}/{g95:.1}"),
    );
    for class in LatencyClass::ALL.iter().filter(|c| c.requires_model()) {
        let (cb50, cb95) = class.budget_ms();
        println!(
            "{:<22}{:>12}{:>12}{:>16}",
            label(*class),
            "(FLU-REF)",
            "(FLU-REF)",
            format!("{:.0}/{:.0}", cb50 * multiplier, cb95 * multiplier),
        );
    }
    println!(
        "\nlatency: model-dependent classes need the assembled hub + real model — \
         run `cargo xtask latency --contractual` on FLU-REF to measure them."
    );

    if pass {
        println!("latency: input_decision PASS — {p50:.4}/{p95:.4} ms ≤ {g50:.1}/{g95:.1} ms");
        ExitCode::SUCCESS
    } else {
        eprintln!("latency: input_decision FAIL — {p50:.4}/{p95:.4} ms > {g50:.1}/{g95:.1} ms");
        ExitCode::FAILURE
    }
}

/// Stable label for a latency class (the wire name, not the Debug name).
fn label(class: LatencyClass) -> &'static str {
    match class {
        LatencyClass::NextChars => "next_chars",
        LatencyClass::SuggestFirstDelta => "suggest_first_delta",
        LatencyClass::SuggestComplete => "suggest_complete",
        LatencyClass::SpeakFirstAudio => "speak_first_audio",
        LatencyClass::Turns => "turns",
        LatencyClass::InputDecision => "input_decision",
    }
}

/// Measures per-sample `on_pointer` latency (ms) through the real engine over a
/// realistic keyboard sweep; returns `(p50, p95)`.
fn measure_input_decision() -> (f64, f64) {
    let map = keyboard();
    let mut engine = SelectionEngine::new(DwellConfig::default());
    let _ = engine.set_targets(&map);

    let mut clock_us = 0u64;
    let mut step = |engine: &mut SelectionEngine, i: usize| {
        let (x, y) = sweep(i);
        let now = Duration::from_micros(clock_us);
        clock_us += 16_000; // ~60 Hz between samples
        let start = Instant::now();
        let _ = engine.on_pointer(x, y, now);
        start.elapsed().as_secs_f64() * 1000.0
    };

    for i in 0..WARMUP {
        let _ = step(&mut engine, i);
    }
    let mut samples: Vec<f64> = (0..SAMPLES)
        .map(|i| step(&mut engine, WARMUP + i))
        .collect();
    percentiles(&mut samples)
}

/// A 10×4 keyboard grid in a 1000×600 viewport (≈ AZERTY density).
fn keyboard() -> TargetMap {
    let (cols, rows) = (10u32, 4u32);
    let (vw, vh) = (1000.0, 600.0);
    let (kw, kh) = (vw / f64::from(cols), vh / f64::from(rows));
    let mut targets = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            targets.push(Target {
                id: TargetId::from(format!("k{r}_{c}").as_str()),
                rect: Rect {
                    x: f64::from(c) * kw,
                    y: f64::from(r) * kh,
                    w: kw,
                    h: kh,
                },
                role: TargetRole::Key,
                label: None,
                prior: Normalized::new(0.5).ok(),
            });
        }
    }
    TargetMap {
        surface: SurfaceId::from("main"),
        viewport: Viewport { w: 1000, h: 600 },
        targets,
    }
}

/// A deterministic sweep over the surface (no RNG) that exercises hit-test,
/// focus changes, dwell and cancel by walking across keys and the gaps.
#[allow(clippy::cast_precision_loss)] // i < WARMUP+SAMPLES (< 2^21): exact in f64.
fn sweep(i: usize) -> (f64, f64) {
    let fi = i as f64;
    let x = 500.0 + 480.0 * (fi * 0.13).sin();
    let y = 300.0 + 280.0 * (fi * 0.07).cos();
    (x, y)
}

/// The `num/den` percentile of an already-sorted slice — integer index
/// arithmetic, no float casts.
fn pick(sorted: &[f64], num: usize, den: usize) -> f64 {
    sorted[(sorted.len() - 1) * num / den]
}

/// p50 and p95 (ms) of a sample set (sorts in place once).
fn percentiles(samples: &mut [f64]) -> (f64, f64) {
    samples.sort_by(f64::total_cmp);
    (pick(samples, 50, 100), pick(samples, 95, 100))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_selects_the_expected_index() {
        let data: Vec<f64> = (0..=100).map(f64::from).collect();
        assert!((pick(&data, 50, 100) - 50.0).abs() < 1e-9);
        assert!((pick(&data, 95, 100) - 95.0).abs() < 1e-9);
        assert!((pick(&data, 0, 100) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn input_decision_measures_finite_and_well_under_budget() {
        let (p50, p95) = measure_input_decision();
        assert!(
            p50.is_finite() && p95.is_finite() && p50 <= p95,
            "{p50}/{p95}"
        );
        // A pure hit-test + dwell step is microseconds; even provisional (×2.5)
        // leaves enormous margin. This validates the harness end to end without
        // a tight, machine-sensitive assertion.
        let (_, b95) = LatencyClass::InputDecision.budget_ms();
        assert!(
            p95 < b95 * PROVISIONAL_MULTIPLIER,
            "p95 {p95} ms within budget"
        );
    }

    #[test]
    fn keyboard_has_forty_keys() {
        assert_eq!(keyboard().targets.len(), 40);
    }
}
