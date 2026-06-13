// SPDX-License-Identifier: Apache-2.0

//! WebSocket envelope: one `/ws` connection, multiplexed by topics
//! (SPEC §2.A).
//!
//! **Connection contract** — designed so a client never needs a second
//! round-trip before receiving events:
//!
//! 1. The client opens `GET /ws?topics=input,system&v=1` with its device
//!    token (header `X-Fluence-Token`). Subscription and protocol version
//!    are negotiated *at open time* via the query string (SPEC §4.A:
//!    `v:1` negotiated when the topic opens).
//! 2. The hub filters the requested topics by the token's scope (§2.A) and
//!    answers with a first frame: [`crate::api::system::SystemEvent::Hello`]
//!    listing the granted topics and retained version.
//! 3. Heartbeat: protocol-level WebSocket ping every 5 s; a missed pong
//!    closes the connection. Clients reconnect automatically and resume
//!    (drafts and sessions survive disconnections, §2.A).
//!
//! Frames are JSON objects `{"topic": …, "msg": …}`. Topics with no payload
//! defined yet (`asr`, `suggest`, `voice` — P2 domains) are reserved: their
//! names are valid for subscription, but no frame is emitted on them in v1.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::api::system::SystemEvent;
use crate::input::{InputClientMessage, SelectionEvent};

/// A multiplexing topic (SPEC §2.A). Subscription is filtered by scope:
/// `display` receives `system` + composed text; `control` adds `input`…
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Topic {
    /// Input protocol: samples in, selection events out (SPEC §4.A).
    Input,
    /// Partner speech transcription (P2 — reserved, no frames in v1).
    Asr,
    /// Push suggestions (P2 — reserved, no frames in v1).
    Suggest,
    /// Voice/TTS events (P2 — reserved, no frames in v1).
    Voice,
    /// System state: degradations, listening indicator, hello (SPEC §2.C).
    System,
}

/// Client → hub frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "topic", content = "msg", rename_all = "lowercase")]
pub enum ClientFrame {
    /// Input-topic message (sensor sample or target patch).
    Input(InputClientMessage),
}

/// Hub → client frame.
///
/// Marked `non_exhaustive`: new topics gain payloads over time (P2);
/// clients must ignore frames whose topic they do not handle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "topic", content = "msg", rename_all = "lowercase")]
#[non_exhaustive]
pub enum ServerFrame {
    /// Selection engine event.
    Input(SelectionEvent),
    /// System state event.
    System(SystemEvent),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::TimestampMicros;

    #[test]
    fn frames_carry_topic_and_msg() {
        let frame = ServerFrame::Input(SelectionEvent::Commit {
            target: "key_e".into(),
            method: crate::input::CommitMethod::Dwell,
            t: TimestampMicros(123),
        });
        let json = serde_json::to_value(&frame).unwrap();
        assert_eq!(json["topic"], "input");
        assert_eq!(json["msg"]["k"], "sel.commit");
    }
}
