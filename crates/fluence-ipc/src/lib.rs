// SPDX-License-Identifier: Apache-2.0

//! Hub ↔ worker IPC for Fluence (SPEC §2.C, D-2.6; ADR-0005).
//!
//! Inference workers run as supervised child processes so a native-library
//! crash never kills input. This crate is the wire between them and the
//! hub: **length-prefixed JSON messages** over Unix domain sockets (Linux)
//! or named pipes (Windows), behind one platform-neutral API.
//!
//! Design choices (SPEC §2.C):
//! - JSON frames: debuggable with standard tools; a binary optimization
//!   can come later without changing this API.
//! - Length-prefix (`u32`, 16 MiB cap): a malformed or hostile worker
//!   cannot make the hub allocate unbounded memory.
//! - The endpoint is a plain platform path ([`IpcEndpoint`]): the
//!   supervisor passes it to workers as a CLI argument.
//!
//! The audio path (ring buffer in shared memory, SPEC §2.C) is *not* this
//! crate: it arrives with the TTS worker (Phase 5).

mod endpoint;
mod messages;
mod transport;

pub use endpoint::IpcEndpoint;
pub use messages::{HubToWorker, IPC_PROTOCOL_VERSION, WorkerToHub};
pub use transport::{IpcConnection, IpcError, IpcListener, MAX_FRAME_BYTES, connect, listen};

#[cfg(test)]
mod tests {
    /// D-10.1: the IPC layer is a reusable brick, licensed Apache-2.0.
    #[test]
    fn crate_license_follows_d_10_1() {
        assert_eq!(env!("CARGO_PKG_LICENSE"), "Apache-2.0");
    }
}
