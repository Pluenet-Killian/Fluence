// SPDX-License-Identifier: Apache-2.0

//! Piper TTS backend — the everyday voice (SPEC §6, D-6.1).
//!
//! Realised as a **subprocess** (the official `piper` binary), like the LLM
//! backend (ADR-0007): no C++/CMake in our build, crash-isolated by the process
//! boundary, portable Windows/Linux. Each `synthesize` spawns `piper`, writes
//! the text to its stdin, and reads the raw 16-bit mono PCM from its stdout
//! (`--output_raw`), which [`crate::wav`] wraps into a WAV.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use fluence_protocol::api::voice::{ProsodyTag, VoiceInfo, VoiceKind};

use crate::wav::wav_from_pcm;
use crate::{Utterance, VoiceBackend, VoiceError};

/// Sample rate used when the voice config omits it (Piper medium = 22050 Hz).
const DEFAULT_SAMPLE_RATE: u32 = 22050;

/// A Piper voice served by the `piper` binary and one ONNX model.
#[derive(Debug, Clone)]
pub struct PiperBackend {
    exe: PathBuf,
    model: PathBuf,
    voice_id: String,
    name: String,
    language: String,
    sample_rate: u32,
}

impl PiperBackend {
    /// Builds a backend from the `piper` executable and an ONNX voice model.
    ///
    /// Reads the model's `<model>.onnx.json` sidecar for the sample rate, the
    /// language tag and a display name; missing fields fall back to sane
    /// defaults rather than failing.
    ///
    /// # Errors
    ///
    /// [`VoiceError::Unavailable`] if the binary or the model file is absent.
    pub fn new(
        exe: impl Into<PathBuf>,
        model: impl Into<PathBuf>,
        voice_id: impl Into<String>,
    ) -> Result<Self, VoiceError> {
        let exe = exe.into();
        let model = model.into();
        if !exe.exists() {
            return Err(VoiceError::Unavailable(format!(
                "piper binary not found: {}",
                exe.display()
            )));
        }
        if !model.exists() {
            return Err(VoiceError::Unavailable(format!(
                "voice model not found: {}",
                model.display()
            )));
        }
        let voice_id = voice_id.into();
        let config = VoiceConfig::read(&model);
        Ok(Self {
            name: config.name.unwrap_or_else(|| voice_id.clone()),
            language: config.language,
            sample_rate: config.sample_rate,
            voice_id,
            exe,
            model,
        })
    }
}

impl VoiceBackend for PiperBackend {
    fn voices(&self) -> Vec<VoiceInfo> {
        vec![VoiceInfo {
            id: self.voice_id.clone().into(),
            name: self.name.clone(),
            kind: VoiceKind::Piper,
            language: self.language.clone(),
        }]
    }

    fn synthesize(
        &self,
        text: &str,
        _voice_id: &str,
        prosody: Option<ProsodyTag>,
    ) -> Result<Utterance, VoiceError> {
        let mut child = Command::new(&self.exe)
            .arg("--model")
            .arg(&self.model)
            .arg("--output_raw")
            .arg("--length_scale")
            .arg(length_scale_for(prosody).to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Piper logs progress on stderr; discard it (it can echo the text,
            // which is P0 — §9.A).
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| VoiceError::Synthesis(format!("spawn piper: {err}")))?;

        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| VoiceError::Synthesis("piper stdin unavailable".to_owned()))?;
            stdin
                .write_all(text.as_bytes())
                .map_err(|err| VoiceError::Synthesis(format!("write to piper: {err}")))?;
        } // drop stdin → EOF so piper synthesizes and exits

        let output = child
            .wait_with_output()
            .map_err(|err| VoiceError::Synthesis(format!("await piper: {err}")))?;
        if !output.status.success() {
            return Err(VoiceError::Synthesis(format!(
                "piper exited with {}",
                output.status
            )));
        }
        Ok(Utterance {
            wav: wav_from_pcm(&output.stdout, self.sample_rate),
            voice_id: self.voice_id.clone(),
        })
    }
}

