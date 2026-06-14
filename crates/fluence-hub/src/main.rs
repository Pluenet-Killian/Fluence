// SPDX-License-Identifier: Apache-2.0

//! Hub binary. Default: load config, init telemetry, serve until Ctrl-C.
//!
//! Offline data-maintenance subcommands (hub stopped, PLAN 7.3) reuse the same
//! config so they find the same store and key:
//!
//! - `fluence-hub backup --out <archive>` — encrypted backup + recovery kit.
//! - `fluence-hub restore --in <archive> --recovery "<phrase>"` — restore it.
//! - `fluence-hub wipe --yes` — erase all personal content (SPEC §9.A).
//!
//! `--config <path>` selects the config for any of these (else defaults + env).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fluence_hub::config::HubConfig;
use fluence_hub::maintenance;

#[tokio::main]
async fn main() -> ExitCode {
    fluence_hub::telemetry::init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = match HubConfig::load(config_path(&args).as_deref()) {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(%error, "invalid configuration");
            return ExitCode::FAILURE;
        }
    };

    match subcommand(&args).as_deref() {
        None | Some("serve") => serve(config).await,
        Some("backup") => {
            let Some(out) = flag_value(&args, "--out") else {
                eprintln!("backup: --out <archive> is required");
                return ExitCode::FAILURE;
            };
            maintenance::backup(&config, Path::new(&out))
        }
        Some("restore") => {
            let (Some(input), Some(phrase)) =
                (flag_value(&args, "--in"), flag_value(&args, "--recovery"))
            else {
                eprintln!("restore: --in <archive> and --recovery \"<phrase>\" are required");
                return ExitCode::FAILURE;
            };
            maintenance::restore_cmd(&config, Path::new(&input), &phrase)
        }
        Some("wipe") => maintenance::wipe(&config, args.iter().any(|a| a == "--yes")).await,
        Some(other) => {
            eprintln!("fluence-hub: unknown subcommand `{other}`");
            print_usage();
            ExitCode::FAILURE
        }
    }
}

/// Runs the server until Ctrl-C, then shuts down gracefully.
async fn serve(config: HubConfig) -> ExitCode {
    let hub = match fluence_hub::start(config).await {
        Ok(hub) => hub,
        Err(error) => {
            tracing::error!(%error, "hub failed to start");
            return ExitCode::FAILURE;
        }
    };

    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "signal handler failed");
    }
    tracing::info!("shutting down");
    hub.shutdown().await;
    ExitCode::SUCCESS
}

/// The first positional argument (the subcommand), skipping `--config <value>`
/// and any other leading flag. Returns `None` when only flags are present.
fn subcommand(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--config" => i += 2, // skip the flag and its value
            flag if flag.starts_with('-') => i += 1,
            positional => return Some(positional.to_owned()),
        }
    }
    None
}

/// The value following a `--flag`, if present.
fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

/// The `--config` path, if given.
fn config_path(args: &[String]) -> Option<PathBuf> {
    flag_value(args, "--config").map(PathBuf::from)
}

fn print_usage() {
    eprintln!("Usage: fluence-hub [--config <path>] [<subcommand>]\n");
    eprintln!("Without a subcommand, runs the hub server.\n");
    eprintln!("Subcommands (run with the hub stopped):");
    eprintln!("  backup  --out <archive>                 encrypted backup + recovery kit");
    eprintln!("  restore --in <archive> --recovery <ph>  restore a backup (moves the old aside)");
    eprintln!("  wipe    --yes                           erase all personal content (SPEC §9.A)");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn no_args_means_serve() {
        assert_eq!(subcommand(&argv(&[])), None);
    }

    #[test]
    fn config_flag_and_value_are_skipped_before_the_subcommand() {
        assert_eq!(
            subcommand(&argv(&["--config", "hub.toml", "backup", "--out", "a"])),
            Some("backup".to_owned())
        );
        // A subcommand-owned flag value is never mistaken for the subcommand.
        assert_eq!(
            subcommand(&argv(&["backup", "--out", "archive"])),
            Some("backup".to_owned())
        );
    }

    #[test]
    fn config_path_and_flag_values_are_extracted() {
        // A quoted phrase reaches us as one argv element (spaces preserved).
        let args = argv(&[
            "--config",
            "c.toml",
            "restore",
            "--in",
            "a.bak",
            "--recovery",
            "AB12 CD34",
        ]);
        assert_eq!(config_path(&args), Some(PathBuf::from("c.toml")));
        assert_eq!(flag_value(&args, "--in"), Some("a.bak".to_owned()));
        assert_eq!(
            flag_value(&args, "--recovery"),
            Some("AB12 CD34".to_owned())
        );
        assert_eq!(flag_value(&args, "--missing"), None);
    }
}
