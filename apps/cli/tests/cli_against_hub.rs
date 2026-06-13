// SPDX-License-Identifier: AGPL-3.0-only

//! T5-ish — `fluencectl` drives a real hub: the Phase 2 "Done quand"
//! scripted-demo path (health, pairing window, pair, journal), exercised
//! mechanically. The hub runs in-process (its library); the CLI runs as
//! the real built binary.

use std::process::Command;

use fluence_hub::config::HubConfig;
use fluence_hub::{RunningHub, start};

/// Runs `fluencectl` with the given args, returning (success, stdout).
fn fluencectl(args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_fluencectl"))
        .args(args)
        .output()
        .expect("fluencectl binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status.success(), format!("{stdout}{stderr}"))
}

async fn test_hub(dir: &std::path::Path) -> RunningHub {
    let config = HubConfig {
        port: 0,
        data_dir: dir.to_owned(),
        store_key_file: Some(dir.join("store.key")),
        ..HubConfig::default()
    };
    start(config).await.expect("hub starts")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_reports_the_running_hub() {
    let dir = tempfile::tempdir().expect("tempdir");
    let hub = test_hub(dir.path()).await;

    let (ok, out) = fluencectl(&["--data-dir", &dir.path().to_string_lossy(), "health"]);
    assert!(ok, "health failed: {out}");
    assert!(out.contains("hub"), "unexpected health output: {out}");

    hub.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pairing_window_pair_and_journal_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let hub = test_hub(dir.path()).await;
    let data_dir = dir.path().to_string_lossy().into_owned();

    // Open a window with the local system token, read the 8-digit code.
    let (ok, out) = fluencectl(&["--data-dir", &data_dir, "pair-window", "--scope", "control"]);
    assert!(ok, "pair-window failed: {out}");
    let code = out
        .lines()
        .find_map(|l| l.trim().strip_prefix("code: "))
        .expect("a code line")
        .to_owned();
    assert_eq!(code.len(), 8, "8-digit code, got {code:?}");

    // A second "device" (separate data dir) exchanges the code for a token.
    let device_dir = tempfile::tempdir().expect("device dir");
    let device_data = device_dir.path().to_string_lossy().into_owned();
    let url = format!("http://127.0.0.1:{}", hub.addr.port());
    let (ok, out) = fluencectl(&[
        "--data-dir",
        &device_data,
        "--url",
        &url,
        "pair",
        "--code",
        &code,
        "--name",
        "demo-device",
    ]);
    assert!(ok, "pair failed: {out}");
    assert!(device_dir.path().join("cli-token").exists(), "token saved");

    // The journal (caregiver view) shows the pairing actions, no P0.
    let (ok, out) = fluencectl(&["--data-dir", &data_dir, "journal", "--limit", "20"]);
    assert!(ok, "journal failed: {out}");
    assert!(
        out.contains("pair.window_opened"),
        "window open journaled: {out}"
    );
    assert!(out.contains("device.paired"), "pairing journaled: {out}");

    hub.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pair_rejects_a_wrong_code() {
    let dir = tempfile::tempdir().expect("tempdir");
    let hub = test_hub(dir.path()).await;
    let url = format!("http://127.0.0.1:{}", hub.addr.port());
    let device_dir = tempfile::tempdir().expect("device dir");

    // No window open → any code is refused; the CLI exits non-zero.
    let (ok, out) = fluencectl(&[
        "--data-dir",
        &device_dir.path().to_string_lossy(),
        "--url",
        &url,
        "pair",
        "--code",
        "00000000",
    ]);
    assert!(!ok, "pairing with no open window must fail: {out}");
    assert!(
        !device_dir.path().join("cli-token").exists(),
        "no token on failure"
    );

    hub.shutdown().await;
}
