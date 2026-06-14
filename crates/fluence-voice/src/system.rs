// SPDX-License-Identifier: Apache-2.0

//! OS fallback voice — « une voix, toujours » (SPEC §2.C, [`VoiceKind::System`]).
//!
//! A non-neural, always-available voice so that *vocalising* never depends on
//! the neural TTS (Piper) being up — the voice counterpart of « le clavier
//! parle toujours » (D-2.6). Windows uses SAPI (`System.Speech` via
//! `PowerShell`); Linux uses `espeak-ng`. Both write a WAV to a temp file we
//! read back. The
//! text is passed on **stdin**, never the command line (it is P0 — §9.A).

use fluence_protocol::api::voice::{ProsodyTag, VoiceInfo, VoiceKind};

use crate::{Utterance, VoiceBackend, VoiceError};

/// Stable id of the OS fallback voice.
pub const SYSTEM_VOICE_ID: &str = "system:default";

/// The operating-system text-to-speech, used as the last-resort voice.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemVoiceBackend;

impl SystemVoiceBackend {
    /// A handle to the OS voice (cheap; availability is probed lazily).
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Whether the platform's OS TTS can be invoked on this machine.
    #[must_use]
    pub fn available() -> bool {
        os::available()
    }
}

impl VoiceBackend for SystemVoiceBackend {
    fn voices(&self) -> Vec<VoiceInfo> {
        if Self::available() {
            vec![VoiceInfo {
                id: SYSTEM_VOICE_ID.into(),
                name: "Voix du système".to_owned(),
                kind: VoiceKind::System,
                language: "fr-FR".to_owned(),
            }]
        } else {
            Vec::new()
        }
    }

    fn synthesize(
        &self,
        text: &str,
        _voice_id: &str,
        _prosody: Option<ProsodyTag>,
    ) -> Result<Utterance, VoiceError> {
        // The OS engines emit a complete WAV, so it is returned as-is.
        let wav = os::synthesize(text)?;
        Ok(Utterance {
            wav,
            voice_id: SYSTEM_VOICE_ID.to_owned(),
        })
    }
}

#[cfg(any(windows, unix))]
mod os {
    use std::io::Write;
    use std::process::{Command, Stdio};

    use crate::VoiceError;

    #[cfg(windows)]
    pub fn available() -> bool {
        true // PowerShell + System.Speech ship with Windows.
    }

    #[cfg(windows)]
    pub fn synthesize(text: &str) -> Result<Vec<u8>, VoiceError> {
        // A temp *directory* with a path inside — not a `NamedTempFile`, whose
        // still-open handle makes SAPI's `SetOutputToWaveFile` write nothing on
        // Windows (a silent 0-byte WAV). The external process owns the file.
        let dir =
            tempfile::tempdir().map_err(|err| VoiceError::Synthesis(format!("temp dir: {err}")))?;
        let path = dir.path().join("voice.wav");
        // SetOutputToWaveFile takes a path (not P0); the text arrives on stdin.
        let script = format!(
            "Add-Type -AssemblyName System.Speech; \
             $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
             $s.SetOutputToWaveFile('{}'); \
             $s.Speak([Console]::In.ReadToEnd()); \
             $s.Dispose()",
            path.display()
        );
        let mut command = std::process::Command::new("powershell");
        command
            .arg("-NoProfile")
            .arg("-NonInteractive")
            .arg("-Command")
            .arg(script);
        run_os(command, text)?;
        std::fs::read(&path).map_err(|err| VoiceError::Synthesis(format!("read OS wav: {err}")))
    }

    #[cfg(unix)]
    pub fn available() -> bool {
        std::process::Command::new("espeak-ng")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[cfg(unix)]
    pub fn synthesize(text: &str) -> Result<Vec<u8>, VoiceError> {
        // A temp *directory* + an inner path (not a `NamedTempFile`): uniform
        // with the Windows path, and the external writer owns the file.
        let dir =
            tempfile::tempdir().map_err(|err| VoiceError::Synthesis(format!("temp dir: {err}")))?;
        let path = dir.path().join("voice.wav");
        let mut command = std::process::Command::new("espeak-ng");
        command
            .arg("-v")
            .arg("fr")
            .arg("--stdin")
            .arg("-w")
            .arg(&path);
        run_os(command, text)?;
        std::fs::read(&path).map_err(|err| VoiceError::Synthesis(format!("read OS wav: {err}")))
    }

    /// Runs the OS command with `text` on stdin and waits for success.
    fn run_os(mut command: Command, text: &str) -> Result<(), VoiceError> {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| VoiceError::Unavailable(format!("OS voice unavailable: {err}")))?;
        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| VoiceError::Synthesis("OS voice stdin unavailable".to_owned()))?;
            stdin
                .write_all(text.as_bytes())
                .map_err(|err| VoiceError::Synthesis(format!("write to OS voice: {err}")))?;
        }
        let status = child
            .wait()
            .map_err(|err| VoiceError::Synthesis(format!("await OS voice: {err}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(VoiceError::Synthesis(format!(
                "OS voice exited with {status}"
            )))
        }
    }
}

#[cfg(not(any(windows, unix)))]
mod os {
    use crate::VoiceError;

    pub fn available() -> bool {
        false
    }

    pub fn synthesize(_text: &str) -> Result<Vec<u8>, VoiceError> {
        Err(VoiceError::Unavailable(
            "no OS voice on this platform".to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voices_reflect_availability() {
        let backend = SystemVoiceBackend::new();
        let voices = backend.voices();
        if SystemVoiceBackend::available() {
            assert_eq!(voices.len(), 1);
            assert_eq!(voices[0].kind, VoiceKind::System);
        } else {
            assert!(voices.is_empty());
        }
    }

    /// On a host whose OS voice is available, synthesis must yield a real,
    /// non-empty WAV — never a 200-with-0-bytes "silent voice" (« une voix,
    /// toujours », SPEC §2.C). Skipped where no OS voice exists (e.g. a headless
    /// Linux runner without `espeak-ng`), so it asserts only where it can.
    #[test]
    fn an_available_os_voice_emits_a_nonempty_wav() {
        if !SystemVoiceBackend::available() {
            return;
        }
        let wav = SystemVoiceBackend::new()
            .synthesize("bonjour", SYSTEM_VOICE_ID, None)
            .expect("an available OS voice must synthesize")
            .wav;
        assert!(
            wav.len() > 44,
            "OS voice produced {} bytes — expected a WAV past the 44-byte header",
            wav.len()
        );
        assert_eq!(
            &wav[0..4],
            b"RIFF",
            "OS voice output is not a RIFF/WAVE file"
        );
    }
}
