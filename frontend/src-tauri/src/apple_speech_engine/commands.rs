use crate::apple_speech_engine::AppleSpeechEngine;
use std::sync::{Arc, Mutex};
use tauri::{command, AppHandle, Manager, Runtime};

pub const DEFAULT_APPLE_SPEECH_LOCALE: &str = "en-US";

// Global Apple Speech engine (mirrors PARAKEET_ENGINE / WHISPER_ENGINE)
pub static APPLE_SPEECH_ENGINE: Mutex<Option<Arc<AppleSpeechEngine>>> = Mutex::new(None);

#[command]
pub async fn apple_speech_init() -> Result<(), String> {
    let mut guard = APPLE_SPEECH_ENGINE.lock().unwrap();
    if guard.is_some() {
        return Ok(());
    }

    let engine =
        AppleSpeechEngine::new().map_err(|e| format!("Failed to initialize Apple Speech engine: {}", e))?;
    *guard = Some(Arc::new(engine));
    Ok(())
}

/// Whether this machine can run Apple's on-device SpeechAnalyzer at all
/// (macOS 26+). Frontend uses this to decide whether to offer the option.
#[command]
pub async fn apple_speech_is_available() -> bool {
    AppleSpeechEngine::is_available()
}

#[command]
pub async fn apple_speech_get_current_model() -> Result<Option<String>, String> {
    let engine = {
        let guard = APPLE_SPEECH_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    match engine {
        Some(engine) => Ok(engine.get_current_model().await),
        None => Err("Apple Speech engine not initialized".to_string()),
    }
}

#[command]
pub async fn apple_speech_is_model_loaded() -> Result<bool, String> {
    let engine = {
        let guard = APPLE_SPEECH_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    match engine {
        Some(engine) => Ok(engine.is_model_loaded().await),
        None => Err("Apple Speech engine not initialized".to_string()),
    }
}

#[command]
pub async fn apple_speech_transcribe_audio(audio_data: Vec<f32>) -> Result<String, String> {
    let engine = {
        let guard = APPLE_SPEECH_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    match engine {
        Some(engine) => engine
            .transcribe_audio(audio_data)
            .await
            .map_err(|e| format!("Apple Speech transcription failed: {}", e)),
        None => Err("Apple Speech engine not initialized".to_string()),
    }
}

/// Ensure the locale asset from the user's saved transcript config (falling
/// back to en-US) is reserved and ready. Returns the resolved locale
/// identifier actually in use, same shape as the Whisper/Parakeet
/// `*_validate_model_ready_with_config` commands.
pub async fn apple_speech_validate_model_ready_with_config<R: Runtime>(
    app: &AppHandle<R>,
) -> Result<String, String> {
    let engine = {
        let guard = APPLE_SPEECH_ENGINE.lock().unwrap();
        guard.as_ref().cloned()
    };

    let engine = engine.ok_or_else(|| "Apple Speech engine not initialized".to_string())?;

    if engine.is_model_loaded().await {
        if let Some(current) = engine.get_current_model().await {
            log::info!("Apple Speech locale already reserved: {}", current);
            return Ok(current);
        }
    }

    let requested_locale = match crate::api::api::api_get_transcript_config(app.clone(), app.state(), None).await {
        Ok(Some(config)) if config.provider == "appleSpeech" && !config.model.is_empty() => {
            log::info!("Using user's configured Apple Speech locale: {}", config.model);
            config.model
        }
        _ => {
            log::info!(
                "No configured Apple Speech locale, defaulting to {}",
                DEFAULT_APPLE_SPEECH_LOCALE
            );
            DEFAULT_APPLE_SPEECH_LOCALE.to_string()
        }
    };

    engine
        .ensure_ready(Some(requested_locale))
        .await
        .map_err(|e| format!("Failed to prepare Apple Speech locale: {}", e))
}
