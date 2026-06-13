// SPDX-License-Identifier: Apache-2.0

//! Integration tests: the repository itself conforms to its own rules, and
//! the xtask CLI reports phase-gated commands the way CI wiring expects.

use std::path::Path;
use std::process::Command;

/// Every source file in this repository carries the SPDX header required by
/// D-10.1. Adding a file without a header turns this test red — by design.
#[test]
fn repository_conforms_to_license_layout() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the repository root");
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("check-licenses")
        .current_dir(repo_root)
        .output()
        .expect("xtask binary runs");
    assert!(
        output.status.success(),
        "check-licenses failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The test-asset manifest is well-formed and the command is wired: `--check`
/// validates and reports without touching the network, so CI and contributors
/// can trust the manifest before a (large) download. A malformed manifest —
/// bad sha256, non-https URL, traversal-prone filename — turns this red.
#[test]
fn download_test_assets_check_validates_the_manifest() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the repository root");
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["download-test-assets", "--check"])
        .current_dir(repo_root)
        .output()
        .expect("xtask binary runs");
    assert!(
        output.status.success(),
        "download-test-assets --check failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("manifest v1 OK") && stdout.contains("tiny-llm"),
        "check output should confirm the manifest and list the model, got: {stdout}"
    );
}

/// No arguments and unknown commands print usage and fail with code 1.
#[test]
fn unknown_command_prints_usage_and_fails() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .output()
        .expect("xtask binary runs");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage: cargo xtask"));

    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("frobnicate")
        .output()
        .expect("xtask binary runs");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown command"));
}
