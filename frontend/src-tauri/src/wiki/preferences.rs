use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WikiPreferences {
    pub enabled: bool,
    pub wiki_folder: PathBuf,
}

impl Default for WikiPreferences {
    fn default() -> Self {
        Self {
            enabled: false,
            wiki_folder: get_default_wiki_folder(),
        }
    }
}

/// Get the default wiki folder based on platform. Disabled by default, so this
/// only matters once the user turns the feature on without picking a folder first.
pub fn get_default_wiki_folder() -> PathBuf {
    dirs::document_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Meetily-Wiki")
}

/// Ensure the wiki directory exists
pub fn ensure_wiki_directory(path: &PathBuf) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
        info!("Created wiki directory: {:?}", path);
    }
    Ok(())
}

/// Load wiki preferences from store
pub async fn load_wiki_preferences<R: Runtime>(app: &AppHandle<R>) -> Result<WikiPreferences> {
    let store = match app.store("wiki_preferences.json") {
        Ok(store) => store,
        Err(e) => {
            warn!("Failed to access wiki preferences store: {}, using defaults", e);
            return Ok(WikiPreferences::default());
        }
    };

    let prefs = if let Some(value) = store.get("preferences") {
        match serde_json::from_value::<WikiPreferences>(value.clone()) {
            Ok(p) => {
                info!("Loaded wiki preferences from store");
                p
            }
            Err(e) => {
                warn!("Failed to deserialize wiki preferences: {}, using defaults", e);
                WikiPreferences::default()
            }
        }
    } else {
        info!("No stored wiki preferences found, using defaults");
        WikiPreferences::default()
    };

    info!(
        "Loaded wiki preferences: enabled={}, wiki_folder={:?}",
        prefs.enabled, prefs.wiki_folder
    );
    Ok(prefs)
}

/// Save wiki preferences to store
pub async fn save_wiki_preferences<R: Runtime>(
    app: &AppHandle<R>,
    preferences: &WikiPreferences,
) -> Result<()> {
    info!(
        "Saving wiki preferences: enabled={}, wiki_folder={:?}",
        preferences.enabled, preferences.wiki_folder
    );

    let store = app
        .store("wiki_preferences.json")
        .map_err(|e| anyhow::anyhow!("Failed to access wiki preferences store: {}", e))?;

    let prefs_value = serde_json::to_value(preferences)
        .map_err(|e| anyhow::anyhow!("Failed to serialize wiki preferences: {}", e))?;

    store.set("preferences", prefs_value);
    store
        .save()
        .map_err(|e| anyhow::anyhow!("Failed to save wiki preferences store to disk: {}", e))?;

    if preferences.enabled {
        ensure_wiki_directory(&preferences.wiki_folder)?;
    }

    Ok(())
}
