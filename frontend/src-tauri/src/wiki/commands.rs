use crate::wiki::preferences::{
    ensure_wiki_directory, get_default_wiki_folder, load_wiki_preferences,
    save_wiki_preferences, WikiPreferences,
};
use log::info;
use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub async fn get_wiki_preferences<R: Runtime>(
    app: AppHandle<R>,
) -> Result<WikiPreferences, String> {
    load_wiki_preferences(&app)
        .await
        .map_err(|e| format!("Failed to load wiki preferences: {}", e))
}

#[tauri::command]
pub async fn set_wiki_preferences<R: Runtime>(
    app: AppHandle<R>,
    preferences: WikiPreferences,
) -> Result<(), String> {
    save_wiki_preferences(&app, &preferences)
        .await
        .map_err(|e| format!("Failed to save wiki preferences: {}", e))
}

#[tauri::command]
pub async fn get_default_wiki_folder_path() -> Result<String, String> {
    Ok(get_default_wiki_folder().to_string_lossy().to_string())
}

#[tauri::command]
pub async fn open_wiki_folder<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let preferences = load_wiki_preferences(&app)
        .await
        .map_err(|e| format!("Failed to load preferences: {}", e))?;

    ensure_wiki_directory(&preferences.wiki_folder)
        .map_err(|e| format!("Failed to create directory: {}", e))?;

    let folder_path = preferences.wiki_folder.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&folder_path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    info!("Opened wiki folder: {}", folder_path);
    Ok(())
}

#[tauri::command]
pub async fn select_wiki_folder<R: Runtime>(app: AppHandle<R>) -> Result<Option<String>, String> {
    let app_clone = app.clone();
    let folder_path = tokio::task::spawn_blocking(move || app_clone.dialog().file().blocking_pick_folder())
        .await
        .map_err(|e| format!("Folder dialog task failed: {}", e))?;

    Ok(folder_path.map(|p| p.to_string()))
}
