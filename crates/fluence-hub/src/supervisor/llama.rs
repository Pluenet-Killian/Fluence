// SPDX-License-Identifier: Apache-2.0

//! Supervision of the `llama-server` subprocess (PLAN 4.2; ADR-0007; D-2.6).
//!
//! Unlike the IPC workers ([`super`]), `llama-server` is an HTTP server, so its
//! lifecycle is gated on `GET /health` rather than an IPC handshake:
//!
//! ```text
//! spawn child → poll /health until ok (or it dies) → Ready, flip `ready` on
//!     └─ child exits ───────────────────────────────────────┐
//!                                                            ▼
//!                            Down + system.degraded, flip `ready` off
//!                            backoff (exp ×2, cap 10 s, jitter) → respawn
//! ```
//!
//! The hub injects ONE [`SupervisedLlama`] as the acceleration engine; this
//! task is the sole writer of its shared `ready` flag. While the flag is off
//! (starting, crashed, backing off, shutting down) the backend returns
//! [`BackendError::Unavailable`], so `/suggest` and `/next-chars` degrade to the
//! n-gram fallback instead of blocking on a dead socket — « le clavier parle
//! toujours » (D-2.6). Crash isolation is the process boundary itself: a GGML
//! fault takes down the child, never the keyboard path.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use fluence_inference::{
    BackendError, CancelToken, GenerateOutcome, GenerateRequest, LlamaServerBackend, LlmBackend,
};
use fluence_protocol::api::suggest::CharProb;
use fluence_protocol::api::system::{WorkerKind, WorkerState};
use tokio::process::Command;
use tokio::sync::watch;
use tokio::time::Instant;

use super::{Backoff, WorkerHandle, WorkerStatus, prod_jitter, publish_state};
use crate::events::EventBus;

/// Longest a fresh instance may take to load its model and answer `/health`
/// before the supervisor gives up on it and restarts (model load on the 8 GiB
/// tier can be slow; this is generous on purpose).
const READY_TIMEOUT: Duration = Duration::from_secs(120);
/// How often `/health` is polled while waiting for readiness.
const HEALTH_POLL_PERIOD: Duration = Duration::from_millis(250);

/// What to spawn for the LLM backend.
#[derive(Debug, Clone)]
pub struct LlamaSpec {
    /// The `llama-server` binary (llama.cpp).
    pub command: PathBuf,
    /// The GGUF model to load.
    pub model: PathBuf,
    /// Loopback port — chosen once and reused across respawns so the backend's
    /// base URL is stable (the `ready` flag, not the URL, tracks availability).
    pub port: u16,
    /// Context window passed as `-c`.
    pub context_size: u32,
}

/// The LLM backend wrapped in a readiness gate.
///
/// One of these is the hub's engine. The supervisor flips `ready` on once the
/// server is healthy and off the moment it dies; until then every call returns
/// [`BackendError::Unavailable`] so the endpoints degrade to the n-gram
/// fallback (D-2.6) instead of stalling on a connection that will be refused.
#[derive(Debug, Clone)]
pub struct SupervisedLlama {
    inner: LlamaServerBackend,
    ready: Arc<AtomicBool>,
}

impl SupervisedLlama {
    /// Wraps a backend at `base_url`, gated by the shared `ready` flag (the
    /// supervisor returned by [`supervise_llama_server`] is its only writer).
    #[must_use]
    pub fn new(base_url: &str, ready: Arc<AtomicBool>) -> Self {
        Self {
            inner: LlamaServerBackend::new(base_url),
            ready,
        }
    }

    fn unavailable() -> BackendError {
        BackendError::Unavailable("llama-server not ready".to_owned())
    }

    fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
}

impl LlmBackend for SupervisedLlama {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn generate(
        &self,
        request: &GenerateRequest,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(&str),
    ) -> Result<GenerateOutcome, BackendError> {
        if !self.is_ready() {
            return Err(Self::unavailable());
        }
        self.inner.generate(request, cancel, sink)
    }

    fn next_chars(&self, context: &str, top_k: usize) -> Result<Vec<CharProb>, BackendError> {
        if !self.is_ready() {
            return Err(Self::unavailable());
        }
        self.inner.next_chars(context, top_k)
    }
}

/// Spawns the `llama-server` supervision task and returns its handle.
///
/// `ready` is shared with the [`SupervisedLlama`] engine the hub injected: this
/// task is its sole writer.
#[must_use]
pub fn supervise_llama_server(
    spec: LlamaSpec,
    bus: EventBus,
    ready: Arc<AtomicBool>,
) -> Arc<WorkerHandle> {
    let (status_tx, status_rx) = watch::channel(WorkerStatus {
        kind: WorkerKind::Llm,
        state: WorkerState::Starting,
        restart_count: 0,
    });
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let handle = Arc::new(WorkerHandle {
        status: status_rx,
        shutdown: shutdown_tx,
    });
    tokio::spawn(lifecycle(spec, bus, ready, status_tx, shutdown_rx));
    handle
}

/// Why a `llama-server` instance ended.
enum InstanceOutcome {
    /// The hub asked us to stop.
    ShutdownRequested,
    /// The process died or never became healthy (reason for the log).
    Died(String),
}

