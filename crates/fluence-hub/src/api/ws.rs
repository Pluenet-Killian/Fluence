// SPDX-License-Identifier: Apache-2.0

//! `/ws` — the multiplexed event channel (SPEC §2.A; contract in
//! `fluence-protocol::ws`).
//!
//! Open-time negotiation: `?topics=…&v=1&token=…` (the browser
//! `WebSocket` API cannot set headers — ADR-0004 §1). The granted topics
//! are `requested ∩ allowed_by_scope`; the first frame is `system.hello`
//! with the outcome. Heartbeat: protocol-level ping every 5 s.

use std::collections::HashSet;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::Response;
use fluence_protocol::api::pair::Scope;
use fluence_protocol::api::system::SystemEvent;
use fluence_protocol::ws::{ServerFrame, Topic};
use serde::Deserialize;

use crate::api::problem_response;
use crate::auth::token_hash;
use crate::state::AppState;

/// Heartbeat period (SPEC §2.A: 5 s).
const PING_PERIOD: Duration = Duration::from_secs(5);

/// Topics a scope may subscribe to (SPEC §2.A scope table). `system`
/// passes everywhere and is handled separately.
#[must_use]
pub fn allowed_topics(scope: Scope) -> &'static [Topic] {
    match scope {
        // Read-only: state events. (A display-content topic arrives with
        // the partner screen, Phase 5.)
        Scope::Display | Scope::Care => &[Topic::System],
        // Composing: the full event surface. `system` already passes
        // everywhere (auth), but listing it keeps this self-contained.
        Scope::Control | Scope::System => &[
            Topic::Input,
            Topic::Suggest,
            Topic::Voice,
            Topic::Asr,
            Topic::System,
        ],
    }
}

/// Query parameters of the upgrade request.
#[derive(Debug, Deserialize)]
pub struct WsQuery {
    /// Comma-separated topic list.
    topics: String,
    /// Input protocol version.
    v: u32,
    /// Device token (query form — ADR-0004 §1). Never logged.
    token: String,
}

/// `GET /ws`: authenticates, filters topics by scope, upgrades.
pub async fn upgrade(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if query.v != fluence_protocol::INPUT_PROTOCOL_VERSION {
        return problem_response(
            fluence_protocol::error::ErrorCode::ValidationFailed,
            Some(format!(
                "unsupported protocol version {} (hub speaks {})",
                query.v,
                fluence_protocol::INPUT_PROTOCOL_VERSION
            )),
        );
    }
    let device = match state
        .store()
        .device_by_token_hash(token_hash(&query.token))
        .await
    {
        Ok(Some(device)) => device,
        Ok(None) => {
            state
                .journal("auth.rejected", None, Some("invalid token on /ws"))
                .await;
            return problem_response(fluence_protocol::error::ErrorCode::TokenInvalid, None);
        }
        Err(error) => {
            tracing::error!(%error, "store unavailable during ws auth");
            return problem_response(fluence_protocol::error::ErrorCode::Internal, None);
        }
    };

    let allowed = allowed_topics(device.scope);
    let granted: Vec<Topic> = parse_topics(&query.topics)
        .into_iter()
        .filter(|topic| allowed.contains(topic))
        .collect();

    // Cap concurrent `/ws` per device and hub-wide so a paired-but-rogue
    // device cannot exhaust file descriptors and starve the keyboard
    // (F15 / SPEC §2.C). Reserved BEFORE upgrade: a refusal commits no task
    // and no bus subscription. The guard moves into `serve`, releasing the
    // slot when the connection ends by any path.
    let Some(guard) = state.try_acquire_ws(&device.device_id) else {
        state
            .journal(
                "ws.rejected",
                Some(device.device_id.clone()),
                Some("ws connection quota reached"),
            )
            .await;
        return problem_response(fluence_protocol::error::ErrorCode::RateLimited, None);
    };

    upgrade.on_upgrade(move |socket| serve(socket, state, granted, guard))
}

/// Parses the comma-separated topic list, ignoring unknown names
/// (forward compatibility: an older hub tolerates newer topic names).
fn parse_topics(raw: &str) -> Vec<Topic> {
    raw.split(',')
        .filter_map(|name| {
            serde_json::from_value(serde_json::Value::String(name.trim().to_owned())).ok()
        })
        .collect()
}

/// Connection loop: hello, then fan out bus frames filtered by granted
/// topics, with heartbeat pings. `_guard` releases this connection's `/ws`
/// slot when the task ends, by any path (F15).
async fn serve(
    mut socket: WebSocket,
    state: AppState,
    granted: Vec<Topic>,
    _guard: crate::state::WsConnectionGuard,
) {
    let hello = ServerFrame::System(SystemEvent::Hello {
        v: fluence_protocol::INPUT_PROTOCOL_VERSION,
        topics: granted.clone(),
    });
    if send_frame(&mut socket, &hello).await.is_err() {
        return;
    }

    let granted: HashSet<Topic> = granted.into_iter().collect();
    let mut bus = state.bus().subscribe();
    let mut ping = tokio::time::interval(PING_PERIOD);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            received = bus.recv() => {
                match received {
                    Ok(frame) => {
                        // Deliver only frames whose topic this connection
                        // subscribed to. A frame of an unrecognized future
                        // topic (`None`) is never delivered to a
                        // topic-filtered subscriber — silence is safer than
                        // misrouting onto the wrong channel.
                        if frame_topic(&frame).is_some_and(|topic| granted.contains(&topic))
                            && send_frame(&mut socket, &frame).await.is_err()
                        {
                            return; // client gone
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::debug!(missed, "ws subscriber lagged; events dropped");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
            }
            _ = ping.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    return;
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    // Client input frames are a Phase 5 concern (remote
                    // sensors); accepted and dropped with a debug trace
                    // until the input engine lands.
                    Some(Ok(Message::Text(_))) => {
                        tracing::debug!("client frame ignored (input engine arrives Phase 5)");
                    }
                    Some(Ok(Message::Close(_))) | None => return,
                    Some(Ok(_)) => {} // pongs, binary: nothing to do yet
                    Some(Err(error)) => {
                        tracing::debug!(%error, "ws receive error");
                        return;
                    }
                }
            }
        }
    }
}

/// Topic a server frame belongs to, for subscription filtering.
///
/// `ServerFrame` is `#[non_exhaustive]`, so a wildcard arm is mandatory.
/// Rather than guess a topic for a variant added by a newer protocol, we
/// return `None` and the caller withholds the frame — a connection is
/// never sent events on a channel it could not name.
fn frame_topic(frame: &ServerFrame) -> Option<Topic> {
    match frame {
        ServerFrame::Input(_) => Some(Topic::Input),
        ServerFrame::System(_) => Some(Topic::System),
        _ => None,
    }
}

/// Serializes and sends one frame.
async fn send_frame(socket: &mut WebSocket, frame: &ServerFrame) -> Result<(), axum::Error> {
    let json = serde_json::to_string(frame).expect("contract frames serialize");
    socket.send(Message::Text(json.into())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_topic_matrix_follows_spec_2a() {
        assert_eq!(allowed_topics(Scope::Display), &[Topic::System]);
        assert_eq!(allowed_topics(Scope::Care), &[Topic::System]);
        assert!(allowed_topics(Scope::Control).contains(&Topic::Input));
        assert!(allowed_topics(Scope::System).contains(&Topic::Input));
    }

    #[test]
    fn unknown_topic_names_are_ignored() {
        let parsed = parse_topics("input,brain-waves,system");
        assert_eq!(parsed, vec![Topic::Input, Topic::System]);
    }
}
