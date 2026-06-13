// SPDX-License-Identifier: Apache-2.0

//! Worker supervision (PLAN 2.3; SPEC §2.C, D-2.6).
//!
//! One task per worker runs the lifecycle loop:
//!
//! ```text
//! spawn child → IPC hello (Starting) → Ready
//!     ├─ heartbeat ping 1 s, pong timeout 3 s ──┐
//!     └─ child exits ──────────────────────────┤
//!                                              ▼
//!                              Down + system.degraded event
//!                              backoff (exp ×2, cap 10 s, jitter)
//!                              respawn (restart_count += 1)
//! ```
//!
//! Crash detection is event-driven (`child.wait()`), not poll-driven: a
//! killed worker is observed immediately — the `< 500 ms` kill-test bound
//! (PLAN T4) does not depend on the heartbeat period. The heartbeat
//! catches the other failure mode: a process that is alive but wedged.

mod backoff;
mod llama;

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use fluence_protocol::api::system::{SystemEvent, WorkerKind, WorkerState};
use fluence_protocol::ws::ServerFrame;
use tokio::process::Command;
use tokio::sync::watch;
use tokio::time::timeout;

pub use backoff::Backoff;
use fluence_ipc::{HubToWorker, IPC_PROTOCOL_VERSION, IpcEndpoint, WorkerToHub};
pub use llama::{LlamaSpec, SupervisedLlama, supervise_llama_server};

use crate::events::EventBus;

/// Heartbeat probe period.
const PING_PERIOD: Duration = Duration::from_secs(1);
/// Missing pongs for this long mean the worker is wedged.
const PONG_TIMEOUT: Duration = Duration::from_secs(3);
/// Time allowed for spawn → hello before declaring the start failed.
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);

/// What to supervise.
#[derive(Debug, Clone)]
pub struct WorkerSpec {
    /// Which worker this is (health reporting, events).
    pub kind: WorkerKind,
    /// Executable to spawn. Receives `--ipc <endpoint>` as arguments.
    pub command: PathBuf,
}

/// Live view of one worker, kept fresh by its lifecycle task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerStatus {
    /// Which worker.
    pub kind: WorkerKind,
    /// Current lifecycle state.
    pub state: WorkerState,
    /// Restarts since hub boot.
    pub restart_count: u32,
}

/// Handle over a supervised worker: observe state, request shutdown.
#[derive(Debug)]
pub struct WorkerHandle {
    status: watch::Receiver<WorkerStatus>,
    shutdown: watch::Sender<bool>,
}

impl WorkerHandle {
    /// Current status snapshot.
    #[must_use]
    pub fn status(&self) -> WorkerStatus {
        self.status.borrow().clone()
    }

    /// Asks the lifecycle loop to stop (polite IPC `Shutdown`, then kill).
    pub fn request_shutdown(&self) {
        let _ = self.shutdown.send(true);
    }
}

/// Spawns the lifecycle task for `spec` and returns its handle.
#[must_use]
pub fn supervise(spec: WorkerSpec, bus: EventBus) -> Arc<WorkerHandle> {
    let (status_tx, status_rx) = watch::channel(WorkerStatus {
        kind: spec.kind,
        state: WorkerState::Starting,
        restart_count: 0,
    });
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let handle = Arc::new(WorkerHandle {
        status: status_rx,
        shutdown: shutdown_tx,
    });

    tokio::spawn(lifecycle(spec, bus, status_tx, shutdown_rx));
    handle
}

/// Publishes a state change on both the watch (health) and the bus (WS).
fn publish_state(
    status_tx: &watch::Sender<WorkerStatus>,
    bus: &EventBus,
    kind: WorkerKind,
    state: WorkerState,
    restart_count: u32,
) {
    let _ = status_tx.send(WorkerStatus {
        kind,
        state,
        restart_count,
    });
    bus.publish(ServerFrame::System(SystemEvent::Degraded {
        worker: kind,
        state,
        restart_count: Some(restart_count),
    }));
}