/// The lifecycle loop (one per hub, lives as long as the hub).
async fn lifecycle(
    spec: LlamaSpec,
    bus: EventBus,
    ready: Arc<AtomicBool>,
    status_tx: watch::Sender<WorkerStatus>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut backoff = Backoff::supervisor();
    let mut restart_count = 0u32;

    loop {
        if *shutdown.borrow() {
            return;
        }
        ready.store(false, Ordering::Release); // not ready while (re)starting
        publish_state(
            &status_tx,
            &bus,
            WorkerKind::Llm,
            WorkerState::Starting,
            restart_count,
        );

        let outcome = run_one_instance(&spec, &ready, &mut shutdown, || {
            publish_state(
                &status_tx,
                &bus,
                WorkerKind::Llm,
                WorkerState::Ready,
                restart_count,
            );
            backoff.reset();
        })
        .await;

        ready.store(false, Ordering::Release); // never serve a dead instance
        match outcome {
            InstanceOutcome::ShutdownRequested => {
                publish_state(
                    &status_tx,
                    &bus,
                    WorkerKind::Llm,
                    WorkerState::Down,
                    restart_count,
                );
                return;
            }
            InstanceOutcome::Died(reason) => {
                restart_count = restart_count.saturating_add(1);
                tracing::warn!(
                    %reason,
                    restart_count,
                    "llama-server died; backing off then restarting"
                );
                publish_state(
                    &status_tx,
                    &bus,
                    WorkerKind::Llm,
                    WorkerState::Down,
                    restart_count,
                );
                let delay = backoff.next_delay(prod_jitter());
                tokio::select! {
                    () = tokio::time::sleep(delay) => {}
                    _ = shutdown.changed() => return,
                }
            }
        }
    }
}

/// Runs one instance: spawn, wait for `/health`, then watch until it exits or a
/// shutdown is requested. `on_ready` fires once, after the first healthy probe.
async fn run_one_instance(
    spec: &LlamaSpec,
    ready: &Arc<AtomicBool>,
    shutdown: &mut watch::Receiver<bool>,
    on_ready: impl FnOnce(),
) -> InstanceOutcome {
    // stdout/stderr go to the void on purpose: llama-server can log prompt and
    // completion text, which is P0 — it must never reach our logs (§9.A).
    let mut child = match Command::new(&spec.command)
        .arg("-m")
        .arg(&spec.model)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(spec.port.to_string())
        .arg("-c")
        .arg(spec.context_size.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return InstanceOutcome::Died(format!("spawn failed: {error}")),
    };

    let backend = LlamaServerBackend::new(&format!("http://127.0.0.1:{}", spec.port));
    let deadline = Instant::now() + READY_TIMEOUT;

    // Wait for the first healthy probe, bailing out if the child dies during
    // model load, a shutdown lands, or the deadline passes.
    loop {
        if is_healthy(&backend).await {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill().await;
            return InstanceOutcome::Died("health check timed out".to_owned());
        }
        tokio::select! {
            () = tokio::time::sleep(HEALTH_POLL_PERIOD) => {}
            exit = child.wait() => {
                return InstanceOutcome::Died(match exit {
                    Ok(status) => format!("exited during model load: {status}"),
                    Err(error) => format!("wait failed during load: {error}"),
                });
            }
            _ = shutdown.changed() => {
                let _ = child.kill().await;
                return InstanceOutcome::ShutdownRequested;
            }
        }
    }

    ready.store(true, Ordering::Release);
    on_ready();

    tokio::select! {
        exit = child.wait() => InstanceOutcome::Died(match exit {
            Ok(status) => format!("process exited: {status}"),
            Err(error) => format!("wait failed: {error}"),
        }),
        _ = shutdown.changed() => {
            let _ = child.kill().await;
            InstanceOutcome::ShutdownRequested
        }
    }
}

/// `GET /health` off the async runtime (the backend's client is blocking).
async fn is_healthy(backend: &LlamaServerBackend) -> bool {
    let backend = backend.clone();
    tokio::task::spawn_blocking(move || backend.is_healthy())
        .await
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The readiness gate: a not-ready backend never reaches the network — it
    /// degrades immediately so the endpoints fall back to the n-gram (D-2.6).
    #[test]
    fn a_not_ready_backend_is_unavailable_on_every_path() {
        let ready = Arc::new(AtomicBool::new(false));
        // A dead URL would also fail, but the gate must short-circuit *before*
        // any network call: with `ready` false, both paths are Unavailable.
        let backend = SupervisedLlama::new("http://127.0.0.1:1", ready.clone());
        assert_eq!(backend.id(), "llama-server");

        let request = GenerateRequest {
            prompt: "x".to_owned(),
            max_tokens: 8,
        };
        assert!(matches!(
            backend.generate(&request, &CancelToken::new(), &mut |_| {}),
            Err(BackendError::Unavailable(_))
        ));
        assert!(matches!(
            backend.next_chars("x", 4),
            Err(BackendError::Unavailable(_))
        ));

        // Flipping the flag on lets calls through to the inner client (which
        // then fails on the dead URL — but no longer at the gate).
        ready.store(true, Ordering::Release);
        assert!(matches!(
            backend.next_chars("x", 4),
            Err(BackendError::Unavailable(_))
        ));
    }
}
