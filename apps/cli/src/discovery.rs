// SPDX-License-Identifier: AGPL-3.0-only

//! Locating the local hub and a usable token.
//!
//! The hub writes `hub.port` and `system.token` into its data directory at
//! startup. `fluencectl` reads them to talk to an embedded-mode hub with no
//! configuration — the same trust boundary as the data dir itself (local
//! files). Overrides (`--url`, `--token`, `--data-dir`) always win, which is
//! how a second machine talks to a remote hub.

use std::path::{Path, PathBuf};

/// Resolved connection details.
pub struct Connection {
    /// Base URL of the hub (`http://127.0.0.1:7411`).
    pub url: String,
    /// Bearer token, when one is available (absent for `pair`).
    pub token: Option<String>,
}

/// The default data directory (matches the hub's `default_data_dir`).
#[must_use]
pub fn default_data_dir() -> PathBuf {
    directories::ProjectDirs::from("org", "fluence", "fluence").map_or_else(
        || PathBuf::from(".fluence"),
        |dirs| dirs.data_dir().to_owned(),
    )
}

/// Resolves the hub URL: explicit override, else `127.0.0.1:<hub.port>`
/// read from the data dir.
///
/// # Errors
///
/// Returns a message when no override is given and the port file is
/// unreadable (the hub is probably not running).
pub fn resolve_url(explicit: Option<&str>, data_dir: &Path) -> Result<String, String> {
    if let Some(url) = explicit {
        return Ok(url.trim_end_matches('/').to_owned());
    }
    let port_file = data_dir.join("hub.port");
    let raw = std::fs::read_to_string(&port_file).map_err(|e| {
        format!(
            "cannot read {} ({e}); is the hub running? (try --url)",
            port_file.display()
        )
    })?;
    let port: u16 = raw
        .trim()
        .parse()
        .map_err(|e| format!("invalid port in {}: {e}", port_file.display()))?;
    Ok(format!("http://127.0.0.1:{port}"))
}

/// Resolves a token: explicit override, else the saved CLI token, else the
/// local system token. Returns `None` if none is found (the caller decides
/// whether that is fatal).
#[must_use]
pub fn resolve_token(explicit: Option<&str>, data_dir: &Path) -> Option<String> {
    if let Some(token) = explicit {
        return Some(token.to_owned());
    }
    read_trimmed(&cli_token_path(data_dir)).or_else(|| read_trimmed(&data_dir.join("system.token")))
}

/// Path where `fluencectl pair` saves the token it obtained.
#[must_use]
pub fn cli_token_path(data_dir: &Path) -> PathBuf {
    data_dir.join("cli-token")
}

fn read_trimmed(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}
