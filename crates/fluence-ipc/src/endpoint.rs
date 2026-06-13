// SPDX-License-Identifier: Apache-2.0

//! IPC endpoint naming — one portable representation, per-OS realization.

use std::fmt;
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

/// Where an IPC channel lives: a named-pipe path on Windows
/// (`\\.\pipe\fluence-…`), a socket path on Unix (`…/fluence-….sock`).
///
/// The inner string is the exact platform path; it travels to workers as a
/// plain CLI argument (`worker-echo --ipc <endpoint>`), so the worker side
/// needs no naming logic at all.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IpcEndpoint(String);

/// Distinguishes concurrently created endpoints within one process.
static ENDPOINT_COUNTER: AtomicU64 = AtomicU64::new(0);

impl IpcEndpoint {
    /// Creates a fresh, collision-free endpoint for `name`
    /// (`[a-z0-9-]`, enforced) — unique across processes (pid) and within
    /// the process (counter). This is what the supervisor uses per worker.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains characters outside `[a-z0-9-]` — endpoint
    /// names are developer-provided constants, never user input.
    #[must_use]
    pub fn unique(name: &str) -> Self {
        assert!(
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "endpoint name must match [a-z0-9-]+, got {name:?}"
        );
        let discriminant = format!(
            "{name}-{}-{}",
            process::id(),
            ENDPOINT_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        Self(Self::platform_path(&discriminant))
    }

    /// Wraps an exact platform path (the worker side, from `--ipc`).
    #[must_use]
    pub fn from_path(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    /// The platform path this endpoint designates.
    #[must_use]
    pub fn as_path(&self) -> &str {
        &self.0
    }

    #[cfg(windows)]
    fn platform_path(discriminant: &str) -> String {
        format!(r"\\.\pipe\fluence-{discriminant}")
    }

    #[cfg(unix)]
    fn platform_path(discriminant: &str) -> String {
        // Unix socket paths are length-limited (~104 bytes): temp_dir keeps
        // them short. The hub may later place them under its runtime dir
        // via `from_path`.
        std::env::temp_dir()
            .join(format!("fluence-{discriminant}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}

impl fmt::Display for IpcEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_endpoints_never_collide() {
        let a = IpcEndpoint::unique("echo");
        let b = IpcEndpoint::unique("echo");
        assert_ne!(a, b);
    }

    #[test]
    fn display_round_trips_through_from_path() {
        let original = IpcEndpoint::unique("worker-llm");
        let parsed = IpcEndpoint::from_path(original.to_string());
        assert_eq!(original, parsed);
    }

    #[test]
    #[should_panic(expected = "endpoint name must match")]
    fn invalid_names_are_rejected() {
        let _ = IpcEndpoint::unique("Bad Name!");
    }
}
