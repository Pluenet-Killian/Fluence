// SPDX-License-Identifier: AGPL-3.0-only

//! `fluencectl` — command-line client for the Fluence hub (PLAN 2.6).
//!
//! v0 surface: `health`, `pair-window`, `pair`, `watch`, `journal`, `suggest`.
//! It is a client of the public hub API like any other (D-2.1): no privileged
//! access, only the local token files written by the hub.

mod discovery;

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use fluence_protocol::api::pair::{PairResponse, Scope};
use fluence_protocol::api::suggest::{SuggestFinal, SuggestionOrigin};
use fluence_protocol::api::system::{AccessJournalResponse, CapabilitiesResponse, HealthResponse};

use discovery::Connection;

/// Fluence hub command-line tool.
#[derive(Parser)]
#[command(name = "fluencectl", version, about)]
struct Cli {
    /// Hub data directory (where `hub.port` / `system.token` live).
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    /// Hub base URL (overrides discovery from `hub.port`).
    #[arg(long, global = true)]
    url: Option<String>,
    /// Bearer token (overrides the saved CLI / system token).
    #[arg(long, global = true)]
    token: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show worker states and rolling latencies.
    Health,
    /// Open a pairing window and print the code to read to a device.
    PairWindow {
        /// Scope the paired device will receive.
        #[arg(long, value_enum, default_value = "control")]
        scope: ScopeArg,
    },
    /// Exchange a pairing code for a device token (saved locally).
    Pair {
        /// Eight-digit code shown by `pair-window`.
        #[arg(long)]
        code: String,
        /// Human name for this device.
        #[arg(long, default_value = "fluencectl")]
        name: String,
    },
    /// Stream system/input events from the hub.
    Watch {
        /// Comma-separated topics to subscribe to.
        #[arg(long, default_value = "system")]
        topics: String,
    },
    /// Print the local access journal (caregiver view).
    Journal {
        /// Maximum entries (newest first).
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Revoke a paired device's token (caregiver; find ids in `journal`).
    Revoke {
        /// Device id to revoke.
        device: String,
    },
    /// Rephrase or continue a draft through the acceleration engine (§5.A).
    Suggest {
        /// The draft text to act on (P0 — stays on the loopback to the hub).
        draft: String,
        /// Which acceleration function to run.
        #[arg(long, value_enum, default_value = "rephrase")]
        mode: ModeArg,
        /// How many suggestions to request (UI guidance: ≤ 3, SPEC §7.A).
        #[arg(long, default_value_t = 3)]
        n: u8,
    },
}

/// CLI mirror of [`Scope`] (clap value enum).
#[derive(Clone, Copy, clap::ValueEnum)]
enum ScopeArg {
    Display,
    Control,
    Care,
    System,
}

impl From<ScopeArg> for Scope {
    fn from(arg: ScopeArg) -> Self {
        match arg {
            ScopeArg::Display => Scope::Display,
            ScopeArg::Control => Scope::Control,
            ScopeArg::Care => Scope::Care,
            ScopeArg::System => Scope::System,
        }
    }
}

/// CLI mirror of the implemented [`SuggestMode`](fluence_protocol::api::suggest::SuggestMode)s
/// (v0: rephrase, continue — replies/expand are P2).
#[derive(Clone, Copy, clap::ValueEnum)]
enum ModeArg {
    Rephrase,
    Continue,
}

impl ModeArg {
    /// The wire value the hub expects (`SuggestMode`, lowercase).
    fn wire(self) -> &'static str {
        match self {
            ModeArg::Rephrase => "rephrase",
            ModeArg::Continue => "continue",
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let data_dir = cli
        .data_dir
        .clone()
        .unwrap_or_else(discovery::default_data_dir);

    let result = match &cli.command {
        Command::Health => run_health(&cli, &data_dir),
        Command::PairWindow { scope } => run_pair_window(&cli, &data_dir, (*scope).into()),
        Command::Pair { code, name } => run_pair(&cli, &data_dir, code, name),
        Command::Watch { topics } => run_watch(&cli, &data_dir, topics),
        Command::Journal { limit } => run_journal(&cli, &data_dir, *limit),
        Command::Revoke { device } => run_revoke(&cli, &data_dir, device),
        Command::Suggest { draft, mode, n } => run_suggest(&cli, &data_dir, draft, *mode, *n),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("fluencectl: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Resolves URL + token, requiring a token (every command but `pair`).
fn connect(cli: &Cli, data_dir: &std::path::Path) -> Result<Connection, String> {
    let url = discovery::resolve_url(cli.url.as_deref(), data_dir)?;
    let token = discovery::resolve_token(cli.token.as_deref(), data_dir)
        .ok_or("no token found (run `pair`, or pass --token)")?;
    Ok(Connection {
        url,
        token: Some(token),
    })
}

/// GET a JSON resource with the bearer token.
fn get_json<T: serde::de::DeserializeOwned>(conn: &Connection, path: &str) -> Result<T, String> {
    let token = conn.token.as_deref().unwrap_or_default();
    ureq::get(format!("{}{path}", conn.url))
        .header("X-Fluence-Token", token)
        .call()
        .map_err(|e| format!("request failed: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("invalid response: {e}"))
}

fn run_health(cli: &Cli, data_dir: &std::path::Path) -> Result<(), String> {
    let conn = connect(cli, data_dir)?;
    let health: HealthResponse = get_json(&conn, "/api/v1/system/health")?;
    let caps: CapabilitiesResponse = get_json(&conn, "/api/v1/system/capabilities")?;
    println!(
        "hub {} — tier {:?}, up since {}",
        health.version, caps.tier, health.started_at
    );
    if health.workers.is_empty() {
        println!("  workers: (none)");
    }
    for worker in &health.workers {
        println!(
            "  worker {:?}: {:?} (restarts: {})",
            worker.worker, worker.state, worker.restart_count
        );
    }
    for latency in &health.latencies {
        println!(
            "  latency {:?}: p50 {} ms / p95 {} ms",
            latency.class, latency.p50_ms, latency.p95_ms
        );
    }
    Ok(())
}

fn run_pair_window(cli: &Cli, data_dir: &std::path::Path, scope: Scope) -> Result<(), String> {
    let conn = connect(cli, data_dir)?;
    let token = conn.token.as_deref().unwrap_or_default();
    let response: serde_json::Value = ureq::post(format!("{}/api/v1/pair/window", conn.url))
        .header("X-Fluence-Token", token)
        .send_json(serde_json::json!({ "scope": scope }))
        .map_err(|e| format!("could not open window: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("invalid response: {e}"))?;
    let code = response["code"].as_str().ok_or("response missing code")?;
    println!("pairing window open for scope {scope:?}");
    println!("  code: {code}");
    println!(
        "  expires: {}",
        response["expires_at"].as_str().unwrap_or("?")
    );
    println!("on the other device: fluencectl pair --url <this hub> --code {code}");
    Ok(())
}

fn run_pair(cli: &Cli, data_dir: &std::path::Path, code: &str, name: &str) -> Result<(), String> {
    // `pair` needs the URL but no prior token (it is how a device gets one).
    let url = discovery::resolve_url(cli.url.as_deref(), data_dir)?;
    let paired: PairResponse = ureq::post(format!("{url}/pair"))
        .send_json(serde_json::json!({
            "code": code, "device_name": name, "device_kind": "cli"
        }))
        .map_err(|e| format!("pairing failed: {e}"))?
        .body_mut()
        .read_json()
        .map_err(|e| format!("invalid response: {e}"))?;
    let token_path = discovery::cli_token_path(data_dir);
    if let Some(parent) = token_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::write(&token_path, &paired.device_token)
        .map_err(|e| format!("cannot save token to {}: {e}", token_path.display()))?;
    restrict_permissions(&token_path);
    println!(
        "paired with scope {:?}; token saved to {}",
        paired.scope,
        token_path.display()
    );
    Ok(())
}

/// Restricts a saved token file to the owner (0600) on Unix; on Windows
/// the user profile ACL is the boundary.
fn restrict_permissions(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn run_watch(cli: &Cli, data_dir: &std::path::Path, topics: &str) -> Result<(), String> {
    let conn = connect(cli, data_dir)?;
    let token = conn.token.as_deref().unwrap_or_default();
    let ws_url = conn.url.replacen("http", "ws", 1);
    let url = format!(
        "{ws_url}/ws?topics={}&v={}&token={token}",
        topics,
        fluence_protocol::INPUT_PROTOCOL_VERSION
    );
    let (mut socket, _) =
        tungstenite::connect(&url).map_err(|e| format!("ws connect failed: {e}"))?;
    println!("watching topics [{topics}] — Ctrl-C to stop");
    loop {
        match socket.read() {
            Ok(tungstenite::Message::Text(text)) => println!("{text}"),
            Ok(tungstenite::Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            Err(e) => return Err(format!("ws error: {e}")),
        }
    }
}

fn run_journal(cli: &Cli, data_dir: &std::path::Path, limit: u32) -> Result<(), String> {
    let conn = connect(cli, data_dir)?;
    let journal: AccessJournalResponse =
        get_json(&conn, &format!("/api/v1/system/journal?limit={limit}"))?;
    if journal.entries.is_empty() {
        println!("(access journal is empty)");
    }
    for entry in &journal.entries {
        let device = entry.device_id.as_deref().unwrap_or("-");
        let detail = entry.detail.as_deref().unwrap_or("");
        println!("{}  {:24}  {device}  {detail}", entry.at, entry.action);
    }
    Ok(())
}

fn run_revoke(cli: &Cli, data_dir: &std::path::Path, device: &str) -> Result<(), String> {
    let conn = connect(cli, data_dir)?;
    let token = conn.token.as_deref().unwrap_or_default();
    ureq::delete(format!("{}/api/v1/devices/{device}", conn.url))
        .header("X-Fluence-Token", token)
        .call()
        .map_err(|e| format!("revoke failed: {e}"))?;
    println!("device {device} revoked");
    Ok(())
}

fn run_suggest(
    cli: &Cli,
    data_dir: &std::path::Path,
    draft: &str,
    mode: ModeArg,
    n: u8,
) -> Result<(), String> {
    let conn = connect(cli, data_dir)?;
    let token = conn.token.as_deref().unwrap_or_default();
    let body = serde_json::json!({ "mode": mode.wire(), "draft": draft, "n": n, "slot": "main" });
    // The hub answers with an SSE stream (deltas then a final list). For a
    // one-shot CLI we read it whole, then print the post-processed final.
    let mut raw = String::new();
    ureq::post(format!("{}/api/v1/sessions/cli/suggest", conn.url))
        .header("X-Fluence-Token", token)
        .send_json(body)
        .map_err(|e| format!("suggest failed: {e}"))?
        .into_body()
        .into_reader()
        .read_to_string(&mut raw)
        .map_err(|e| format!("reading suggestions failed: {e}"))?;

    let final_data = final_event_data(&raw).ok_or("no final event in the stream")?;
    let parsed: SuggestFinal =
        serde_json::from_str(&final_data).map_err(|e| format!("invalid final event: {e}"))?;
    if parsed.suggestions.is_empty() {
        println!("(no suggestion — the model is unavailable and the fallback had nothing)");
        return Ok(());
    }
    for (index, suggestion) in parsed.suggestions.iter().enumerate() {
        let origin = match suggestion.origin {
            Some(SuggestionOrigin::Model) => "model",
            Some(SuggestionOrigin::Ngram) => "n-gram",
            Some(SuggestionOrigin::Memory) => "memory",
            _ => "?",
        };
        println!("{}. {}  [{origin}]", index + 1, suggestion.text);
    }
    Ok(())
}

/// Extracts the `data:` payload of the SSE `final` frame, if present.
fn final_event_data(body: &str) -> Option<String> {
    body.split("\n\n").find_map(|frame| {
        let is_final = frame
            .lines()
            .any(|line| line.strip_prefix("event:").map(str::trim) == Some("final"));
        if !is_final {
            return None;
        }
        frame
            .lines()
            .find_map(|line| line.strip_prefix("data:"))
            .map(|data| data.trim().to_owned())
    })
}
