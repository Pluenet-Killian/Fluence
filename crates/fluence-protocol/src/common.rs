// SPDX-License-Identifier: Apache-2.0

//! Primitive types shared across the protocol: validated scalars and typed
//! identifiers.
//!
//! Invariants live in the types themselves (SPEC §4.A): a [`Normalized`]
//! cannot hold `1.2`, deserialization rejects it — so every layer above can
//! trust the data by construction.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A float in `[0.0, 1.0]` — coordinates, confidences, probabilities,
/// dwell progress (SPEC §4.A).
///
/// Construction and deserialization validate the bounds and reject
/// non-finite values, so a `Normalized` is always a trustworthy number.
///
/// ```
/// use fluence_protocol::Normalized;
/// assert!(Normalized::new(0.5).is_ok());
/// assert!(Normalized::new(1.2).is_err());
/// assert!(Normalized::new(f64::NAN).is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Normalized(#[schemars(range(min = 0.0, max = 1.0))] f64);

impl Normalized {
    /// Validates that `value` is finite and within `[0.0, 1.0]`.
    ///
    /// # Errors
    ///
    /// Returns [`OutOfRange`] when the value is non-finite or out of bounds.
    pub fn new(value: f64) -> Result<Self, OutOfRange> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(OutOfRange { value })
        }
    }

    /// Returns the inner value (guaranteed finite, in `[0.0, 1.0]`).
    #[must_use]
    pub fn get(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for Normalized {
    type Error = OutOfRange;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for Normalized {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = f64::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Error returned when a value does not fit `[0.0, 1.0]`.
#[derive(Debug, Clone, PartialEq)]
pub struct OutOfRange {
    /// The rejected value.
    pub value: f64,
}

impl fmt::Display for OutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "value {} is outside [0.0, 1.0]", self.value)
    }
}

impl std::error::Error for OutOfRange {}

/// Microseconds since an arbitrary epoch, monotonic **per source**
/// (SPEC §4.A).
///
/// Monotonicity is a stream property the hub enforces at ingestion; the type
/// itself only carries the value.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct TimestampMicros(pub u64);

/// Declares a typed string identifier: same wire format (plain JSON string),
/// distinct Rust types so identifiers cannot be mixed up.
macro_rules! string_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// Wraps a raw string as this identifier type.
            #[must_use]
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
    };
}

string_id! {
    /// Input source, formatted `kind:instance` by convention
    /// (`gaze:webcam0`, `switch:ble0` — SPEC §4.A). The format is a
    /// convention, not a parsing contract.
    SourceId
}
string_id! {
    /// A selectable target declared by a UI (SPEC §4.A).
    TargetId
}
string_id! {
    /// A target surface (screen/window) declared by a UI (SPEC §4.A).
    SurfaceId
}
string_id! {
    /// A conversation session with a warm KV-cache hub-side (SPEC §5.A).
    SessionId
}
string_id! {
    /// Cancellation slot for `/suggest`: a new request on the same slot
    /// aborts the previous one (SPEC §5.A).
    SlotId
}
string_id! {
    /// A paired device (SPEC §2.A).
    DeviceId
}
string_id! {
    /// A user profile (style, keyboards, modalities, voice — SPEC §5.A).
    ProfileId
}
string_id! {
    /// A personal memory item (SPEC §5.B).
    MemoryItemId
}
string_id! {
    /// An installed voice (SPEC §6).
    VoiceId
}
string_id! {
    /// A model from the registry (SPEC D-3.2).
    ModelId
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // 0.0 and 1.0 are exactly representable; the test asserts identity of
    // construction, not arithmetic proximity.
    #[allow(clippy::float_cmp)]
    fn normalized_accepts_bounds() {
        assert_eq!(Normalized::new(0.0).unwrap().get(), 0.0);
        assert_eq!(Normalized::new(1.0).unwrap().get(), 1.0);
    }

    #[test]
    fn normalized_rejects_out_of_range_and_non_finite() {
        for bad in [-0.001, 1.001, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(Normalized::new(bad).is_err(), "{bad} must be rejected");
        }
    }

    #[test]
    fn normalized_deserialization_rejects_out_of_bounds() {
        // SPEC §4.A invariant, PLAN T2: x = 1.2 must not deserialize.
        let err = serde_json::from_str::<Normalized>("1.2").unwrap_err();
        assert!(err.to_string().contains("outside"));
        assert!(serde_json::from_str::<Normalized>("0.86").is_ok());
    }

    #[test]
    fn ids_serialize_as_plain_strings() {
        let id = SourceId::new("gaze:webcam0");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"gaze:webcam0\"");
        assert_eq!(id.to_string(), "gaze:webcam0");
    }
}
