// SPDX-License-Identifier: Apache-2.0

//! Platform transport: listen/connect + framed, typed send/recv.
//!
//! Framing: `u32` big-endian length prefix, 16 MiB cap
//! ([`MAX_FRAME_BYTES`]) — a misbehaving peer cannot make the other side
//! allocate unbounded memory. Payloads are JSON (debuggable; a binary
//! optimization can come later without changing this API — SPEC §2.C).

use tokio_util::codec::{Framed, LengthDelimitedCodec};

/// Hard cap on one frame. Generous for control messages (audio uses shared
/// memory, not this channel — SPEC §2.C).
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// IPC failure modes.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// Transport-level failure (peer died, frame too large, OS error).
    #[error("ipc transport error: {0}")]
    Io(#[from] std::io::Error),
    /// The peer sent bytes that are not the expected message type.
    #[error("ipc message decode error: {0}")]
    Decode(#[from] serde_json::Error),
}

/// Builds the codec with the project's framing parameters.
fn codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME_BYTES)
        .length_field_type::<u32>()
        .new_codec()
}

macro_rules! platform_transport {
    ($stream:ty, $listener:ty) => {
        /// A connected IPC channel with typed, framed send/recv.
        pub struct IpcConnection {
            framed: Framed<$stream, LengthDelimitedCodec>,
        }

        impl IpcConnection {
            fn new(stream: $stream) -> Self {
                Self {
                    framed: Framed::new(stream, codec()),
                }
            }

            /// Sends one message as one frame.
            ///
            /// # Errors
            ///
            /// [`IpcError::Io`] when the peer is gone or the frame exceeds
            /// [`super::MAX_FRAME_BYTES`]; [`IpcError::Decode`] if our own
            /// value fails to serialize (e.g. a non-string map key —
            /// impossible for the v0 message set, kept for safety).
            pub async fn send<T: serde::Serialize>(&mut self, message: &T) -> Result<(), IpcError> {
                use futures_util::SinkExt as _;
                let bytes = serde_json::to_vec(message)?;
                self.framed.send(bytes.into()).await?;
                Ok(())
            }

            /// Receives the next message. `Ok(None)` is a clean EOF (the
            /// peer closed without garbage) — distinct from an error.
            ///
            /// # Errors
            ///
            /// [`IpcError::Io`] on transport failure (including oversized
            /// frames); [`IpcError::Decode`] when the frame is not valid
            /// JSON for `T`.
            pub async fn recv<T: serde::de::DeserializeOwned>(
                &mut self,
            ) -> Result<Option<T>, IpcError> {
                use futures_util::StreamExt as _;
                match self.framed.next().await {
                    None => Ok(None),
                    Some(frame) => {
                        let frame = frame?;
                        Ok(Some(serde_json::from_slice(&frame)?))
                    }
                }
            }
        }

        /// An IPC server endpoint accepting worker connections.
        pub struct IpcListener {
            inner: $listener,
        }
    };
}

#[cfg(unix)]
mod platform {
    use tokio::net::{UnixListener, UnixStream};

    use super::{Framed, IpcError, LengthDelimitedCodec, codec};
    use crate::IpcEndpoint;

    platform_transport!(UnixStream, UnixListener);

