// SPDX-License-Identifier: Apache-2.0

//! Hub binary: load config, init telemetry, run until Ctrl-C.

use std::path::PathBuf;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    fluence_hub::telemetry::init();

    let config_path = std::env::args()
        .skip_while(|arg| arg != "--config")
        .nth(1)
        .map(PathBuf::from);
    let config = match fluence_hub::config::HubConfig::load(config_path.as_deref()) {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(%error, "invalid configuration");
            return ExitCode::FAILURE;
        }
    };

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
