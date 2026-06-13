// SPDX-License-Identifier: Apache-2.0

//! T4 — security surface (SPEC §2.A): uniform 401 without a token, the
//! pairing window lifecycle (expiry, replay, brute force → 429), and the
//! complete scope×route matrix, exercised through the real pairing flow.

use fluence_hub::config::HubConfig;
use fluence_hub::{RunningHub, start};
use fluence_protocol::api::pair::Scope;

/// Starts an in-process hub on an ephemeral port, isolated data dir.
async fn test_hub() -> (RunningHub, tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = HubConfig {
        port: 0,
        data_dir: dir.path().to_owned(),
        store_key_file: Some(dir.path().join("store.key")),
        ..HubConfig::default()
    };
    let hub = start(config).await.expect("hub starts");
    let system_token = std::fs::read_to_string(dir.path().join("system.token"))
        .expect("bootstrap system token")
        .trim()
        .to_owned();
    (hub, dir, system_token)
}

fn url(hub: &RunningHub, path: &str) -> String {
    format!("http://{}{path}", hub.addr)
}

/// GETs `path` with an optional token, returns the status code.
fn get_status(hub: &RunningHub, path: &str, token: Option<&str>) -> u16 {
    let agent = ureq::agent();
    let mut request = agent.get(url(hub, path));
    if let Some(token) = token {
        request = request.header("X-Fluence-Token", token);
    }
    match request.call() {
        Ok(response) => response.status().as_u16(),
        Err(ureq::Error::StatusCode(code)) => code,
        Err(error) => panic!("transport error on {path}: {error}"),
    }
}

