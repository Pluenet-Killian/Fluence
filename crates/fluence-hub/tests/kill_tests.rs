// SPDX-License-Identifier: Apache-2.0

//! T4 kill-tests — the heart of Phase 2 (PLAN): the hub survives
//! everything, against the **real binaries**.
//!
//! - killed worker → `system.degraded` on the WS in < 500 ms, automatic
//!   restart with backoff, restart counter exposed;
//! - hub killed (-9) mid-typing → on restart the draft is restored with
//!   ≤ 1 s of loss, measured by keystroke timestamps;
//! - kill/restart worker cycles (soak proxy, `FLUENCE_SOAK_CYCLES`) → hub RSS
//!   bounded (< +10 % after warm-up);
//! - boot → ready < 3 s (provisional ×2.5 on CI runners — PLAN §0.8);
//! - hub logs never contain draft content (P0, end to end).

use std::fmt::Write as _;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Provisional multiplier on contractual budgets (PLAN §0.8).
const PROVISIONAL: f64 = 2.5;

/// Warm-up cycles before the RSS baseline in the soak proxy: the allocator and
/// OS caches reach steady state over the first few respawns, so a cold baseline
/// would inflate the growth ratio (a measurement artefact, not a leak).
const SOAK_WARMUP_CYCLES: u64 = 5;

/// Waits until the hub has written its port file and answers `/pair/info`,
/// returning the bound port. Panics past `timeout` (the test has failed).
fn wait_for_ready_port(data_dir: &std::path::Path, timeout: Duration) -> u16 {
    let port_file = data_dir.join("hub.port");
    let deadline = Instant::now() + timeout;
    loop {
        assert!(Instant::now() < deadline, "hub never became ready");
        if let Ok(raw) = std::fs::read_to_string(&port_file)
            && let Ok(port) = raw.trim().parse::<u16>()
            && ureq::get(format!("http://127.0.0.1:{port}/pair/info"))
                .call()
                .is_ok()
        {
            break port;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

struct HubProcess {
    child: Child,
    port: u16,
    system_token: String,
    data_dir: std::path::PathBuf,
}

impl HubProcess {
    /// Spawns the real hub binary on an ephemeral port and waits ready.
    /// Returns the boot duration alongside.
    fn spawn(data_dir: &std::path::Path, with_echo_worker: bool) -> (Self, Duration) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fluence-hub"));
        command
            .env("FLUENCE_DATA_DIR", data_dir)
            .env("FLUENCE_PORT", "0")
            .env("FLUENCE_STORE_KEY_FILE", data_dir.join("store.key"))
            .env("FLUENCE_PID_DIR", data_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if with_echo_worker {
            command.env("FLUENCE_ECHO_WORKER", env!("CARGO_BIN_EXE_worker-echo"));
        }
        let started = Instant::now();
        let child = command.spawn().expect("hub spawns");
        let port = wait_for_ready_port(data_dir, Duration::from_secs(15));
        let boot = started.elapsed();
        let system_token = std::fs::read_to_string(data_dir.join("system.token"))
            .expect("system token")
            .trim()
            .to_owned();
        (
            Self {
                child,
                port,
                system_token,
                data_dir: data_dir.to_owned(),
            },
            boot,
        )
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// Pairs a control device through the real flow.
    fn pair_control_token(&self) -> String {
        let agent = ureq::agent();
        let window: serde_json::Value = agent
            .post(self.url("/api/v1/pair/window"))
            .header("X-Fluence-Token", &self.system_token)
            .send_json(serde_json::json!({ "scope": "control" }))
            .expect("window")
            .body_mut()
            .read_json()
            .expect("json");
        let paired: serde_json::Value = agent
            .post(self.url("/pair"))
            .send_json(serde_json::json!({
                "code": window["code"], "device_name": "kill-test", "device_kind": "cli"
            }))
            .expect("pair")
            .body_mut()
            .read_json()
            .expect("json");
        paired["device_token"].as_str().expect("token").to_owned()
    }

    /// The hub's resident set size, in bytes.
    fn rss(&self) -> u64 {
        let mut system = sysinfo::System::new();
        let pid = sysinfo::Pid::from_u32(self.child.id());
        system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
        system.process(pid).map_or(0, sysinfo::Process::memory)
    }

    fn kill_dash_nine(&mut self) {
        self.child.kill().expect("kill -9 hub");
        let _ = self.child.wait();
    }
}

impl Drop for HubProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Finds the (single) echo worker pid recorded under the data dir.
fn read_worker_pid(data_dir: &std::path::Path) -> Option<u32> {
    let mut pids: Vec<u32> = std::fs::read_dir(data_dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("worker-echo-")
        })
        .filter_map(|entry| {
            std::fs::read_to_string(entry.path())
                .ok()?
                .trim()
                .parse()
                .ok()
        })
        .filter(|pid| process_alive(*pid))
        .collect();
    pids.sort_unstable();
    pids.last().copied()
}

fn process_alive(pid: u32) -> bool {
    let mut system = sysinfo::System::new();
    let pid = sysinfo::Pid::from_u32(pid);
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).is_some()
}

/// Kills an arbitrary process by pid (the worker).
fn kill_pid(pid: u32) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .output();
    }
    #[cfg(unix)]
    {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
    }
}

