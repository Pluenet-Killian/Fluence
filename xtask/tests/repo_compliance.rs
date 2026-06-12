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

/// Phase-gated commands exit with the dedicated code 2 and say which phase
/// delivers them, so a CI job calling them too early fails loudly and
/// explicitly rather than silently passing.
#[test]
fn phase_gated_commands_fail_explicitly() {
    for (command, phase) in [
        ("check-contracts", "Phase 1"),
        ("download-test-assets", "Phase 3"),
        ("run-eval", "Phase 3"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg(command)
            .output()
            .expect("xtask binary runs");
        assert_eq!(output.status.code(), Some(2), "{command} must exit with 2");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(phase),
            "{command} must mention {phase}, got: {stderr}"
        );
    }
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
