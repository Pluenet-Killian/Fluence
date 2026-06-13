// SPDX-License-Identifier: Apache-2.0

//! T4 — the hub supervises `llama-server` (a fake stand-in here, so no model
//! download): `/suggest` streams **model**-origin suggestions while it is
//! healthy, and the moment it dies the endpoint **degrades to the n-gram
//! fallback** — 200, never a 5xx — « le clavier parle toujours » (D-2.6).
//!
//! The fake server is the `fake-llama-server` test binary; the hub spawns and
//! supervises it through the real production path (`build_llama_engine` +
//! `supervise_llama_server`), so this exercises spawn → `/health` → ready →
//! crash → degrade end to end against the real hub binary.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const TOKEN_HEADER: &str = "X-Fluence-Token";

/// Waits until the hub has written its port file and answers `/pair/info`.
fn wait_for_ready_port(data_dir: &Path, timeout: Duration) -> u16 {
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

struct Hub {
    child: Child,
    port: u16,
    system_token: String,
}

impl Drop for Hub {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Hub {
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// State of the (single) supervised LLM worker, read from `/system/health`.
    fn llm_state(&self) -> Option<String> {
        let health: serde_json::Value = ureq::agent()
            .get(self.url("/api/v1/system/health"))
            .header(TOKEN_HEADER, &self.system_token)
            .call()
            .ok()?
            .body_mut()
            .read_json()
            .ok()?;
        health["workers"]
            .as_array()?
            .iter()
            .find(|w| w["worker"] == "llm")?["state"]
            .as_str()
            .map(ToOwned::to_owned)
    }

    fn pair_control_token(&self) -> String {
        let agent = ureq::agent();
        let window: serde_json::Value = agent
            .post(self.url("/api/v1/pair/window"))
            .header(TOKEN_HEADER, &self.system_token)
            .send_json(serde_json::json!({ "scope": "control" }))
            .expect("window")
            .body_mut()
            .read_json()
            .expect("json");
        let paired: serde_json::Value = agent
            .post(self.url("/pair"))
            .send_json(serde_json::json!({
                "code": window["code"], "device_name": "llama-test", "device_kind": "cli"
            }))
            .expect("pair")
            .body_mut()
            .read_json()
            .expect("json");
        paired["device_token"].as_str().expect("token").to_owned()
    }
}

/// Spawns the real hub binary configured to supervise `fake-llama-server`.
fn spawn_hub(data_dir: &Path, pidfile: &Path, die_marker: &Path) -> Hub {
    let fake = env!("CARGO_BIN_EXE_fake-llama-server");
    let child = Command::new(env!("CARGO_BIN_EXE_fluence-hub"))
        .env("FLUENCE_DATA_DIR", data_dir)
        .env("FLUENCE_PORT", "0")
        .env("FLUENCE_STORE_KEY_FILE", data_dir.join("store.key"))
        .env("FLUENCE_PID_DIR", data_dir)
        .env("FLUENCE_LLAMA_SERVER_BIN", fake)
        // The fake ignores -m; any existing path satisfies the "model" arg.
        .env("FLUENCE_LLAMA_MODEL", fake)
        .env("FLUENCE_FAKE_LLAMA_PIDFILE", pidfile)
        .env("FLUENCE_FAKE_LLAMA_DIE_IF_EXISTS", die_marker)
        .env("FLUENCE_FAKE_LLAMA_WEDGE_IF_EXISTS", data_dir.join("wedge"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("hub spawns");
    let port = wait_for_ready_port(data_dir, Duration::from_secs(15));
    let system_token = std::fs::read_to_string(data_dir.join("system.token"))
        .expect("system token")
        .trim()
        .to_owned();
    Hub {
        child,
        port,
        system_token,
    }
}

/// POSTs a rephrase request and returns `(status, parsed SSE (event, data))`.
fn suggest(hub: &Hub, control: &str, draft: &str) -> (u16, Vec<(String, String)>) {
    let body = serde_json::json!({ "mode": "rephrase", "draft": draft, "n": 3, "slot": "main" });
    let response = ureq::agent()
        .post(hub.url("/api/v1/sessions/s-llama/suggest"))
        .header(TOKEN_HEADER, control)
        .send_json(body)
        .expect("suggest never 5xx (D-2.6)");
    let status = response.status().as_u16();
    let mut raw = String::new();
    response
        .into_body()
        .into_reader()
        .read_to_string(&mut raw)
        .expect("read SSE body");
    (status, parse_sse(&raw))
}

/// Parses an SSE body into `(event, data)` pairs.
fn parse_sse(body: &str) -> Vec<(String, String)> {
    let field = |frame: &str, name: &str| {
        frame
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .unwrap_or("")
            .trim()
            .to_owned()
    };
    body.split("\n\n")
        .filter(|frame| !frame.trim().is_empty())
        .map(|frame| (field(frame, "event:"), field(frame, "data:")))
        .collect()
}

/// The `final` event's payload, parsed.
fn final_payload(events: &[(String, String)]) -> serde_json::Value {
    let (_, data) = events
        .iter()
        .find(|(event, _)| event == "final")
        .expect("a final event");
    serde_json::from_str(data).expect("SuggestFinal json")
}

fn read_pid(pidfile: &Path, timeout: Duration) -> u32 {
    let deadline = Instant::now() + timeout;
    loop {
        assert!(Instant::now() < deadline, "fake-llama never wrote its pid");
        if let Ok(raw) = std::fs::read_to_string(pidfile)
            && let Ok(pid) = raw.trim().parse::<u32>()
        {
            return pid;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

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

/// Polls `/system/health` until the LLM worker reaches `ready`.
fn wait_for_llm_ready(hub: &Hub, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        assert!(
            Instant::now() < deadline,
            "the LLM worker never became ready"
        );
        if hub.llm_state().as_deref() == Some("ready") {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn suggest_uses_the_model_then_degrades_when_llama_dies() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pidfile = dir.path().join("fake.pid");
    let die_marker = dir.path().join("die");
    let hub = spawn_hub(dir.path(), &pidfile, &die_marker);

    // The supervisor spawns the fake, polls /health, and marks it ready.
    wait_for_llm_ready(&hub, Duration::from_secs(15));
    let control = hub.pair_control_token();

    // Healthy: /suggest streams the model's rephrase to a final event.
    let (status, events) = suggest(&hub, &control, "veu eau frache");
    assert_eq!(status, 200);
    assert!(events.iter().any(|(e, _)| e == "delta"), "expected deltas");
    let parsed = final_payload(&events);
    let suggestions = parsed["suggestions"].as_array().expect("suggestions");
    assert!(
        suggestions.iter().any(|s| s["origin"] == "model"),
        "a healthy llama-server must produce model-origin suggestions, got: {parsed}"
    );

    // Make any respawn fail, then kill the running fake: the engine stays down.
    std::fs::write(&die_marker, b"die").expect("write die marker");
    kill_pid(read_pid(&pidfile, Duration::from_secs(5)));

    // Degradation: /suggest stays 200 (never a 5xx) and stops returning
    // model-origin suggestions — it has fallen back (D-2.6). Poll until the
    // supervisor has observed the death.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (status, events) = suggest(&hub, &control, "veu eau frache");
        assert_eq!(status, 200, "degradation must never be a 5xx (D-2.6)");
        let parsed = final_payload(&events);
        let has_model = parsed["suggestions"]
            .as_array()
            .is_some_and(|s| s.iter().any(|s| s["origin"] == "model"));
        if !has_model {
            break; // degraded: no model-origin suggestion any more
        }
        assert!(
            Instant::now() < deadline,
            "the engine never degraded after llama-server died"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_wedged_but_alive_llama_is_detected_and_degrades() {
    // Unlike the crash test above, the fake server stays ALIVE here — we never
    // kill it — but stops answering /health. Only the supervisor's liveness
    // probe can notice this (a dead-process watch never fires for a live
    // process), restart it, and so degrade /suggest to the n-gram fallback
    // (D-2.6 « le clavier parle toujours »).
    let dir = tempfile::tempdir().expect("tempdir");
    let pidfile = dir.path().join("fake.pid");
    let die_marker = dir.path().join("die");
    let wedge_marker = dir.path().join("wedge");
    let hub = spawn_hub(dir.path(), &pidfile, &die_marker);

    wait_for_llm_ready(&hub, Duration::from_secs(15));
    let control = hub.pair_control_token();

    // Healthy first: a model-origin suggestion proves the engine is live.
    let (status, events) = suggest(&hub, &control, "veu eau frache");
    assert_eq!(status, 200);
    assert!(
        final_payload(&events)["suggestions"]
            .as_array()
            .is_some_and(|s| s.iter().any(|s| s["origin"] == "model")),
        "a healthy llama-server must produce model-origin suggestions"
    );

    // Wedge /health while the process stays alive. Also make any respawn exit,
    // so once the probe restarts the worker the engine stays down — proving the
    // probe, not a crash, caught it.
    std::fs::write(&die_marker, b"die").expect("write die marker");
    std::fs::write(&wedge_marker, b"wedge").expect("write wedge marker");

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let (status, events) = suggest(&hub, &control, "veu eau frache");
        assert_eq!(status, 200, "degradation must never be a 5xx (D-2.6)");
        let degraded = !final_payload(&events)["suggestions"]
            .as_array()
            .is_some_and(|s| s.iter().any(|s| s["origin"] == "model"));
        if degraded {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the liveness probe never detected the wedged (but alive) server"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}