/// Opens the system-topic WebSocket and returns the connected socket
/// (hello frame already consumed and checked).
fn open_system_ws(
    hub: &HubProcess,
) -> tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>> {
    let url = format!(
        "ws://127.0.0.1:{}/ws?topics=system&v=1&token={}",
        hub.port, hub.system_token
    );
    let (mut socket, _) = tungstenite::connect(&url).expect("ws connects");
    let hello = socket.read().expect("hello frame");
    let text = hello.into_text().expect("text frame");
    assert!(
        text.contains("system.hello"),
        "first frame is hello: {text}"
    );
    socket
}

/// State and restart count of the first supervised worker, read from
/// `/system/health` — the **state of record** (the broadcast event bus is
/// a notification channel; an event fired before a subscriber connects is
/// gone, so readiness is polled here, not awaited on the WS).
fn worker_health(hub: &HubProcess) -> Option<(String, u64)> {
    let health: serde_json::Value = ureq::agent()
        .get(hub.url("/api/v1/system/health"))
        .header("X-Fluence-Token", &hub.system_token)
        .call()
        .ok()?
        .body_mut()
        .read_json()
        .ok()?;
    let worker = health["workers"].get(0)?;
    let state = worker["state"].as_str()?.to_owned();
    let restarts = worker["restart_count"].as_u64()?;
    Some((state, restarts))
}

/// Polls `/system/health` until the worker is `ready` with at least
/// `min_restarts` restarts. Panics past `timeout` (the test has failed).
fn wait_for_worker_ready(hub: &HubProcess, min_restarts: u64, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        assert!(
            Instant::now() < deadline,
            "worker not ready with >= {min_restarts} restarts in time"
        );
        if let Some((state, restarts)) = worker_health(hub)
            && state == "ready"
            && restarts >= min_restarts
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn boot_to_ready_under_three_seconds_provisional() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (hub, boot) = HubProcess::spawn(dir.path(), false);
    let budget = Duration::from_secs(3).mul_f64(PROVISIONAL);
    assert!(boot < budget, "boot took {boot:?} (budget {budget:?})");
    drop(hub);
}

#[test]
fn killed_worker_degrades_within_500ms_and_restarts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (hub, _boot) = HubProcess::spawn(dir.path(), true);

    // Settle: worker ready (polled via health, not the event bus).
    wait_for_worker_ready(&hub, 0, Duration::from_secs(10));
    let pid = read_worker_pid(&hub.data_dir).expect("worker pid recorded");

    // Subscribe BEFORE the kill so the `down` event cannot be missed; the
    // event-to-observation latency is the contract under test (PLAN T4).
    let mut socket = open_system_ws(&hub);
    let killed_at = Instant::now();
    kill_pid(pid);
    let budget = Duration::from_millis(500).mul_f64(PROVISIONAL);
    let down_latency = loop {
        assert!(
            Instant::now() < killed_at + Duration::from_secs(10),
            "no degraded event after kill"
        );
        let frame = socket.read().expect("frame").into_text().expect("text");
        if frame.contains(r#""state":"down""#) {
            break killed_at.elapsed();
        }
    };
    assert!(
        down_latency < budget,
        "degraded event took {down_latency:?} (budget {budget:?}, PLAN T4)"
    );

    // Supervision restarts it: health shows ready again with the counter
    // incremented.
    wait_for_worker_ready(&hub, 1, Duration::from_secs(10));
}