/// Maps a prosody tag to Piper's `--length_scale` (rate). Piper v0 realises
/// only the rate-based tags; emotional tags need F5-TTS (Cloned, P2), so they
/// fall back to neutral pacing here (D-6.3).
fn length_scale_for(prosody: Option<ProsodyTag>) -> f64 {
    match prosody {
        Some(ProsodyTag::Slow) => 1.3,
        Some(ProsodyTag::Fast) => 0.85,
        _ => 1.0,
    }
}

/// The few voice-config fields we read from the `.onnx.json` sidecar.
struct VoiceConfig {
    sample_rate: u32,
    language: String,
    name: Option<String>,
}

impl VoiceConfig {
    /// Reads `<model>.json` (Piper's sidecar is `<model>.onnx.json`), tolerating
    /// any read/parse failure with defaults — a missing config must not stop the
    /// voice from speaking.
    fn read(model: &std::path::Path) -> Self {
        let sidecar = PathBuf::from(format!("{}.json", model.display()));
        let parsed = std::fs::read_to_string(&sidecar)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
        let Some(value) = parsed else {
            return Self {
                sample_rate: DEFAULT_SAMPLE_RATE,
                language: "fr-FR".to_owned(),
                name: None,
            };
        };
        let sample_rate = value["audio"]["sample_rate"]
            .as_u64()
            .and_then(|rate| u32::try_from(rate).ok())
            .unwrap_or(DEFAULT_SAMPLE_RATE);
        // Piper writes the locale as `fr_FR`; BCP 47 wants `fr-FR`.
        let language = value["language"]["code"]
            .as_str()
            .map_or_else(|| "fr-FR".to_owned(), |code| code.replace('_', "-"));
        let name = value["dataset"].as_str().map(capitalize);
        Self {
            sample_rate,
            language,
            name,
        }
    }
}

/// Upper-cases the first character (`siwis` → `Siwis`) for a display name.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_scale_maps_rate_tags_only() {
        assert!((length_scale_for(Some(ProsodyTag::Slow)) - 1.3).abs() < 1e-9);
        assert!((length_scale_for(Some(ProsodyTag::Fast)) - 0.85).abs() < 1e-9);
        assert!((length_scale_for(Some(ProsodyTag::Joy)) - 1.0).abs() < 1e-9);
        assert!((length_scale_for(None) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn capitalize_first_letter() {
        assert_eq!(capitalize("siwis"), "Siwis");
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn missing_binary_is_unavailable() {
        let result = PiperBackend::new("/no/such/piper", "/no/such/model.onnx", "piper:x");
        assert!(matches!(result, Err(VoiceError::Unavailable(_))));
    }

    #[test]
    fn voice_config_falls_back_when_absent() {
        let config = VoiceConfig::read(std::path::Path::new("/no/such/model.onnx"));
        assert_eq!(config.sample_rate, DEFAULT_SAMPLE_RATE);
        assert_eq!(config.language, "fr-FR");
        assert!(config.name.is_none());
    }

    /// Live smoke test against a real `piper` binary + voice (like the
    /// llama-server smoke test). Set `FLUENCE_PIPER_BIN` and
    /// `FLUENCE_PIPER_VOICE`, then `cargo test -p fluence-voice -- --ignored`.
    #[test]
    #[ignore = "requires a real piper binary + voice model"]
    fn piper_synthesizes_a_wav_live() {
        let exe = std::env::var("FLUENCE_PIPER_BIN").expect("FLUENCE_PIPER_BIN set");
        let model = std::env::var("FLUENCE_PIPER_VOICE").expect("FLUENCE_PIPER_VOICE set");
        let backend =
            PiperBackend::new(exe, model, "piper:fr_FR-siwis-medium").expect("backend builds");
        let utterance = backend
            .synthesize(
                "Bonjour, ceci est un test de Fluence.",
                "piper:fr_FR-siwis-medium",
                None,
            )
            .expect("synthesis succeeds");
        assert_eq!(&utterance.wav[0..4], b"RIFF");
        assert!(
            utterance.wav.len() > crate::wav::HEADER_LEN,
            "the WAV carries audio data"
        );
    }
}