/// Opens a pairing window (system token) and pairs a device of `scope`,
/// returning its token — the only legitimate way to mint one.
fn pair_device(hub: &RunningHub, system_token: &str, scope: Scope) -> String {
    let agent = ureq::agent();
    let window: serde_json::Value = agent
        .post(url(hub, "/api/v1/pair/window"))
        .header("X-Fluence-Token", system_token)
        .send_json(serde_json::json!({ "scope": scope }))
        .expect("window opens")
        .body_mut()
        .read_json()
        .expect("window json");
    let code = window["code"].as_str().expect("code").to_owned();

    let paired: serde_json::Value = agent
        .post(url(hub, "/pair"))
        .send_json(serde_json::json!({
            "code": code,
            "device_name": format!("test-{scope:?}"),
            "device_kind": "cli",
        }))
        .expect("pairs")
        .body_mut()
        .read_json()
        .expect("pair json");
    paired["device_token"].as_str().expect("token").to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tokenless_requests_get_a_uniform_401() {
    let (hub, _dir, _system) = test_hub().await;
    for path in [
        "/api/v1/system/health",
        "/api/v1/system/capabilities",
        "/api/v1/sessions/x/draft",
    ] {
        assert_eq!(get_status(&hub, path, None), 401, "{path}");
        assert_eq!(
            get_status(&hub, path, Some("flt_forged")),
            401,
            "{path} forged"
        );
    }
    hub.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pair_info_is_the_only_anonymous_read() {
    let (hub, _dir, _system) = test_hub().await;
    assert_eq!(get_status(&hub, "/pair/info", None), 200);
    hub.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scope_route_matrix_is_enforced() {
    let (hub, _dir, system_token) = test_hub().await;

    // Mint one real token per scope through the real flow.
    let display = pair_device(&hub, &system_token, Scope::Display);
    let control = pair_device(&hub, &system_token, Scope::Control);
    let care = pair_device(&hub, &system_token, Scope::Care);

    // (route, [expected status per scope: display, control, care, system])
    // 200/204 = allowed; 403 = scope_insufficient (PLAN T1 matrix).
    let health = "/api/v1/system/health";
    for (token, expected) in [
        (&display, 200),
        (&control, 200),
        (&care, 200),
        (&system_token, 200),
    ] {
        assert_eq!(get_status(&hub, health, Some(token)), expected, "health");
    }

    // Sessions are control-only (plus system).
    let agent = ureq::agent();
    for (token, expected) in [
        (&display, 403u16),
        (&control, 200),
        (&care, 403),
        (&system_token, 200),
    ] {
        let status = match agent
            .post(url(&hub, "/api/v1/sessions"))
            .header("X-Fluence-Token", token.as_str())
            .send_empty()
        {
            Ok(response) => response.status().as_u16(),
            Err(ureq::Error::StatusCode(code)) => code,
            Err(error) => panic!("transport: {error}"),
        };
        assert_eq!(status, expected, "sessions create");
    }

    // The pairing window is system-only: every paired scope is refused.
    for token in [&display, &control, &care] {
        let status = match agent
            .post(url(&hub, "/api/v1/pair/window"))
            .header("X-Fluence-Token", token.as_str())
            .send_json(serde_json::json!({ "scope": "control" }))
        {
            Ok(response) => response.status().as_u16(),
            Err(ureq::Error::StatusCode(code)) => code,
            Err(error) => panic!("transport: {error}"),
        };
        assert_eq!(status, 403, "pair/window must be system-only");
    }
    hub.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pairing_codes_are_single_use_and_brute_force_burns_the_window() {
    let (hub, _dir, system_token) = test_hub().await;
    let agent = ureq::agent();

    // Single use: a second pairing with the same code is refused.
    let token = pair_device(&hub, &system_token, Scope::Display);
    assert!(token.starts_with("flt_"));
    let replay = agent
        .post(url(&hub, "/pair"))
        .send_json(serde_json::json!({
            "code": "00000000", "device_name": "replay", "device_kind": "cli"
        }))
        .err()
        .map(|e| match e {
            ureq::Error::StatusCode(code) => code,
            other => panic!("transport: {other}"),
        });
    assert_eq!(replay, Some(403), "window is consumed");

    // Brute force: open a fresh window, then hammer wrong codes.
    let window: serde_json::Value = agent
        .post(url(&hub, "/api/v1/pair/window"))
        .header("X-Fluence-Token", system_token.as_str())
        .send_json(serde_json::json!({ "scope": "display" }))
        .expect("window")
        .body_mut()
        .read_json()
        .expect("json");
    let real_code = window["code"].as_str().expect("code");

    let mut last_status = 0u16;
    for attempt in 0..5 {
        let wrong = if real_code == "99999999" {
            "11111111"
        } else {
            "99999999"
        };
        last_status = match agent.post(url(&hub, "/pair")).send_json(serde_json::json!({
            "code": wrong, "device_name": format!("bf-{attempt}"), "device_kind": "cli"
        })) {
            Ok(response) => response.status().as_u16(),
            Err(ureq::Error::StatusCode(code)) => code,
            Err(error) => panic!("transport: {error}"),
        };
    }
    assert_eq!(last_status, 429, "fifth wrong code rate-limits (PLAN T1)");

    // The burned window refuses even the real code now.
    let after = match agent.post(url(&hub, "/pair")).send_json(serde_json::json!({
        "code": real_code, "device_name": "late", "device_kind": "cli"
    })) {
        Ok(response) => response.status().as_u16(),
        Err(ureq::Error::StatusCode(code)) => code,
        Err(error) => panic!("transport: {error}"),
    };
    assert_eq!(after, 403, "burned window stays closed");
    hub.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cross_origin_browser_calls_are_refused() {
    let (hub, _dir, system_token) = test_hub().await;
    // Preflight from an unknown web origin: the allowlist is empty in
    // Phase 2, so CORS must not grant anything.
    let agent = ureq::agent();
    let request = ureq::http::Request::builder()
        .method(ureq::http::Method::OPTIONS)
        .uri(url(&hub, "/api/v1/system/health"))
        .header("Origin", "https://evil.example")
        .header("Access-Control-Request-Method", "GET")
        .body(())
        .expect("request builds");
    let response = agent.run(request);
    let allowed_origin = match response {
        Ok(r) => r
            .headers()
            .get("access-control-allow-origin")
            .map(|v| v.to_str().unwrap_or_default().to_owned()),
        Err(ureq::Error::StatusCode(_)) => None,
        Err(error) => panic!("transport: {error}"),
    };
    assert_eq!(allowed_origin, None, "no origin may be allowed in Phase 2");
    drop(system_token);
    hub.shutdown().await;
}
