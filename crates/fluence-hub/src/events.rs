// SPDX-License-Identifier: Apache-2.0

//! System event bus: one broadcast channel of [`ServerFrame`]s; each
//! WebSocket connection subscribes and filters by its granted topics
//! (ADR-0005 §3).
//!
//! Lagging receivers (a stalled client) lose old events rather than
//! blocking emitters — state events are observable through
//! `/system/health` anyway; the bus is a notification path, not a queue
//! of record.

use fluence_protocol::ws::ServerFrame;
use tokio::sync::broadcast;

/// Bus capacity. Generous for state-change events; if a client lags this
/// far behind, it reconnects and reads fresh state.
const CAPACITY: usize = 256;

/// Cloneable handle to the system event bus.
#[derive(Debug, Clone)]
pub struct EventBus {
    sender: broadcast::Sender<ServerFrame>,
}

impl EventBus {
    /// Creates the bus.
    #[must_use]
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(CAPACITY);
        Self { sender }
    }

    /// Publishes a frame to every connected subscriber. A bus with no
    /// subscribers silently drops (normal at boot).
    pub fn publish(&self, frame: ServerFrame) {
        let _ = self.sender.send(frame);
    }

    /// Subscribes from now on.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<ServerFrame> {
        self.sender.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use fluence_protocol::api::system::{SystemEvent, WorkerKind, WorkerState};

    use super::*;

    #[tokio::test]
    async fn subscribers_receive_published_frames() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe();
        let frame = ServerFrame::System(SystemEvent::Degraded {
            worker: WorkerKind::Llm,
            state: WorkerState::Down,
            restart_count: Some(1),
        });
        bus.publish(frame.clone());
        assert_eq!(receiver.recv().await.expect("frame"), frame);
    }

    #[tokio::test]
    async fn publishing_without_subscribers_is_fine() {
        let bus = EventBus::new();
        bus.publish(ServerFrame::System(SystemEvent::Listening {
            enabled: false,
        }));
    }
}
