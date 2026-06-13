// SPDX-License-Identifier: Apache-2.0

//! Repository tasks for Fluence (`cargo xtask <command>`).
//!
//! The xtask pattern keeps repo automation in plain, tested Rust instead of
//! shell scripts: same toolchain on Windows and Linux, no extra installs
//! (PLAN Phase 0, task 0.1).

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod contracts;
mod licenses;

/// Exit code for commands that exist but are not implemented in the current
/// phase — distinct from `1` (check failed) so CI wiring mistakes are loud.
const EXIT_NOT_YET_AVAILABLE: u8 = 2;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let command = args.next();
    match command.as_deref() {
        Some("check-licenses") => check_licenses(),
        Some("check-contracts") => {
            let check = match args.next().as_deref() {
                Some("--check") => true,
                None => false,
                Some(other) => {
                    eprintln!("xtask check-contracts: unknown flag `{other}` (only --check)");
                    return ExitCode::FAILURE;
                }
            };
            contracts::run(&repo_root(), check)
        }
        Some("download-test-assets") => not_yet(
            "download-test-assets",
            "Phase 3",
            "no test assets are referenced yet",
        ),
        Some("run-eval") => not_yet(
            "run-eval",
            "Phase 3",
            "the evaluation harness does not exist yet",
        ),
        Some(other) => {
            eprintln!("xtask: unknown command `{other}`\n");
            print_usage();
            ExitCode::FAILURE
        }
        None => {
            print_usage();
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!("Usage: cargo xtask <command>\n");
    eprintln!("Commands:");
    eprintln!("  check-licenses          verify SPDX headers against the D-10.1 layout");
    eprintln!("  check-contracts [--check]");
    eprintln!("                          regenerate contract artifacts (schemas/, OpenAPI,");
    eprintln!("                          SDK types); --check compares without writing (CI)");
    eprintln!("  download-test-assets    (Phase 3) fetch pinned test models and fixtures");
    eprintln!("  run-eval                (Phase 3) run the evaluation harness");
}

/// Reports a command scheduled for a later PLAN phase and exits with
/// [`EXIT_NOT_YET_AVAILABLE`].
fn not_yet(command: &str, phase: &str, reason: &str) -> ExitCode {
    eprintln!("xtask {command}: not available yet — arrives in PLAN {phase} ({reason}).");
    ExitCode::from(EXIT_NOT_YET_AVAILABLE)
}

/// The repository root (the parent of the `xtask` crate, wherever cargo was
/// invoked from).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask always lives one level below the repository root")
        .to_path_buf()
}

/// Runs the SPDX header check from the repository root.
fn check_licenses() -> ExitCode {
    let repo_root = repo_root();
    match licenses::check(&repo_root) {
        Ok(checked) => {
            println!("check-licenses: {checked} source files conform to the D-10.1 layout.");
            ExitCode::SUCCESS
        }
        Err(violations) => {
            for violation in &violations {
                eprintln!("{violation}");
            }
            eprintln!("\ncheck-licenses: {} violation(s).", violations.len());
            ExitCode::FAILURE
        }
    }
}
