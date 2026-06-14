// SPDX-License-Identifier: Apache-2.0

//! `/ws` — the multiplexed event channel (SPEC §2.A; contract in
//! `fluence-protocol::ws`).
//!
//! Open-time negotiation: `?topics=…&v=1&token=…` (the browser
//! `WebSocket` API cannot set headers — ADR-0004 §1). The granted topics
//! are `requested ∩ allowed_by_scope`; the first frame is `system.hello`
//! with the outcome. Heartbeat: protocol-level ping every 5 s.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::Response;
use fluence_input::{Calibrator, GazePipeline, SelectionUpdate};
use fluence_protocol::Normalized;
use fluence_protocol::api::pair::Scope;
use fluence_protocol::api::system::SystemEvent;
use fluence_protocol::common::TimestampMicros;
use fluence_protocol::input::{InputClientMessage, SelectionEvent};
use fluence_protocol::ws::{ClientFrame, ServerFrame, Topic};
use serde::Deserialize;

use crate::api::problem_response;
use crate::auth::token_hash;
use crate::events::EventBus;
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

    // A connection that subscribed to `input` (control/system scope) runs a
    // per-connection selection engine (SPEC §4.A, D-4.1), seeded from the
    // surface's declared targets. Its monotonic clock starts now, so dwell
    // accumulation replays deterministically (the lib is clock-free, PLAN 5.1).
    let started = Instant::now();
    let mut engine = granted.contains(&Topic::Input).then(|| {
        let mut pipeline = GazePipeline::new(Calibrator::new("default"));
        if let Some(map) = state.input_targets() {
            let _ = pipeline.set_targets(&map);
        }
        pipeline
    });

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
                    // Client input frames (remote sensors / target patches):
                    // drive this connection's selection engine and publish the
                    // resulting events. A connection without the `input` topic
                    // has no engine, so its text frames are ignored.
                    Some(Ok(Message::Text(text))) => {
                        if let Some(engine) = engine.as_mut() {
                            match serde_json::from_str::<ClientFrame>(&text) {
                                Ok(ClientFrame::Input(message)) => {
                                    relay_input(engine, &message, started.elapsed(), state.bus());
                                }
                                Err(error) => {
                                    // A malformed frame is the client's bug:
                                    // trace (no P0 in the error) and stay up.
                                    tracing::debug!(%error, "unparsable client input frame ignored");
                                }
                            }
                        }
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

/// Drives the per-connection selection engine with one client message and
/// publishes the resulting selection events (stamped with the hub clock) on the
/// bus, where every `input`-topic subscriber — the composer and any partner
/// screen — receives them (SPEC §4.A).
fn relay_input(
    pipeline: &mut GazePipeline,
    message: &InputClientMessage,
    now: Duration,
    bus: &EventBus,
) {
    let updates = match message {
        // Gaze sources (`gaze:…`, SPEC §4.A convention) run the full calibration
        // + fusion pipeline; everything else (mouse, by convention `mouse:…`) is
        // already in screen coordinates and drives the dwell engine directly.
        InputClientMessage::Pointer(sample) => {
            if sample.src.0.starts_with("gaze:") {
                pipeline.on_gaze(
                    &[sample.x.get(), sample.y.get()],
                    sample.pose,
                    sample.conf.get(),
                    now,
                )
            } else {
                pipeline.on_mouse(sample.x.get(), sample.y.get(), now)
            }
        }
        InputClientMessage::Switch(_) => pipeline.on_switch(now),
        InputClientMessage::TargetsPatch(patch) => pipeline.apply_patch(patch),
        // Calibration (SPEC §4.D): collect pairs, then fit on request. Neither
        // produces a selection event.
        InputClientMessage::CalibrationSample { target, x, y, .. } => {
            pipeline.add_calibration_sample(target, x.get(), y.get());
            Vec::new()
        }
        InputClientMessage::CalibrationFit { .. } => {
            let _ = pipeline.fit_calibration();
            Vec::new()
        }
    };
    for update in updates {
        bus.publish(ServerFrame::Input(stamp(update, now)));
    }
}

/// Stamps a clock-free [`SelectionUpdate`] into the wire [`SelectionEvent`]:
/// the engine decides, the hub dates (SPEC §4.A). `now` is the connection's
/// monotonic elapsed time, used both as the event timestamp and (by the engine)
/// for dwell accumulation, so a replay is deterministic.
fn stamp(update: SelectionUpdate, now: Duration) -> SelectionEvent {
    let t = TimestampMicros(u64::try_from(now.as_micros()).unwrap_or(u64::MAX));
    match update {
        SelectionUpdate::Focus { target } => SelectionEvent::Focus { target, t },
        SelectionUpdate::Dwell {
            target,
            progress,
            eta,
        } => SelectionEvent::Dwell {
            target,
            progress: Normalized::new(progress.clamp(0.0, 1.0))
                .expect("a clamped [0, 1] value is a valid Normalized"),
            eta_ms: u32::try_from(eta.as_millis()).unwrap_or(u32::MAX),
        },
        SelectionUpdate::Commit { target, method } => SelectionEvent::Commit { target, method, t },
        SelectionUpdate::Cancel => SelectionEvent::Cancel,
    }
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

    use fluence_protocol::input::{
        PointerSample, Target, TargetMap, TargetMapPatch, TargetRole, Viewport,
    };

    fn full_surface() -> TargetMap {
        TargetMap {
            surface: "main".into(),
            viewport: Viewport { w: 100, h: 100 },
            targets: vec![Target {
                id: "key_e".into(),
                rect: serde_json::from_str("[0, 0, 100, 100]").expect("valid rect"),
                role: TargetRole::Key,
                label: Some("e".to_owned()),
                prior: None,
            }],
        }
    }

    fn pointer_at(x: f64, y: f64) -> InputClientMessage {
        InputClientMessage::Pointer(PointerSample {
            t: TimestampMicros(0),
            src: "mouse:test".into(),
            x: Normalized::new(x).expect("in range"),
            y: Normalized::new(y).expect("in range"),
            conf: Normalized::new(1.0).expect("in range"),
            pose: None,
        })
    }

    /// A gaze pointer sample (`gaze:` source) — routed through the calibration
    /// + fusion pipeline rather than straight to the dwell engine.
    fn gaze_at(x: f64, y: f64) -> InputClientMessage {
        InputClientMessage::Pointer(PointerSample {
            t: TimestampMicros(0),
            src: "gaze:test".into(),
            x: Normalized::new(x).expect("in range"),
            y: Normalized::new(y).expect("in range"),
            conf: Normalized::new(1.0).expect("in range"),
            pose: None,
        })
    }

    fn cal_sample(target: &str, x: f64, y: f64) -> InputClientMessage {
        InputClientMessage::CalibrationSample {
            surface: "main".into(),
            target: target.into(),
            x: Normalized::new(x).expect("in range"),
            y: Normalized::new(y).expect("in range"),
        }
    }

    fn cal_fit() -> InputClientMessage {
        InputClientMessage::CalibrationFit {
            surface: "main".into(),
        }
    }

    #[tokio::test]
    async fn a_sustained_dwell_publishes_a_commit_on_the_bus() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe();
        let mut engine = GazePipeline::new(Calibrator::new("test"));
        let _ = engine.set_targets(&full_surface());

        // First sample establishes focus; a second past the base dwell commits.
        relay_input(&mut engine, &pointer_at(0.5, 0.5), Duration::ZERO, &bus);
        relay_input(
            &mut engine,
            &pointer_at(0.5, 0.5),
            Duration::from_millis(900),
            &bus,
        );

        let mut committed = false;
        while let Ok(frame) = receiver.try_recv() {
            if matches!(frame, ServerFrame::Input(SelectionEvent::Commit { .. })) {
                committed = true;
            }
        }
        assert!(committed, "a sustained dwell must publish a commit event");
    }

    #[tokio::test]
    async fn a_targets_patch_seeds_the_live_engine_so_a_later_dwell_commits() {
        // The path the web composer relies on (PLAN 5.1): instead of depending
        // on the `PUT /input/targets` snapshot winning a race against the `/ws`
        // upgrade, the UI seeds *this* connection's engine by sending a
        // `targets.patch` frame on its own socket, in order, before any pointer
        // sample. `relay_input` applies it; a later sustained dwell then commits.
        let bus = EventBus::new();
        let mut receiver = bus.subscribe();
        // A fresh engine with NO targets — exactly what `serve` builds when the
        // hub's snapshot is still empty. Without seeding, pointers hit nothing.
        let mut engine = GazePipeline::new(Calibrator::new("test"));

        let map = full_surface();
        let patch = InputClientMessage::TargetsPatch(TargetMapPatch {
            surface: map.surface.clone(),
            viewport: Some(map.viewport),
            upsert: map.targets.clone(),
            remove: Vec::new(),
        });
        relay_input(&mut engine, &patch, Duration::ZERO, &bus);

        // A sustained dwell on the just-seeded target must now commit.
        relay_input(&mut engine, &pointer_at(0.5, 0.5), Duration::ZERO, &bus);
        relay_input(
            &mut engine,
            &pointer_at(0.5, 0.5),
            Duration::from_millis(900),
            &bus,
        );

        let mut committed = false;
        while let Ok(frame) = receiver.try_recv() {
            if matches!(frame, ServerFrame::Input(SelectionEvent::Commit { .. })) {
                committed = true;
            }
        }
        assert!(
            committed,
            "a targets.patch over the ws must seed the engine so a later dwell commits"
        );
    }

    #[tokio::test]
    async fn a_connection_without_targets_publishes_nothing() {
        let bus = EventBus::new();
        let mut receiver = bus.subscribe();
        let mut engine = GazePipeline::new(Calibrator::new("test"));
        // No targets declared: a pointer hits nothing, so no event is emitted.
        relay_input(&mut engine, &pointer_at(0.5, 0.5), Duration::ZERO, &bus);
        assert!(
            receiver.try_recv().is_err(),
            "no target ⇒ no selection event"
        );
    }

    #[tokio::test]
    async fn a_calibrated_gaze_stream_commits_via_the_pipeline() {
        // The end-to-end hub gaze path (SPEC §4.C/§4.D): calibrate from raw-gaze
        // → target pairs, fit, then a sustained gaze fixation drives the
        // calibration + fusion + dwell pipeline to a commit on the bus.
        let bus = EventBus::new();
        let mut receiver = bus.subscribe();
        let mut engine = GazePipeline::new(Calibrator::new("test"));
        let _ = engine.set_targets(&full_surface());

        for (x, y) in [(0.3, 0.3), (0.7, 0.3), (0.5, 0.7), (0.5, 0.5)] {
            relay_input(
                &mut engine,
                &cal_sample("key_e", x, y),
                Duration::ZERO,
                &bus,
            );
        }
        relay_input(&mut engine, &cal_fit(), Duration::ZERO, &bus);

        // Steady fixation, samples within the I-VT loss window so the dwell
        // accumulates past the base 800 ms.
        let mut t = 0u64;
        for _ in 0..12 {
            relay_input(
                &mut engine,
                &gaze_at(0.5, 0.5),
                Duration::from_millis(t),
                &bus,
            );
            t += 100;
        }

        let mut committed = false;
        while let Ok(frame) = receiver.try_recv() {
            if matches!(frame, ServerFrame::Input(SelectionEvent::Commit { .. })) {
                committed = true;
            }
        }
        assert!(
            committed,
            "a calibrated, sustained gaze must commit via the pipeline"
        );
    }

    #[tokio::test]
    async fn an_uncalibrated_gaze_publishes_nothing() {
        // Without a fitted calibration the pipeline cannot map features to the
        // screen, so it holds and emits nothing (no spurious selection).
        let bus = EventBus::new();
        let mut receiver = bus.subscribe();
        let mut engine = GazePipeline::new(Calibrator::new("test"));
        let _ = engine.set_targets(&full_surface());
        let mut t = 0u64;
        for _ in 0..12 {
            relay_input(
                &mut engine,
                &gaze_at(0.5, 0.5),
                Duration::from_millis(t),
                &bus,
            );
            t += 100;
        }
        assert!(
            receiver.try_recv().is_err(),
            "uncalibrated gaze must emit nothing"
        );
    }

    #[test]
    fn stamp_maps_dwell_progress_and_eta_onto_the_wire_event() {
        let event = stamp(
            SelectionUpdate::Dwell {
                target: "key_e".into(),
                progress: 0.5,
                eta: Duration::from_millis(400),
            },
            Duration::from_micros(123),
        );
        match event {
            SelectionEvent::Dwell {
                progress, eta_ms, ..
            } => {
                assert!((progress.get() - 0.5).abs() < 1e-9);
                assert_eq!(eta_ms, 400);
            }
            other => panic!("expected a dwell event, got {other:?}"),
        }
    }
}
