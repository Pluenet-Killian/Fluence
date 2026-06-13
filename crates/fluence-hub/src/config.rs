// SPDX-License-Identifier: Apache-2.0

//! Hub configuration (PLAN 2.1): defaults → TOML file → environment.
//!
//! Precedence is strict and testable: every field has a built-in default,
//! the optional TOML file overrides it, and a documented `FLUENCE_*`
//! environment variable overrides both (the finite list lives in
//! [`HubConfig::apply_env`] — no magic prefix scanning).

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Default API port (SPEC §2.A).
pub const DEFAULT_PORT: u16 = 7411;

/// Complete hub configuration.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct HubConfig {
    /// Listen address. Loopback by default (embedded mode); home mode
    /// (LAN + TLS) is explicitly opt-in (SPEC §2.A, D-2.4).
    pub listen_addr: IpAddr,
    /// Listen port. When taken, the hub falls back to an ephemeral port
    /// and logs it (SPEC §2.A « repli dynamique si occupé »).
    pub port: u16,
    /// Data directory (store, keys). Default: the OS project dir
    /// (`ProjectDirs("org", "fluence", "fluence")`).
    pub data_dir: PathBuf,
    /// Store the master key in a file instead of the OS keystore —
    /// tests and headless installs (SPEC D-9.1 keeps the keystore as the
    /// production default).
    pub store_key_file: Option<PathBuf>,
    /// Household name shown on pairing screens and announced over mDNS.
    pub household_name: String,
    /// Command to launch the echo worker (test harness). Real worker
    /// commands join this table in Phase 4+.
    pub echo_worker_command: Option<PathBuf>,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            listen_addr: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: DEFAULT_PORT,
            data_dir: default_data_dir(),
            store_key_file: None,
            household_name: "Fluence".to_owned(),
            echo_worker_command: None,
        }
    }
}

/// Configuration loading errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The TOML file exists but cannot be read.
    #[error("cannot read config file {path}: {source}")]
    Unreadable {
        /// Offending path.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// The TOML file does not parse or contains unknown fields.
    #[error("invalid config file {path}: {source}")]
    Invalid {
        /// Offending path.
        path: PathBuf,
        /// Underlying error.
        source: toml::de::Error,
    },
    /// An environment override does not parse.
    #[error("invalid environment override {name}={value}: {reason}")]
    InvalidEnv {
        /// Variable name.
        name: &'static str,
        /// Rejected value.
        value: String,
        /// Why it was rejected.
        reason: String,
    },
}

impl HubConfig {
    /// Loads configuration: defaults, then `path` (if it exists), then
    /// `FLUENCE_*` environment overrides.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] when the file exists but is unreadable/invalid, or
    /// when an environment override does not parse. A *missing* file is
    /// not an error: defaults apply.
    pub fn load(path: Option<&Path>) -> Result<Self, ConfigError> {
        let mut config = match path {
            Some(path) if path.exists() => {
                let raw =
                    std::fs::read_to_string(path).map_err(|source| ConfigError::Unreadable {
                        path: path.to_owned(),
                        source,
                    })?;
                toml::from_str(&raw).map_err(|source| ConfigError::Invalid {
                    path: path.to_owned(),
                    source,
                })?
            }
            _ => Self::default(),
        };
        config.apply_env(|name| std::env::var(name).ok())?;
        Ok(config)
    }

    /// Applies the documented `FLUENCE_*` overrides. `lookup` is injected
    /// so tests never mutate the process environment.
    ///
    /// # Errors
    ///
    /// [`ConfigError::InvalidEnv`] when a present variable does not parse.
    pub fn apply_env(
        &mut self,
        lookup: impl Fn(&str) -> Option<String>,
    ) -> Result<(), ConfigError> {
        if let Some(value) = lookup("FLUENCE_LISTEN_ADDR") {
            self.listen_addr = value.parse().map_err(|e| ConfigError::InvalidEnv {
                name: "FLUENCE_LISTEN_ADDR",
                value,
                reason: format!("{e}"),
            })?;
        }
        if let Some(value) = lookup("FLUENCE_PORT") {
            self.port = value.parse().map_err(|e| ConfigError::InvalidEnv {
                name: "FLUENCE_PORT",
                value,
                reason: format!("{e}"),
            })?;
        }
        if let Some(value) = lookup("FLUENCE_DATA_DIR") {
            self.data_dir = PathBuf::from(value);
        }
        if let Some(value) = lookup("FLUENCE_STORE_KEY_FILE") {
            self.store_key_file = Some(PathBuf::from(value));
        }
        if let Some(value) = lookup("FLUENCE_HOUSEHOLD_NAME") {
            self.household_name = value;
        }
        if let Some(value) = lookup("FLUENCE_ECHO_WORKER") {
            self.echo_worker_command = Some(PathBuf::from(value));
        }
        Ok(())
    }
}

/// OS-conventional data directory, with a sane fallback when the OS
/// provides none (rare: stripped-down containers).
fn default_data_dir() -> PathBuf {
    directories::ProjectDirs::from("org", "fluence", "fluence").map_or_else(
        || PathBuf::from(".fluence"),
        |dirs| dirs.data_dir().to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_loopback_7411() {
        let config = HubConfig::default();
        assert_eq!(config.listen_addr, IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(config.port, DEFAULT_PORT);
    }

    #[test]
    fn env_overrides_file_which_overrides_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "port = 9000\nhousehold_name = \"Chez Claire\"\n").expect("write");

        let mut config = HubConfig::load(Some(&path)).expect("load");
        assert_eq!(config.port, 9000); // file beat default
        config
            .apply_env(|name| (name == "FLUENCE_PORT").then(|| "9100".to_owned()))
            .expect("env");
        assert_eq!(config.port, 9100); // env beat file
        assert_eq!(config.household_name, "Chez Claire"); // untouched by env
    }

    #[test]
    fn unknown_fields_in_file_are_rejected() {
        // A typo in a config file must be loud, not silently ignored —
        // an assistive device misconfigured by accident is a real harm.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "prot = 9000\n").expect("write");
        assert!(matches!(
            HubConfig::load(Some(&path)),
            Err(ConfigError::Invalid { .. })
        ));
    }

    #[test]
    fn invalid_env_value_is_a_clean_error() {
        let mut config = HubConfig::default();
        let error = config
            .apply_env(|name| (name == "FLUENCE_PORT").then(|| "not-a-port".to_owned()))
            .expect_err("must reject");
        assert!(error.to_string().contains("FLUENCE_PORT"));
    }

    #[test]
    fn missing_file_means_defaults() {
        let config = HubConfig::load(Some(Path::new("does/not/exist.toml"))).expect("load");
        assert_eq!(config, HubConfig::default());
    }
}
