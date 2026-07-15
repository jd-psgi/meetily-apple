// audio/transcription/apple_speech_provider.rs
//
// Apple Speech (SpeechAnalyzer/SpeechTranscriber, macOS 26+) transcription
// provider implementation.

use super::provider::{TranscriptionError, TranscriptionProvider, TranscriptResult};
use async_trait::async_trait;
use std::sync::Arc;

/// Apple Speech transcription provider (wraps AppleSpeechEngine)
pub struct AppleSpeechProvider {
    engine: Arc<crate::apple_speech_engine::AppleSpeechEngine>,
}

impl AppleSpeechProvider {
    pub fn new(engine: Arc<crate::apple_speech_engine::AppleSpeechEngine>) -> Self {
        Self { engine }
    }
}

#[async_trait]
impl TranscriptionProvider for AppleSpeechProvider {
    async fn transcribe(
        &self,
        audio: Vec<f32>,
        language: Option<String>,
    ) -> std::result::Result<TranscriptResult, TranscriptionError> {
        if let Some(ref lang) = language {
            log::warn!(
                "Apple Speech uses the locale reserved at startup ('{}' requested); per-call language overrides aren't supported yet",
                lang
            );
        }

        match self.engine.transcribe_audio(audio).await {
            Ok(text) => Ok(TranscriptResult {
                text: text.trim().to_string(),
                confidence: None, // SpeechAnalyzer doesn't expose confidence scores
                is_partial: false, // Each call returns only the finalized result
            }),
            Err(e) => Err(TranscriptionError::EngineFailed(e.to_string())),
        }
    }

    async fn is_model_loaded(&self) -> bool {
        self.engine.is_model_loaded().await
    }

    async fn get_current_model(&self) -> Option<String> {
        self.engine.get_current_model().await
    }

    fn provider_name(&self) -> &'static str {
        "Apple Speech"
    }
}
