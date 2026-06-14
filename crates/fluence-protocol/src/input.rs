// SPDX-License-Identifier: Apache-2.0

//! `FluenceInput` v1 — the input protocol (SPEC §4.A, D-4.1).
//!
//! Three stages: **sources** (drivers) publish normalized samples; the
//! **selection engine** (hub-side: fusion, filtering, hit-testing,
//! dwell/scan) decides; **selection events** reach the UIs. UIs declare
//! their selectable targets (`PUT /input/targets` + incremental WebSocket
//! patches); all decision logic stays in the hub so the gaze→target→language
//! loop is replayable and UI-independent.
//!
//! Wire format: JSON messages tagged by a `k` field on the `input` WebSocket
//! topic (see [`crate::ws`] for the envelope and version negotiation), plus
//! the documented `FluenceInput-UDP` mirror for third-party trackers.
//!
//! Stability: **stable** (A1 core, PLAN task 1.3bis).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::common::{Normalized, SourceId, SurfaceId, TargetId, TimestampMicros};

/// One pointing sample from a source, at the sensor's native rate
/// (30–120 Hz). Coordinates are normalized to the surface (SPEC §4.A).
///
/// Wire example (SPEC §4.A):
/// `{"k":"ptr","t":123456789,"src":"gaze:webcam0","x":0.41,"y":0.77,"conf":0.86,"pose":{...}}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PointerSample {
    /// Source timestamp, microseconds, monotonic per source.
    pub t: TimestampMicros,
    /// Producing source (`gaze:webcam0`, `head:opentrack0`…).
    pub src: SourceId,
    /// Horizontal position in `[0, 1]`, relative to the surface.
    pub x: Normalized,
    /// Vertical position in `[0, 1]`, relative to the surface.
    pub y: Normalized,
    /// Source confidence in `[0, 1]` (drives fusion weighting, SPEC §4.C).
    pub conf: Normalized,
    /// Head pose, when the source provides one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose: Option<HeadPose>,
}

/// Head pose in degrees. Free-range (sensor-dependent); fusion treats it as
/// a relative offset signal (« regard désigne, tête affine », SPEC §4.C).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HeadPose {
    /// Rotation around the vertical axis, degrees.
    pub yaw: f64,
    /// Rotation around the lateral axis, degrees.
    pub pitch: f64,
    /// Rotation around the frontal axis, degrees.
    pub roll: f64,
}

/// A physical switch transition (SPEC §4.A).
///
/// Wire example: `{"k":"sw","t":123456789,"src":"switch:ble0","btn":1,"state":"down"}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SwitchEvent {
    /// Source timestamp, microseconds, monotonic per source.
    pub t: TimestampMicros,
    /// Producing source (`switch:ble0`, `switch:usb1`…).
    pub src: SourceId,
    /// Button index on the device, starting at 1.
    pub btn: u8,
    /// Transition direction.
    pub state: SwitchState,
}

/// Switch transition direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SwitchState {
    /// Pressed.
    Down,
    /// Released.
    Up,
}

/// Full declaration of a UI's selectable targets for one surface
/// (`PUT /input/targets`, SPEC §4.A).
///
/// Wire example: `{"surface":"main","viewport":{"w":1920,"h":1080},"targets":[…]}`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TargetMap {
    /// Surface these targets belong to.
    pub surface: SurfaceId,
    /// Pixel size of the surface viewport (target rects are expressed in it).
    pub viewport: Viewport,
    /// Complete list — replaces any previous declaration for this surface.
    pub targets: Vec<Target>,
}

/// Incremental update of a surface's targets (WebSocket `input` topic).
///
/// Application order: `upsert` (insert or replace by id), then `remove`.
/// A `viewport` change invalidates nothing else — rects of untouched targets
/// are assumed already expressed in the new viewport.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TargetMapPatch {
    /// Surface to patch.
    pub surface: SurfaceId,
    /// New viewport size, if it changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<Viewport>,
    /// Targets to insert or replace (matched by `id`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upsert: Vec<Target>,
    /// Target ids to remove.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove: Vec<TargetId>,
}

