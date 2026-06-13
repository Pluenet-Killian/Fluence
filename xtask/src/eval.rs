// SPDX-License-Identifier: Apache-2.0

//! `cargo xtask run-eval` — the offline evaluation harness (SPEC §8.A, PLAN 3.6).
//!
//! Builds the `fluence-ngram` binary the harness measures, then runs the suite
//! through the `ml` virtual environment. It invokes the venv's Python directly
//! rather than `uv run`, which both avoids a second resolve and sidesteps the
//! anti-cheat driver that blocks uv launchers on some Windows dev machines
//! (CONTRIBUTING); CI and local runs take the same path.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// Runs the evaluation suite, returning the harness's exit status.
///
/// Fails if the n-gram build fails or the `ml` virtual environment is missing
/// (run `uv sync --directory ml` first).
pub fn run(repo_root: &Path, suite: &str) -> ExitCode {
    eprintln!("run-eval: building fluence-ngram (the n-gram baseline)…");
    if !build_ngram(repo_root) {
        eprintln!("run-eval: failed to build fluence-ngram");
        return ExitCode::FAILURE;
    }

    let python = venv_python(repo_root);
    if !python.exists() {
        eprintln!(
            "run-eval: {} not found — run `uv sync --directory ml` first",
            python.display()
        );
        return ExitCode::FAILURE;
    }

    let ml_dir = repo_root.join("ml");
    let status = Command::new(&python)
        .args(["-m", "fluence_eval", "run", "--suite", suite])
        .current_dir(&ml_dir)
        .status();
    match status {
        Ok(code) if code.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("run-eval: cannot launch the harness: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Builds the `fluence-ngram` binary (reusing the active cargo).
fn build_ngram(repo_root: &Path) -> bool {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    Command::new(cargo)
        .args(["build", "-p", "fluence-ngram"])
        .current_dir(repo_root)
        .status()
        .is_ok_and(|code| code.success())
}

/// Path to the synced `ml` virtual environment's Python interpreter.
fn venv_python(repo_root: &Path) -> PathBuf {
    let venv = repo_root.join("ml").join(".venv");
    if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    }
}
