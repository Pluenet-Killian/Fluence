// SPDX-License-Identifier: Apache-2.0

//! Fluence hub: HTTP/WS API, worker supervision, selection engine
//! (SPEC §2.B, §2.C; ADR-0001, ADR-0005).
//!
//! The hub is the always-alive core of the platform. Cardinal rule:
//! *composing and speaking NEVER depend on AI component health* —
//! inference workers run as supervised child processes; every failure
//! degrades explicitly.
//!
//! Phase 2 surface: bootstrap (< 3 s to ready), encrypted store, pairing
//! and scoped tokens, draft autosave (≤ 1 s loss bound), supervisor with
//! the echo test worker, `/system/*`, WebSocket events. The selection
//! engine (Phase 5) and inference workers (Phase 4+) plug in here.

pub mod api;
pub mod auth;
pub mod config;
pub mod events;
pub mod state;
pub mod supervisor;
pub mod telemetry;

use std::net::SocketAddr;

use fluence_protocol::api::system::WorkerKind;
use fluence_store::{KeySource, Store, StoreConfig};
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::config::HubConfig;
use crate::events::EventBus;
use crate::state::AppState;
use crate::supervisor::WorkerSpec;

/// Hub startup errors.
#[derive(Debug, thiserror::Error)]
pub enum HubError {
    /// The store refused to open (key, corruption, IO).
    #[error("store: {0}")]
    Store(#[from] fluence_store::StoreError),
    /// Neither the configured port nor an ephemeral one could be bound.
    #[error("cannot bind {addr}: {source}")]
    Bind {
        /// Address that failed.
        addr: SocketAddr,
        /// Underlying error.
        source: std::io::Error,
    },
}

/// A running hub: real address, graceful-stop handle, join handle.
pub struct RunningHub {
    /// The address actually bound (port may differ from the configured
    /// one — fallback, or `0` requested by tests).
    pub addr: SocketAddr,
    stop: watch::Sender<bool>,
    served: tokio::task::JoinHandle<()>,
    state: AppState,
}

impl RunningHub {
    /// Requests a graceful stop and waits for it: drafts flushed, store
    /// closed, workers shut down.
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        let _ = self.served.await;
        for worker in self.state.workers() {
            worker.request_shutdown();
        }
        self.state.flush_drafts().await;
        // Last handle on the store: close politely (best effort — the
        // store also survives kill -9 by WAL design).
        if let Err(error) = self.state.store().clone().close().await {
            tracing::debug!(%error, "store close during shutdown");
        }
    }
}

/// Starts the hub: store, workers, listener, background tasks.
///
/// Ready means *accepting requests with the keyboard path alive*
/// (SPEC §2.C: < 3 s) — nothing here waits for any AI component.
///
/// # Errors
///
/// [`HubError`] when the store cannot open or no port can be bound.
///
/// # Panics
///
/// Panics if the freshly bound listener has no local address — an OS
/// invariant violation that cannot occur in practice.
pub async fn start(config: HubConfig) -> Result<RunningHub, HubError> {
    let store = Store::open(StoreConfig {
        path: config.data_dir.join("store.db"),
        key: store_key_source(&config),
    })
    .await?;

    let bus = EventBus::new();
    let state = AppState::new(config, store, bus);
    state.spawn_draft_flusher();

    // Bootstrap system token: the embedded UI and the local CLI read it
    // from the data dir (same trust boundary as the store key — local
    // file). Created once; pairing covers every other device (§2.A).
    ensure_system_token(&state).await?;

    if let Some(command) = state.config().echo_worker_command.clone() {
        let handle = supervisor::supervise(
            WorkerSpec {
                kind: WorkerKind::Unknown,
                command,
            },
            state.bus().clone(),
        );
        state.register_worker(handle);
    }

    let listener = bind_with_fallback(state.config()).await?;
    let addr = listener
        .local_addr()
        .expect("bound listener has an address");
    tracing::info!(%addr, "hub listening");
    // Local discovery for tools (fluencectl) and tests: the actual port.
    let port_file = state.config().data_dir.join("hub.port");
    if let Err(error) = std::fs::write(&port_file, addr.port().to_string()) {
        tracing::warn!(%error, "cannot write hub.port (local discovery degraded)");
    }

    let (stop_tx, mut stop_rx) = watch::channel(false);
    let router = api::build_router(state.clone());
    let served = tokio::spawn(async move {
        let shutdown = async move {
            let _ = stop_rx.changed().await;
        };
        if let Err(error) = axum::serve(listener, router)
            .with_graceful_shutdown(shutdown)
            .await
        {
            tracing::error!(%error, "server error");
        }
    });

    Ok(RunningHub {
        addr,
        stop: stop_tx,
        served,
        state,
    })
}

/// Chooses where the store master key lives. An explicit `store_key_file`
/// always wins; otherwise Windows uses the OS keystore (DPAPI) and other
/// platforms use a 0600 file in the data dir (ADR-0005; the headless Linux
/// hub has no desktop keystore).
fn store_key_source(config: &HubConfig) -> KeySource {
    if let Some(path) = &config.store_key_file {
        return KeySource::File(path.clone());
    }
    if cfg!(windows) {
        KeySource::Keyring {
            service: "fluence".to_owned(),
            entry: "store-key".to_owned(),
        }
    } else {
        KeySource::File(config.data_dir.join("store.key"))
    }
}

/// Creates (first run) or verifies the local system token and writes it
/// to `data_dir/system.token`. The file inherits the data dir's
/// protection, exactly like the store key file.
async fn ensure_system_token(state: &AppState) -> Result<(), HubError> {
    use fluence_protocol::api::pair::{DeviceKind, Scope};

    let path = state.config().data_dir.join("system.token");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        let known = state
            .store()
            .device_by_token_hash(auth::token_hash(existing.trim()))
            .await?
            .is_some();
        if known {
            return Ok(());
        }
        // Stale file (store reset): fall through and mint a fresh one.
    }
    let token = auth::generate_token();
    state
        .store()
        .insert_device(fluence_store::NewDevice {
            device_id: uuid::Uuid::new_v4().to_string(),
            token_hash: auth::token_hash(&token),
            name: "Embedded UI / local CLI".to_owned(),
            kind: DeviceKind::Desktop,
            scope: Scope::System,
        })
        .await?;
    std::fs::create_dir_all(&state.config().data_dir).ok();
    std::fs::write(&path, &token).map_err(|source| HubError::Bind {
        addr: SocketAddr::new(state.config().listen_addr, 0),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Binds the configured port, falling back to an ephemeral one when taken
/// (SPEC §2.A « repli dynamique si occupé »).
async fn bind_with_fallback(config: &HubConfig) -> Result<TcpListener, HubError> {
    let preferred = SocketAddr::new(config.listen_addr, config.port);
    match TcpListener::bind(preferred).await {
        Ok(listener) => Ok(listener),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse && config.port != 0 => {
            tracing::warn!(%preferred, "port taken; falling back to an ephemeral port");
            let fallback = SocketAddr::new(config.listen_addr, 0);
            TcpListener::bind(fallback)
                .await
                .map_err(|source| HubError::Bind {
                    addr: fallback,
                    source,
                })
        }
        Err(source) => Err(HubError::Bind {
            addr: preferred,
            source,
        }),
    }
}

#[cfg(test)]
mod tests {
    /// D-10.1: hub internals are reusable bricks, licensed Apache-2.0
    /// (the assembled application in `apps/` is AGPL-3.0-only).
    #[test]
    fn crate_license_follows_d_10_1() {
        assert_eq!(env!("CARGO_PKG_LICENSE"), "Apache-2.0");
    }
}
