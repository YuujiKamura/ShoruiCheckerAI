use std::path::PathBuf;
use std::fs;
use serde::{Serialize, Deserialize};

pub const DEFAULT_MODEL: &str = "gemini-2.5-pro";

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    pub watch_folder: Option<String>,
    pub model: Option<String>,
    pub code_watch_folder: Option<String>,
    pub code_review_enabled: bool,
}

pub fn get_settings_path() -> PathBuf {
    let config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    config_dir.join("shoruichecker").join("settings.json")
}

pub fn load_settings() -> AppSettings {
    let path = get_settings_path();
    if path.exists() {
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        AppSettings::default()
    }
}

pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let path = get_settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_model() -> String {
    load_settings()
        .model
        .unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

#[tauri::command]
pub fn set_model(model: String) -> Result<(), String> {
    let mut settings = load_settings();
    settings.model = Some(model);
    save_settings(&settings)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::DEFAULT_MODEL;

    #[test]
    fn default_model_is_set() {
        assert!(!DEFAULT_MODEL.is_empty());
    }

    #[test]
    fn default_model_is_gemini() {
        assert!(DEFAULT_MODEL.contains("gemini"));
    }
}