/// The lifecycle loop (one per worker, lives as long as the hub).
async fn lifecycle(
    spec: WorkerSpec,
    bus: EventBus,
    status_tx: watch::Sender<WorkerStatus>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut backoff = Backoff::supervisor();
    let mut restart_count = 0u32;

    loop {
        if *shutdown.borrow() {
            return;
        }
        publish_state(
            &status_tx,
            &bus,
            spec.kind,
            WorkerState::Starting,
            restart_count,
        );

        match run_one_instance(&spec, &mut shutdown, || {
            publish_state(
                &status_tx,
                &bus,
                spec.kind,
                WorkerState::Ready,
                restart_count,
            );
            backoff.reset();
        })
        .await
        {
            InstanceOutcome::ShutdownRequested => {
                publish_state(
                    &status_tx,
                    &bus,
                    spec.kind,
                    WorkerState::Down,
                    restart_count,
                );
                return;
            }
            InstanceOutcome::Died(reason) => {
                restart_count = restart_count.saturating_add(1);
                tracing::warn!(
                    worker = ?spec.kind,
                    %reason,
                    restart_count,
                    "worker died; backing off then restarting"
                );
                publish_state(
                    &status_tx,
                    &bus,
                    spec.kind,
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

/// Why a worker instance ended.
enum InstanceOutcome {
    /// The hub asked us to stop.
    ShutdownRequested,
    /// The process died or stopped answering (reason for the log).
    Died(String),
}

/// Runs one worker instance to completion: spawn, handshake, heartbeat.
/// `on_ready` fires once after a successful hello.
async fn run_one_instance(
    spec: &WorkerSpec,
    shutdown: &mut watch::Receiver<bool>,
    on_ready: impl FnOnce(),
) -> InstanceOutcome {
    let endpoint = IpcEndpoint::unique("worker");
    let mut listener = match fluence_ipc::listen(&endpoint).await {
        Ok(listener) => listener,
        Err(error) => return InstanceOutcome::Died(format!("ipc listen failed: {error}")),
    };

    let mut child = match Command::new(&spec.command)
        .arg("--ipc")
        .arg(endpoint.as_path())
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return InstanceOutcome::Died(format!("spawn failed: {error}")),
    };

    // Accept + hello within a bound: a worker that connects but never
    // answers must not wedge the supervisor.
    let handshake = async {
        let mut connection = listener.accept().await?;
        connection
            .send(&HubToWorker::Hello {
                v: IPC_PROTOCOL_VERSION,
            })
            .await?;
        let ack: Option<WorkerToHub> = connection.recv().await?;
        Ok::<_, fluence_ipc::IpcError>((connection, ack))
    };
    let (mut connection, ack) = tokio::select! {
        result = timeout(HELLO_TIMEOUT, handshake) => match result {
            Ok(Ok(pair)) => pair,
            Ok(Err(error)) => {
                let _ = child.kill().await;
                return InstanceOutcome::Died(format!("handshake failed: {error}"));
            }
            Err(_) => {
                let _ = child.kill().await;
                return InstanceOutcome::Died("handshake timed out".into());
            }
        },
        _ = shutdown.changed() => {
            let _ = child.kill().await;
            return InstanceOutcome::ShutdownRequested;
        }
    };
    match ack {
        Some(WorkerToHub::HelloAck { v, .. }) if v == IPC_PROTOCOL_VERSION => {}
        other => {
            let _ = child.kill().await;
            return InstanceOutcome::Died(format!("bad hello ack: {other:?}"));
        }
    }
    on_ready();

    // Steady state: ping every second; any of (child exit, pong silence,
    // shutdown request) ends the instance.
    let mut ping_interval = tokio::time::interval(PING_PERIOD);
    ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut seq = 0u64;
    let mut last_pong = tokio::time::Instant::now();

    loop {
        tokio::select! {
            exit = child.wait() => {
                return InstanceOutcome::Died(match exit {
                    Ok(status) => format!("process exited: {status}"),
                    Err(error) => format!("wait failed: {error}"),
                });
            }
            _ = shutdown.changed() => {
                // Polite first; the kill covers a worker ignoring it.
                let _ = connection.send(&HubToWorker::Shutdown).await;
                let _ = timeout(Duration::from_secs(2), child.wait()).await;
                let _ = child.kill().await;
                return InstanceOutcome::ShutdownRequested;
            }
            _ = ping_interval.tick() => {
                if last_pong.elapsed() > PONG_TIMEOUT {
                    let _ = child.kill().await;
                    return InstanceOutcome::Died("heartbeat timed out (wedged)".into());
                }
                seq = seq.wrapping_add(1);
                if let Err(error) = connection.send(&HubToWorker::Ping { seq }).await {
                    // The wait() arm will report the exit; keep looping.
                    tracing::debug!(%error, "ping send failed; awaiting child exit");
                }
            }
            received = connection.recv::<WorkerToHub>() => {
                match received {
                    Ok(Some(WorkerToHub::Pong { .. })) => last_pong = tokio::time::Instant::now(),
                    Ok(Some(other)) => {
                        tracing::debug!(kind = message_kind(&other), "unexpected worker message");
                    }
                    Ok(None) | Err(_) => {
                        // The IPC channel closed: the worker is gone or
                        // malfunctioning. Report it now rather than spin on a
                        // perpetually-ready closed stream until wait() wins
                        // the select (which would also mask the heartbeat).
                        let _ = child.kill().await;
                        return InstanceOutcome::Died("ipc connection closed".into());
                    }
                }
            }
        }
    }
}

/// Production jitter in `[-1, 1]`.
fn prod_jitter() -> f64 {
    rand::random_range(-1.0..=1.0)
}

/// Non-P0 label of a worker message (for debug logs).
fn message_kind(message: &WorkerToHub) -> &'static str {
    match message {
        WorkerToHub::HelloAck { .. } => "hello_ack",
        WorkerToHub::Pong { .. } => "pong",
        WorkerToHub::EchoReply { .. } => "echo_reply",
    }
}