/// Viewport size in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Viewport {
    /// Width in pixels.
    pub w: u32,
    /// Height in pixels.
    pub h: u32,
}

/// One selectable target (SPEC §4.A).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Target {
    /// Unique id within the surface (`key_e`, `sug_1`…).
    pub id: TargetId,
    /// Bounding box in viewport pixels.
    pub rect: Rect,
    /// Semantic role — drives dwell/magnetism policies.
    pub role: TargetRole,
    /// Human-readable label (the letter of a key, the text of a suggestion).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Linguistic prior in `[0, 1]` modulating adaptive dwell and capped
    /// magnetism (SPEC §4.C — never more than 40% of inter-target distance).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior: Option<Normalized>,
}

/// Semantic role of a target.
///
/// Unknown roles deserialize as [`TargetRole::Unknown`] so older hubs
/// tolerate newer UIs (forward compatibility).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TargetRole {
    /// A keyboard key.
    Key,
    /// One of the (max 3, fixed-position) suggestion slots (SPEC §7.A).
    Suggestion,
    /// Any other actionable control (PARLER, tabs, emergency — SPEC §7.A).
    Action,
    /// Role added by a newer peer; treated with default policies.
    #[serde(other)]
    Unknown,
}

/// Axis-aligned rectangle in viewport pixels, serialized as `[x, y, w, h]`
/// (SPEC §4.A wire format).
///
/// Validation: all components finite, `w ≥ 0`, `h ≥ 0` (`x`/`y` may be
/// negative — a target can hang off-surface while scrolling).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "[f64; 4]", into = "[f64; 4]")]
pub struct Rect {
    /// Left edge, pixels.
    pub x: f64,
    /// Top edge, pixels.
    pub y: f64,
    /// Width, pixels (≥ 0).
    pub w: f64,
    /// Height, pixels (≥ 0).
    pub h: f64,
}

impl TryFrom<[f64; 4]> for Rect {
    type Error = InvalidRect;

    fn try_from([x, y, w, h]: [f64; 4]) -> Result<Self, Self::Error> {
        let all_finite = [x, y, w, h].iter().all(|v| v.is_finite());
        if all_finite && w >= 0.0 && h >= 0.0 {
            Ok(Self { x, y, w, h })
        } else {
            Err(InvalidRect { x, y, w, h })
        }
    }
}

impl From<Rect> for [f64; 4] {
    fn from(rect: Rect) -> Self {
        [rect.x, rect.y, rect.w, rect.h]
    }
}

/// Error for a rectangle with non-finite components or negative size.
#[derive(Debug, Clone, PartialEq)]
pub struct InvalidRect {
    /// Left edge as received.
    pub x: f64,
    /// Top edge as received.
    pub y: f64,
    /// Width as received.
    pub w: f64,
    /// Height as received.
    pub h: f64,
}

impl std::fmt::Display for InvalidRect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid rect [{}, {}, {}, {}]: components must be finite and w/h ≥ 0",
            self.x, self.y, self.w, self.h
        )
    }
}

impl std::error::Error for InvalidRect {}

/// Client → hub messages on the `input` topic: sensor samples from remote
/// clients (e.g. a tablet running `MediaPipe` in-browser, SPEC §4.A) and
/// target patches from UIs. Hub-managed drivers bypass this entirely
/// (in-process direct path).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "k")]
pub enum InputClientMessage {
    /// A pointing sample.
    #[serde(rename = "ptr")]
    Pointer(PointerSample),
    /// A switch transition.
    #[serde(rename = "sw")]
    Switch(SwitchEvent),
    /// An incremental target update.
    #[serde(rename = "targets.patch")]
    TargetsPatch(TargetMapPatch),
    /// One calibration pair (SPEC §4.D): the raw gaze point `(x, y)` observed
    /// while the user looked at `target` on `surface`. The hub pairs it with the
    /// target's centre to fit the gaze→screen mapping. Collected during the
    /// smooth-pursuit / express calibration the client animates.
    #[serde(rename = "cal.sample")]
    CalibrationSample {
        /// Surface being calibrated.
        surface: SurfaceId,
        /// Target the user was looking at (ground truth for this pair).
        target: TargetId,
        /// Raw gaze X in `[0, 1]` (the client's uncalibrated estimate).
        x: Normalized,
        /// Raw gaze Y in `[0, 1]`.
        y: Normalized,
    },
    /// Fit (or refit) the calibration mapping for `surface` from the samples
    /// collected so far (SPEC §4.D). Sent at the end of a calibration sequence.
    #[serde(rename = "cal.fit")]
    CalibrationFit {
        /// Surface to fit.
        surface: SurfaceId,
    },
}

