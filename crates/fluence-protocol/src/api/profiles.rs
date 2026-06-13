// SPDX-License-Identifier: Apache-2.0

//! User profiles: style, keyboard, modalities, voice (SPEC §5.A, §7.B).
//!
//! The style profile feeds the stable block 1 of the prompt (§5.C) — it is
//! how the system speaks *like the person*.
//!
//! Stability: **experimental** — the profile shape firms up with the
//! composer (Phase 5) and onboarding (Phase 7); fields may still change.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::common::{ProfileId, VoiceId};

/// `GET/PUT /profiles/{id}` — one user profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Profile {
    /// Stable id (`default` for the primary profile).
    pub id: ProfileId,
    /// Display name.
    pub name: String,
    /// Style — express onboarding answers (SPEC §7.B step 4).
    #[serde(default)]
    pub style: StyleProfile,
    /// Keyboard preferences.
    #[serde(default)]
    pub keyboard: KeyboardPrefs,
    /// Preferred voice, if chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice_id: Option<VoiceId>,
}

/// Style profile (SPEC §7.B: tutoiement ? humour ? façon de dire oui/non ?
/// expressions à soi ?). All fields optional: an empty style is valid.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct StyleProfile {
    /// Prefers informal address (« tutoiement ») with close people.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub informal_address: Option<bool>,
    /// How their humor sounds, in their words (« pince-sans-rire »). **P0.**
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub humor: Option<String>,
    /// How they typically say yes/no (« ouais », « pas question »). **P0.**
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yes_no_style: Option<String>,
    /// Signature expressions, injected into the style prompt. **P0.**
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expressions: Vec<String>,
}

/// Keyboard preferences (SPEC §7.A).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct KeyboardPrefs {
    /// Selected layout.
    #[serde(default)]
    pub layout: KeyboardLayout,
    /// Target-size multiplier applied on top of the noise-model sizing
    /// (§4.C); `None` = automatic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_scale: Option<f64>,
}

/// Keyboard layouts v1 (SPEC §7.A).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum KeyboardLayout {
    /// AZERTY adapted — familiarity beats optimality (default).
    Azerty,
    /// French frequency layout (for switch scanning, where order = speed).
    FrequencyFr,
    /// Alphabetical grid.
    Abc,
}

impl Default for KeyboardLayout {
    fn default() -> Self {
        Self::Azerty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_profile_uses_defaults() {
        let profile: Profile = serde_json::from_str(r#"{"id":"default","name":"Claire"}"#).unwrap();
        assert_eq!(profile.keyboard.layout, KeyboardLayout::Azerty);
        assert!(profile.style.expressions.is_empty());
        assert!(profile.voice_id.is_none());
    }
}
