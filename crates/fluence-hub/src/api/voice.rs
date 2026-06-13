// SPDX-License-Identifier: Apache-2.0

//! Voice surface: `POST /voice/speak` and `GET /voice/voices` (SPEC §5.A, §6).
//!
//! `speak` synthesizes text to a streamed WAV (`audio/wav`, ADR-0009 — Opus is
//! deferred to home mode). The text is **P0** (§9.A): it lives only inside the
//! synthesis closure and is never logged.

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use fluence_protocol::api::voice::{SpeakRequest, VoicesResponse};
use fluence_protocol::error::ErrorCode;
use fluence_voice::VoiceError;

use crate::api::problem_response;
use crate::state::AppState;

/// `POST /api/v1/voice/speak` (control scope): vocalize text.
///
/// Synthesis is a blocking subprocess, so it runs on a blocking task off the
/// async runtime. The response streams the WAV; `x-fluence-voice` reports which
/// voice actually spoke (it may be the OS fallback — « une voix, toujours »).
pub async fn speak(State(state): State<AppState>, Json(request): Json<SpeakRequest>) -> Response {
    let voice = state.voice().clone();
    let outcome = tokio::task::spawn_blocking(move || {
        // `request.text` (P0) is consumed here and never escapes into a log.
        voice.synthesize(&request.text, &request.voice_id.0, request.prosody)
    })
    .await;

    match outcome {
        Ok(Ok(utterance)) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "audio/wav")
            .header("x-fluence-voice", utterance.voice_id)
            .body(Body::from(utterance.wav))
            .unwrap_or_else(|_| problem_response(ErrorCode::Internal, None)),
        Ok(Err(error @ VoiceError::Unavailable(_))) => {
            // No voice could serve (not even the OS fallback). The error never
            // carries P0 (it names the backend, not the text).
            tracing::warn!(%error, "voice unavailable");
            problem_response(ErrorCode::WorkerUnavailable, None)
        }
        Ok(Err(error @ VoiceError::Synthesis(_))) => {
            tracing::error!(%error, "voice synthesis failed");
            problem_response(ErrorCode::Internal, None)
        }
        Err(error) => {
            tracing::error!(%error, "voice synthesis task panicked");
            problem_response(ErrorCode::Internal, None)
        }
    }
}

/// `GET /api/v1/voice/voices` (control/care scope): the installed voices.
pub async fn voices(State(state): State<AppState>) -> Json<VoicesResponse> {
    Json(VoicesResponse {
        voices: state.voice().voices(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::body::to_bytes;
    use fluence_inference::UnavailableBackend;
    use fluence_ngram::NgramModel;
    use fluence_protocol::api::voice::{ProsodyTag, VoiceInfo, VoiceKind};
    use fluence_store::{KeySource, Store, StoreConfig};
    use fluence_voice::{UnavailableVoice, Utterance, VoiceBackend};

    use super::*;
    use crate::config::HubConfig;
    use crate::events::EventBus;

    /// A voice that returns a tiny canned WAV — exercises the hub plumbing
    /// without a real subprocess.
    struct FakeVoice;

    impl VoiceBackend for FakeVoice {
        fn voices(&self) -> Vec<VoiceInfo> {
            vec![VoiceInfo {
                id: "piper:test".into(),
                name: "Test".to_owned(),
                kind: VoiceKind::Piper,
                language: "fr-FR".to_owned(),
            }]
        }

        fn synthesize(
            &self,
            _text: &str,
            _voice_id: &str,
            _prosody: Option<ProsodyTag>,
        ) -> Result<Utterance, VoiceError> {
            Ok(Utterance {
                wav: b"RIFF....WAVE".to_vec(),
                voice_id: "piper:test".to_owned(),
            })
        }
    }

    async fn state_with_voice(dir: &tempfile::TempDir, voice: Arc<dyn VoiceBackend>) -> AppState {
        let store = Store::open(StoreConfig {
            path: dir.path().join("store.db"),
            key: KeySource::File(dir.path().join("store.key")),
        })
        .await
        .expect("store opens");
        AppState::new_with(
            HubConfig::default(),
            store,
            EventBus::new(),
            Arc::new(UnavailableBackend),
            Arc::new(NgramModel::new()),
            voice,
        )
    }

    fn speak_request() -> SpeakRequest {
        SpeakRequest {
            text: "Bonjour".to_owned(),
            voice_id: "piper:test".into(),
            prosody: None,
        }
    }

    #[tokio::test]
    async fn speak_streams_a_wav_with_the_right_content_type() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = state_with_voice(&dir, Arc::new(FakeVoice)).await;
        let response = speak(State(state), Json(speak_request())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("content-type")
                .expect("content-type"),
            "audio/wav"
        );
        let body = to_bytes(response.into_body(), 1 << 20).await.expect("body");
        assert!(body.starts_with(b"RIFF"), "the body is a WAV");
    }

    #[tokio::test]
    async fn speak_without_a_voice_degrades_to_503_not_a_panic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = state_with_voice(&dir, Arc::new(UnavailableVoice)).await;
        let response = speak(State(state), Json(speak_request())).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn voices_lists_the_backend_voices() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = state_with_voice(&dir, Arc::new(FakeVoice)).await;
        let Json(response) = voices(State(state)).await;
        assert_eq!(response.voices.len(), 1);
        assert_eq!(response.voices[0].kind, VoiceKind::Piper);
    }
}
