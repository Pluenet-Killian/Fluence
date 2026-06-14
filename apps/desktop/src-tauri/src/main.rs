// SPDX-License-Identifier: AGPL-3.0-only

//! Fluence desktop app (Phase 7.1): embeds and supervises the hub, then shows
//! the composer the hub serves. Per D-2.1 the webview talks to the hub **only**
//! over the local network API (`tauri.conf.json` points the window at
//! `http://127.0.0.1:7411`) — one code path, the remote mode is free.

// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use fluence_watchdog::{Watchdog, WatchdogConfig};

fn main() {
    // Keep the hub alive for the whole app lifetime: autostart + restart < 2 s
    // if it ever dies (the composer reconnects; the draft is restored, D-2.6).
    // The guard drops when `run` returns, stopping the hub on exit.
    let _hub = spawn_hub();

    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running the Fluence desktop app");
}

/// Spawns the bundled hub sidecar under the watchdog if it can be located next
/// to the app executable (Tauri places `externalBin` there). Returns the guard;
/// `None` if the binary is absent (dev runs where the hub is started by hand).
fn spawn_hub() -> Option<Watchdog> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) {
        "fluence-hub.exe"
    } else {
        "fluence-hub"
    };
    let hub = exe.parent()?.join(name);
    hub.exists()
        .then(|| Watchdog::spawn(WatchdogConfig::new(hub)))
}