/// Hub → UI selection events on the `input` topic (SPEC §4.A).
///
/// Budget: a committed selection reaches the UI < 20 ms after the decision.
///
/// Marked `non_exhaustive`: event enums grow over time; clients must
/// tolerate (ignore) unknown events rather than fail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "k")]
#[non_exhaustive]
pub enum SelectionEvent {
    /// The selection engine's focus entered a target.
    #[serde(rename = "sel.focus")]
    Focus {
        /// Focused target.
        target: TargetId,
        /// Decision timestamp.
        t: TimestampMicros,
    },
    /// Dwell progress on the focused target (drives the UI gauge).
    #[serde(rename = "sel.dwell")]
    Dwell {
        /// Target being dwelled on.
        target: TargetId,
        /// Progress in `[0, 1]`.
        progress: Normalized,
        /// Estimated remaining time to commit, milliseconds.
        eta_ms: u32,
    },
    /// A selection was committed.
    #[serde(rename = "sel.commit")]
    Commit {
        /// Committed target.
        target: TargetId,
        /// How it was committed.
        method: CommitMethod,
        /// Decision timestamp.
        t: TimestampMicros,
    },
    /// Focus/dwell was cancelled (target left, signal lost…).
    #[serde(rename = "sel.cancel")]
    Cancel,
    /// Scanning highlight moved to a group (SPEC §4.A).
    #[serde(rename = "scan.highlight")]
    ScanHighlight {
        /// Highlighted group, by convention `row:<n>` / `col:<n>` / target id.
        group: String,
    },
}

/// How a selection was committed.
///
/// Unknown methods deserialize as [`CommitMethod::Unknown`] (forward
/// compatibility — clients ignore what they don't know).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CommitMethod {
    /// Dwell timer completed on a fixation.
    Dwell,
    /// Validated by a physical switch.
    Switch,
    /// Selected via scanning.
    Scan,
    /// Method added by a newer hub.
    #[serde(other)]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_rejects_negative_sizes_and_non_finite() {
        assert!(serde_json::from_str::<Rect>("[0, 0, -5, 10]").is_err());
        assert!(serde_json::from_str::<Rect>("[0, 0, 5, 10]").is_ok());
        // x may be negative (off-surface target while scrolling).
        assert!(serde_json::from_str::<Rect>("[-5, 0, 5, 10]").is_ok());
    }

    #[test]
    fn rect_round_trips_as_array() {
        let rect = Rect {
            x: 10.0,
            y: 500.0,
            w: 120.0,
            h: 90.0,
        };
        let json = serde_json::to_string(&rect).unwrap();
        assert_eq!(json, "[10.0,500.0,120.0,90.0]");
        assert_eq!(serde_json::from_str::<Rect>(&json).unwrap(), rect);
    }

    #[test]
    fn unknown_commit_method_is_tolerated() {
        let event: SelectionEvent =
            serde_json::from_str(r#"{"k":"sel.commit","target":"key_e","method":"blink","t":1}"#)
                .unwrap();
        let SelectionEvent::Commit { method, .. } = event else {
            panic!("expected commit");
        };
        assert_eq!(method, CommitMethod::Unknown);
    }

    #[test]
    fn unknown_target_role_is_tolerated() {
        let target: Target =
            serde_json::from_str(r#"{"id":"x","rect":[0,0,1,1],"role":"slider"}"#).unwrap();
        assert_eq!(target.role, TargetRole::Unknown);
    }
}