    /// Binds `endpoint` (removing a stale socket file from a previous
    /// crashed run, detected by a refused probe connect).
    ///
    /// # Errors
    ///
    /// [`IpcError::Io`] when binding fails.
    pub async fn listen(endpoint: &IpcEndpoint) -> Result<IpcListener, IpcError> {
        let path = endpoint.as_path();
        if tokio::fs::try_exists(path).await.unwrap_or(false) {
            // A live socket accepts connections; a stale one refuses.
            if UnixStream::connect(path).await.is_err() {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
        Ok(IpcListener {
            inner: UnixListener::bind(path)?,
        })
    }

    impl IpcListener {
        /// Accepts the next worker connection.
        ///
        /// # Errors
        ///
        /// [`IpcError::Io`] on accept failure.
        pub async fn accept(&mut self) -> Result<IpcConnection, IpcError> {
            let (stream, _addr) = self.inner.accept().await?;
            Ok(IpcConnection::new(stream))
        }
    }

    /// Connects to a hub endpoint (worker side).
    ///
    /// # Errors
    ///
    /// [`IpcError::Io`] when the endpoint does not exist or refuses.
    pub async fn connect(endpoint: &IpcEndpoint) -> Result<IpcConnection, IpcError> {
        Ok(IpcConnection::new(
            UnixStream::connect(endpoint.as_path()).await?,
        ))
    }
}

#[cfg(windows)]
mod platform {
    use std::time::Duration;

    use tokio::net::windows::named_pipe::{
        ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions,
    };

    use super::{Framed, IpcError, LengthDelimitedCodec, codec};
    use crate::IpcEndpoint;

    /// `ERROR_PIPE_BUSY`: all instances busy — retry shortly.
    const PIPE_BUSY: i32 = 231;

    // On Windows the two pipe ends have distinct types; erase them behind
    // an enum so `IpcConnection` stays one type for callers.
    pub(super) enum PipeStream {
        Server(NamedPipeServer),
        Client(NamedPipeClient),
    }

    impl tokio::io::AsyncRead for PipeStream {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            match self.get_mut() {
                Self::Server(s) => std::pin::Pin::new(s).poll_read(cx, buf),
                Self::Client(c) => std::pin::Pin::new(c).poll_read(cx, buf),
            }
        }
    }

    impl tokio::io::AsyncWrite for PipeStream {
        fn poll_write(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            match self.get_mut() {
                Self::Server(s) => std::pin::Pin::new(s).poll_write(cx, buf),
                Self::Client(c) => std::pin::Pin::new(c).poll_write(cx, buf),
            }
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            match self.get_mut() {
                Self::Server(s) => std::pin::Pin::new(s).poll_flush(cx),
                Self::Client(c) => std::pin::Pin::new(c).poll_flush(cx),
            }
        }

        fn poll_shutdown(
            self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            match self.get_mut() {
                Self::Server(s) => std::pin::Pin::new(s).poll_shutdown(cx),
                Self::Client(c) => std::pin::Pin::new(c).poll_shutdown(cx),
            }
        }
    }

    platform_transport!(PipeStream, WindowsAccept);

    /// Windows accept state: the *next* (not yet connected) pipe instance.
    pub(super) struct WindowsAccept {
        path: String,
        pending: NamedPipeServer,
    }

    /// Creates the first pipe instance for `endpoint`.
    /// `first_pipe_instance(true)` prevents name squatting by another
    /// process (defense in depth — SPEC §9.A local attacker is out of the
    /// threat model, but the flag is free).
    ///
    /// # Errors
    ///
    /// [`IpcError::Io`] when the pipe cannot be created.
    pub async fn listen(endpoint: &IpcEndpoint) -> Result<IpcListener, IpcError> {
        let path = endpoint.as_path().to_owned();
        let pending = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&path)?;
        Ok(IpcListener {
            inner: WindowsAccept { path, pending },
        })
    }

    impl IpcListener {
        /// Accepts the next worker connection (and pre-creates the next
        /// pipe instance, per the named-pipe accept pattern).
        ///
        /// # Errors
        ///
        /// [`IpcError::Io`] on connect/create failure.
        pub async fn accept(&mut self) -> Result<IpcConnection, IpcError> {
            self.inner.pending.connect().await?;
            let next = ServerOptions::new().create(&self.inner.path)?;
            let connected = std::mem::replace(&mut self.inner.pending, next);
            Ok(IpcConnection::new(PipeStream::Server(connected)))
        }
    }

    /// Connects to a hub endpoint (worker side), retrying briefly on
    /// `ERROR_PIPE_BUSY` (all instances momentarily taken).
    ///
    /// # Errors
    ///
    /// [`IpcError::Io`] when the pipe does not exist or stays busy.
    pub async fn connect(endpoint: &IpcEndpoint) -> Result<IpcConnection, IpcError> {
        let path = endpoint.as_path();
        for _ in 0..10 {
            match ClientOptions::new().open(path) {
                Ok(client) => return Ok(IpcConnection::new(PipeStream::Client(client))),
                Err(e) if e.raw_os_error() == Some(PIPE_BUSY) => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(IpcError::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "named pipe stayed busy",
        )))
    }
}

pub use platform::{IpcConnection, IpcListener, connect, listen};
