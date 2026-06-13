// SPDX-License-Identifier: Apache-2.0

//! IPC protocol v0 — the hub↔worker control vocabulary.
//!
//! Internal contract (not part of the published `fluence-protocol`
//! schemas): both ends live in this repository and ship together; the
//! version handshake exists so a stale worker binary fails loudly instead
//! of misbehaving.

use serde::{Deserialize, Serialize};

/// Version negotiated in `Hello`/`HelloAck`. Bump on any breaking change
/// to the message set.
pub const IPC_PROTOCOL_VERSION: u32 = 0;

/// Messages the hub sends to a worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum HubToWorker {
    /// First message after connect: version check.
    Hello {
        /// Hub's [`IPC_PROTOCOL_VERSION`].
        v: u32,
    },
    /// Liveness probe; the worker answers [`WorkerToHub::Pong`] with the
    /// same sequence number.
    Ping {
        /// Monotonic probe number.
        seq: u64,
    },
    /// Test-harness request: echo `payload` back (`worker-echo` only;
    /// real workers answer their domain messages instead — Phase 4+).
    Echo {
        /// Arbitrary payload to mirror.
        payload: String,
    },
    /// Polite stop: the worker should flush and exit 0. The supervisor
    /// kills the process if it lingers.
    Shutdown,
}

/// Messages a worker sends to the hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum WorkerToHub {
    /// Answer to `Hello`: identity and version.
    HelloAck {
        /// Worker's [`IPC_PROTOCOL_VERSION`] (must equal the hub's).
        v: u32,
        /// Worker kind label (`echo`, `llm`, `tts`…).
        kind: String,
        /// Worker process id (diagnostics, kill-tests).
        pid: u32,
    },
    /// Liveness answer, mirroring the `Ping` sequence number.
    Pong {
        /// The probed sequence number.
        seq: u64,
    },
    /// Test-harness answer to `Echo`.
    EchoReply {
        /// The mirrored payload.
        payload: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_with_k_tag() {
        let ping = HubToWorker::Ping { seq: 42 };
        let json = serde_json::to_string(&ping).unwrap();
        assert!(json.contains(r#""k":"ping""#));
        assert_eq!(serde_json::from_str::<HubToWorker>(&json).unwrap(), ping);

        let ack = WorkerToHub::HelloAck {
            v: 0,
            kind: "echo".into(),
            pid: 1234,
        };
        let json = serde_json::to_string(&ack).unwrap();
        assert_eq!(serde_json::from_str::<WorkerToHub>(&json).unwrap(), ack);
    }
}
