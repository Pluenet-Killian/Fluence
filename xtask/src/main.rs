// SPDX-License-Identifier: Apache-2.0

//! Repository tasks for Fluence (`cargo xtask <command>`).
//!
//! The xtask pattern keeps repo automation in plain, tested Rust instead of
//! shell scripts: same toolchain on Windows and Linux, no extra installs
//! (PLAN Phase 0, task 0.1).

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod assets;
mod contracts;
mod eval;
mod licenses;

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
        Some("download-test-assets") => {
            let check = match args.next().as_deref() {
                Some("--check") => true,
                None => false,
                Some(other) => {
                    eprintln!("xtask download-test-assets: unknown flag `{other}` (only --check)");
                    return ExitCode::FAILURE;
                }
            };
            assets::run(&repo_root(), check)
        }
        Some("run-eval") => {
            let mut suite = String::from("pr");
            loop {
                match args.next().as_deref() {
                    Some("--suite") => {
                        let Some(value) = args.next() else {
                            eprintln!("xtask run-eval: --suite needs a value");
                            return ExitCode::FAILURE;
                        };
                        suite = value;
                    }
                    Some(other) => {
                        eprintln!("xtask run-eval: unknown flag `{other}` (only --suite <name>)");
                        return ExitCode::FAILURE;
                    }
                    None => break,
                }
            }
            eval::run(&repo_root(), &suite)
        }
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
    eprintln!("  download-test-assets [--check]");
    eprintln!("                          fetch the pinned test models (sha256-verified,");
    eprintln!("                          resumable); --check validates the manifest only");
    eprintln!("  run-eval [--suite <name>]");
    eprintln!("                          build the n-gram and run the offline eval suite");
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
