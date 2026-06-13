// SPDX-License-Identifier: Apache-2.0

//! T2 — property tests: serde round-trips for arbitrary valid values, and
//! rejection of out-of-bounds input (PLAN §1, Phase 1 tests).

use proptest::prelude::*;

use fluence_protocol::api::pair::Scope;
use fluence_protocol::api::suggest::{
    AbortReason, SuggestAborted, SuggestConstraints, SuggestEvent, SuggestMode, SuggestRequest,
};
use fluence_protocol::error::ErrorCode;
use fluence_protocol::input::{
    CommitMethod, HeadPose, PointerSample, Rect, SelectionEvent, SwitchEvent, SwitchState,
};
use fluence_protocol::{Normalized, TimestampMicros};

fn normalized() -> impl Strategy<Value = Normalized> {
    (0.0..=1.0f64).prop_map(|v| Normalized::new(v).expect("strategy stays in range"))
}

fn pointer_sample() -> impl Strategy<Value = PointerSample> {
    (
        any::<u64>(),
        "[a-z]{1,8}:[a-z0-9]{1,8}",
        normalized(),
        normalized(),
        normalized(),
        proptest::option::of((-180.0..180.0f64, -90.0..90.0f64, -180.0..180.0f64)),
    )
        .prop_map(|(t, src, x, y, conf, pose)| PointerSample {
            t: TimestampMicros(t),
            src: src.as_str().into(),
            x,
            y,
            conf,
            pose: pose.map(|(yaw, pitch, roll)| HeadPose { yaw, pitch, roll }),
        })
}

fn switch_event() -> impl Strategy<Value = SwitchEvent> {
    (
        any::<u64>(),
        "[a-z]{1,8}:[a-z0-9]{1,8}",
        any::<u8>(),
        prop_oneof![Just(SwitchState::Down), Just(SwitchState::Up)],
    )
        .prop_map(|(t, src, btn, state)| SwitchEvent {
            t: TimestampMicros(t),
            src: src.as_str().into(),
            btn,
            state,
        })
}

fn selection_event() -> impl Strategy<Value = SelectionEvent> {
    prop_oneof![
        (any::<u64>(), "[a-z_0-9]{1,12}").prop_map(|(t, target)| SelectionEvent::Focus {
            target: target.as_str().into(),
            t: TimestampMicros(t),
        }),
        ("[a-z_0-9]{1,12}", normalized(), any::<u32>()).prop_map(|(target, progress, eta_ms)| {
            SelectionEvent::Dwell {
                target: target.as_str().into(),
                progress,
                eta_ms,
            }
        }),
        (
            any::<u64>(),
            "[a-z_0-9]{1,12}",
            prop_oneof![
                Just(CommitMethod::Dwell),
                Just(CommitMethod::Switch),
                Just(CommitMethod::Scan)
            ]
        )
            .prop_map(|(t, target, method)| SelectionEvent::Commit {
                target: target.as_str().into(),
                method,
                t: TimestampMicros(t),
            }),
        Just(SelectionEvent::Cancel),
        "[a-z]{1,5}:[0-9]{1,3}".prop_map(|group| SelectionEvent::ScanHighlight { group }),
    ]
}

fn suggest_request() -> impl Strategy<Value = SuggestRequest> {
    (
        prop_oneof![
            Just(SuggestMode::Replies),
            Just(SuggestMode::Rephrase),
            Just(SuggestMode::Expand),
            Just(SuggestMode::Continue)
        ],
        ".{0,80}",
        1..=5u8,
        "[a-z]{1,8}",
        proptest::option::of((
            proptest::option::of(1..=500u32),
            proptest::option::of(".{1,16}"),
        )),
    )
        .prop_map(|(mode, draft, n, slot, constraints)| SuggestRequest {
            mode,
            draft,
            n,
            slot: slot.as_str().into(),
            constraints: constraints.map(|(max_chars, register)| SuggestConstraints {
                max_chars,
                register,
            }),
            style_profile: None,
        })
}

proptest! {
    /// Round-trip: any valid message survives serialize→deserialize
    /// bit-identically (PLAN T2).
    #[test]
    fn pointer_sample_roundtrips(sample in pointer_sample()) {
        let json = serde_json::to_string(&sample).unwrap();
        let back: PointerSample = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(sample, back);
    }

    #[test]
    fn switch_event_roundtrips(event in switch_event()) {
        let json = serde_json::to_string(&event).unwrap();
        let back: SwitchEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(event, back);
    }

    #[test]
    fn selection_event_roundtrips(event in selection_event()) {
        let json = serde_json::to_string(&event).unwrap();
        let back: SelectionEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(event, back);
    }

    #[test]
    fn suggest_request_roundtrips(request in suggest_request()) {
        let json = serde_json::to_string(&request).unwrap();
        let back: SuggestRequest = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(request, back);
    }

    #[test]
    fn suggest_abort_roundtrips(reason in prop_oneof![
        Just(AbortReason::Superseded),
        Just(AbortReason::SessionClosed),
        Just(AbortReason::Shutdown),
    ]) {
        let event = SuggestEvent::Aborted(SuggestAborted { reason });
        let json = serde_json::to_string(&event).unwrap();
        let back: SuggestEvent = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(event, back);
    }

    /// Out-of-bounds coordinates never deserialize (SPEC §4.A invariant:
    /// `x = 1.2` is the PLAN's canonical example).
    #[test]
    fn out_of_range_normalized_is_rejected(value in prop_oneof![
        (1.0f64..1e12).prop_map(|v| v + f64::EPSILON),
        (-1e12..0.0f64).prop_map(|v| v - f64::EPSILON),
    ]) {
        let json = serde_json::to_string(&value).unwrap();
        prop_assert!(serde_json::from_str::<Normalized>(&json).is_err());
    }

    /// A pointer sample with one out-of-range coordinate is rejected as a
    /// whole — invalid data cannot enter through composition.
    #[test]
    fn sample_with_bad_coordinate_is_rejected(bad_x in 1.0001f64..1e6) {
        let json = format!(
            r#"{{"t":1,"src":"gaze:webcam0","x":{bad_x},"y":0.5,"conf":0.5}}"#
        );
        prop_assert!(serde_json::from_str::<PointerSample>(&json).is_err());
    }

    /// Unknown scopes fail closed (PLAN T2: "scope inconnu" must be
    /// rejected — scopes gate security).
    #[test]
    fn unknown_scopes_are_rejected(name in "[a-z]{1,12}") {
        let known = ["display", "control", "care", "system"];
        prop_assume!(!known.contains(&name.as_str()));
        let json = format!("\"{name}\"");
        prop_assert!(serde_json::from_str::<Scope>(&json).is_err());
    }

    /// Unknown error codes degrade to `Unknown` instead of failing —
    /// clients keep working against newer hubs.
    #[test]
    fn unknown_error_codes_degrade_gracefully(name in "[a-z_]{1,24}") {
        let parsed: ErrorCode = serde_json::from_str(&format!("\"{name}\"")).unwrap();
        let known = serde_json::to_string(&parsed).unwrap() == format!("\"{name}\"");
        prop_assert!(known || parsed == ErrorCode::Unknown);
    }

    /// Rect accepts exactly: finite components, non-negative sizes.
    #[test]
    fn rect_validation_is_exact(x in -1e9..1e9f64, y in -1e9..1e9f64,
                                w in -1e3..1e9f64, h in -1e3..1e9f64) {
        let json = format!("[{x},{y},{w},{h}]");
        let parsed = serde_json::from_str::<Rect>(&json);
        prop_assert_eq!(parsed.is_ok(), w >= 0.0 && h >= 0.0);
    }
}