#[test]
fn hub_killed_mid_typing_loses_at_most_one_second() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut hub, _boot) = HubProcess::spawn(dir.path(), false);
    let control = hub.pair_control_token();

    let agent = ureq::agent();
    let session: serde_json::Value = agent
        .post(hub.url("/api/v1/sessions"))
        .header("X-Fluence-Token", &control)
        .send_empty()
        .expect("session")
        .body_mut()
        .read_json()
        .expect("json");
    let session_id = session["session_id"].as_str().expect("id").to_owned();

    // Simulated typing at 10 Hz for ~2.5 s; remember each acknowledged
    // keystroke with its wall-clock time.
    let mut acknowledged: Vec<(Instant, String)> = Vec::new();
    let mut text = String::new();
    for i in 0..25 {
        let _ = write!(text, "k{i} ");
        let caret = u32::try_from(text.chars().count()).expect("fits");
        let response = agent
            .put(hub.url(&format!("/api/v1/sessions/{session_id}/draft")))
            .header("X-Fluence-Token", &control)
            .send_json(serde_json::json!({ "text": text, "caret": caret }));
        if response.is_ok() {
            acknowledged.push((Instant::now(), text.clone()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let killed_at = Instant::now();
    hub.kill_dash_nine();

    // Restart on the same data dir; the store must reopen and the draft
    // must be at most 1 s older than the kill.
    let (hub2, _boot) = HubProcess::spawn(dir.path(), false);
    let control2 = hub2.pair_control_token();
    let draft: serde_json::Value = ureq::agent()
        .get(hub2.url(&format!("/api/v1/sessions/{session_id}/draft")))
        .header("X-Fluence-Token", &control2)
        .call()
        .expect("draft restored (was persisted)")
        .body_mut()
        .read_json()
        .expect("json");
    let restored = draft["text"].as_str().expect("text");

    let restored_at = acknowledged
        .iter()
        .find(|(_, text)| text == restored)
        .map(|(at, _)| *at)
        .expect("restored draft matches an acknowledged keystroke");
    let lost = killed_at.duration_since(restored_at);
    assert!(
        lost <= Duration::from_secs(1),
        "lost {lost:?} of typing (D-2.6 guarantees ≤ 1 s)"
    );
}

#[test]
fn flush_stays_bounded_under_a_flood_of_dirty_sessions() {
    // F20: a buggy or hostile *local* Control client opens a flood of
    // distinct sessions and PUTs a draft into each. The autosave flush
    // batches the whole tick into a single transaction (one fsync), so its
    // duration cannot grow with the session count and starve a legitimately
    // typed session of its ≤ 1 s loss bound (D-2.6). Before the fix the
    // flush fsynced N drafts one by one and could run for seconds; a kill -9
    // mid-flush then lost far more than 1 s of the real keystroke.
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut hub, _boot) = HubProcess::spawn(dir.path(), false);
    let control = hub.pair_control_token();
    let agent = ureq::agent();

    // Flood: many distinct dirty sessions buffered in one flush window.
    for i in 0..3_000u32 {
        let _ = agent
            .put(hub.url(&format!("/api/v1/sessions/flood-{i}/draft")))
            .header("X-Fluence-Token", &control)
            .send_json(serde_json::json!({ "text": format!("noise {i}"), "caret": 1 }));
    }

    // The legitimate session keeps typing through the flood.
    let session: serde_json::Value = agent
        .post(hub.url("/api/v1/sessions"))
        .header("X-Fluence-Token", &control)
        .send_empty()
        .expect("session")
        .body_mut()
        .read_json()
        .expect("json");
    let session_id = session["session_id"].as_str().expect("id").to_owned();

    let mut acknowledged: Vec<(Instant, String)> = Vec::new();
    let mut text = String::new();
    for i in 0..15 {
        let _ = write!(text, "k{i} ");
        let caret = u32::try_from(text.chars().count()).expect("fits");
        let response = agent
            .put(hub.url(&format!("/api/v1/sessions/{session_id}/draft")))
            .header("X-Fluence-Token", &control)
            .send_json(serde_json::json!({ "text": text, "caret": caret }));
        if response.is_ok() {
            acknowledged.push((Instant::now(), text.clone()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let killed_at = Instant::now();
    hub.kill_dash_nine();

    // Restart: the legitimate draft must be at most 1 s older than the kill,
    // even though thousands of other sessions were flushed in the same tick.
    let (hub2, _boot) = HubProcess::spawn(dir.path(), false);
    let control2 = hub2.pair_control_token();
    let draft: serde_json::Value = ureq::agent()
        .get(hub2.url(&format!("/api/v1/sessions/{session_id}/draft")))
        .header("X-Fluence-Token", &control2)
        .call()
        .expect("legitimate draft restored")
        .body_mut()
        .read_json()
        .expect("json");
    let restored = draft["text"].as_str().expect("text");

    let restored_at = acknowledged
        .iter()
        .find(|(_, text)| text == restored)
        .map(|(at, _)| *at)
        .expect("restored draft matches an acknowledged keystroke");
    let lost = killed_at.duration_since(restored_at);
    assert!(
        lost <= Duration::from_secs(1),
        "lost {lost:?} of typing under a {}-session flood (D-2.6 guarantees ≤ 1 s)",
        3_000
    );
}

#[test]
fn kill_cycles_keep_rss_bounded() {
    // A soak proxy (PLAN 7.6): many supervised-worker kill/restart cycles must
    // not leak — the hub RSS stays bounded. `FLUENCE_SOAK_CYCLES` scales the
    // measured count (default 50; the nightly extended soak sets it far higher);
    // the 72 h one-shot is a separate, physical run on FLU-REF (docs/ops/soak.md).
    let dir = tempfile::tempdir().expect("tempdir");
    let (hub, _boot) = HubProcess::spawn(dir.path(), true);
    wait_for_worker_ready(&hub, 0, Duration::from_secs(10));

    let measured: u64 = std::env::var("FLUENCE_SOAK_CYCLES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);

    // Warm up before the baseline (see SOAK_WARMUP_CYCLES) so the ratio reflects
    // a real leak, not cold-start growth.
    let mut cycle = 0u64;
    let run_cycles = |hub: &HubProcess, count: u64, cycle: &mut u64| {
        for _ in 0..count {
            let pid = read_worker_pid(&hub.data_dir)
                .unwrap_or_else(|| panic!("worker pid at cycle {cycle}"));
            kill_pid(pid);
            *cycle += 1;
            // Each kill increments the restart counter; wait for the respawn to
            // reach ready again (down → backoff → respawn → ready).
            wait_for_worker_ready(hub, *cycle, Duration::from_secs(20));
        }
    };

    run_cycles(&hub, SOAK_WARMUP_CYCLES, &mut cycle);
    let baseline = hub.rss();
    assert!(baseline > 0, "rss readable");
    run_cycles(&hub, measured, &mut cycle);
    let final_rss = hub.rss();

    // RSS in bytes is far below f64's 2^53 exact-integer ceiling, so this
    // ratio is exact in practice.
    #[allow(clippy::cast_precision_loss)]
    let ratio = final_rss as f64 / baseline as f64;
    assert!(
        ratio < 1.10,
        "RSS grew {:.1}% over {measured} post-warmup cycles (baseline {baseline}, final {final_rss})",
        (ratio - 1.0) * 100.0
    );
}

#[test]
fn hub_logs_never_contain_draft_content() {
    let dir = tempfile::tempdir().expect("tempdir");
    let secret_phrase = "p0-canary-never-logged";

    let (mut hub, _boot) = {
        // Spawn with debug logging to maximize the leak surface.
        let mut command = Command::new(env!("CARGO_BIN_EXE_fluence-hub"));
        command
            .env("FLUENCE_DATA_DIR", dir.path())
            .env("FLUENCE_PORT", "0")
            .env("FLUENCE_STORE_KEY_FILE", dir.path().join("store.key"))
            .env("RUST_LOG", "debug")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = command.spawn().expect("hub spawns");
        let port = wait_for_ready_port(dir.path(), Duration::from_secs(15));
        let system_token = std::fs::read_to_string(dir.path().join("system.token"))
            .expect("token")
            .trim()
            .to_owned();
        (
            HubProcess {
                child,
                port,
                system_token,
                data_dir: dir.path().to_owned(),
            },
            Duration::ZERO,
        )
    };

    let control = hub.pair_control_token();
    let agent = ureq::agent();
    let session: serde_json::Value = agent
        .post(hub.url("/api/v1/sessions"))
        .header("X-Fluence-Token", &control)
        .send_empty()
        .expect("session")
        .body_mut()
        .read_json()
        .expect("json");
    let session_id = session["session_id"].as_str().expect("id");
    agent
        .put(hub.url(&format!("/api/v1/sessions/{session_id}/draft")))
        .header("X-Fluence-Token", &control)
        .send_json(serde_json::json!({ "text": secret_phrase, "caret": 5 }))
        .expect("draft accepted");
    std::thread::sleep(Duration::from_millis(800)); // let the flusher run

    // Stop and read everything the process ever logged.
    hub.kill_dash_nine();
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut out) = hub.child.stdout.take() {
        let _ = out.read_to_string(&mut stdout);
    }
    if let Some(mut err) = hub.child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }
    assert!(
        !stdout.contains(secret_phrase) && !stderr.contains(secret_phrase),
        "P0 draft content leaked into hub logs (SPEC §9.A)"
    );
}
