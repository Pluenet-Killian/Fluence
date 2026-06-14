// SPDX-License-Identifier: Apache-2.0

//! `cargo xtask gaze-accuracy` — the T4 gaze-accuracy gate (PLAN §1, Phase 6
//! "Done quand").
//!
//! Replays deterministic synthetic gaze sessions through the real selection
//! pipeline and prints the accuracy of each, gating on a floor so a regression
//! turns CI red. The datasets are synthetic (a pipeline-correctness and
//! regression gate, not a real-precision claim — real precision needs real
//! capture via `fluencectl record-gaze`, SPEC §6 pivot clause). Run nightly so
//! the number is published over time.

use std::process::ExitCode;

use fluence_input::{GazeConfig, evaluate, synthetic_grid};

/// Floor below which a synthetic replay is treated as a regression. The
/// synthetic data is recoverable by construction, so a healthy pipeline scores
/// far above this; the floor only catches a genuine break.
const ACCURACY_FLOOR: f64 = 0.95;

/// Runs the gaze-accuracy replays and returns success unless one regresses.
#[must_use]
pub fn run() -> ExitCode {
    // (cols, rows, jitter): a clean grid and two moderately jittered grids.
    let cases = [(4, 3, 0.0), (5, 4, 0.05), (3, 3, 0.08)];

    println!("gaze-accuracy (synthetic, T4):");
    let mut regressed = false;
    for (cols, rows, jitter) in cases {
        let session = synthetic_grid(cols, rows, jitter);
        let report = evaluate(&session, GazeConfig::default());
        let accuracy = report.accuracy();
        let status = if accuracy + 1e-9 >= ACCURACY_FLOOR {
            "ok"
        } else {
            regressed = true;
            "REGRESSED"
        };
        println!(
            "  {name:<22} {correct:>3}/{total:<3} = {pct:>5.1}%  [{status}]",
            name = session.name,
            correct = report.correct,
            total = report.total,
            pct = accuracy * 100.0,
        );
    }

    if regressed {
        eprintln!("\ngaze-accuracy: a synthetic replay fell below the {ACCURACY_FLOOR:.2} floor.");
        ExitCode::FAILURE
    } else {
        println!("\ngaze-accuracy: all synthetic replays at or above the floor.");
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_passes_on_the_current_pipeline() {
        // The xtask gate itself is covered: the synthetic replays clear the floor.
        assert_eq!(run(), ExitCode::SUCCESS);
    }
}
