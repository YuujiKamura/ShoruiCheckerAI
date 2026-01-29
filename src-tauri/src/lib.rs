mod watcher;
mod pdf_processor;
mod claude_api;
mod database;

use std::sync::Arc;
use tokio::sync::Mutex;
use tauri::State;
use serde::{Deserialize, Serialize};

// Application state
pub struct AppState {
    pub watcher: Arc<Mutex<Option<watcher::FolderWatcher>>>,
    pub db: Arc<Mutex<database::Database>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CheckResult {
    pub file_path: String,
    pub file_name: String,
    pub checked_at: String,
    pub status: String,  // "ok", "warning", "error"
    pub message: String,
    pub details: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WatchFolder {
    pub path: String,
    pub enabled: bool,
}

// Tauri commands
#[tauri::command]
async fn start_watching(
    state: State<'_, AppState>,
    folder_path: String,
) -> Result<String, String> {
    let mut watcher_guard = state.watcher.lock().await;

    match watcher::FolderWatcher::new(&folder_path) {
        Ok(w) => {
            *watcher_guard = Some(w);
            Ok(format!("Started watching: {}", folder_path))
        }
        Err(e) => Err(format!("Failed to start watcher: {}", e))
    }
}

#[tauri::command]
async fn stop_watching(state: State<'_, AppState>) -> Result<String, String> {
    let mut watcher_guard = state.watcher.lock().await;
    *watcher_guard = None;
    Ok("Stopped watching".to_string())
}

#[tauri::command]
async fn get_check_history(
    state: State<'_, AppState>,
    limit: Option<i32>,
) -> Result<Vec<CheckResult>, String> {
    let db = state.db.lock().await;
    db.get_recent_results(limit.unwrap_or(50))
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn check_pdf_manually(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<CheckResult, String> {
    // Extract text from PDF
    let text = pdf_processor::extract_text(&file_path)
        .map_err(|e| format!("PDF extraction failed: {}", e))?;

    // Call Claude API for analysis
    let analysis = claude_api::analyze_document(&text).await
        .map_err(|e| format!("Claude API failed: {}", e))?;

    let result = CheckResult {
        file_path: file_path.clone(),
        file_name: std::path::Path::new(&file_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        checked_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        status: analysis.status,
        message: analysis.message,
        details: analysis.details,
    };

    // Save to database
    let db = state.db.lock().await;
    db.save_result(&result).map_err(|e| e.to_string())?;

    Ok(result)
}

#[tauri::command]
fn get_api_key_status() -> bool {
    std::env::var("ANTHROPIC_API_KEY").is_ok()
}

#[tauri::command]
fn set_api_key(key: String) -> Result<(), String> {
    std::env::set_var("ANTHROPIC_API_KEY", key);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    let db = database::Database::new()
        .expect("Failed to initialize database");

    let state = AppState {
        watcher: Arc::new(Mutex::new(None)),
        db: Arc::new(Mutex::new(db)),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            start_watching,
            stop_watching,
            get_check_history,
            check_pdf_manually,
            get_api_key_status,
            set_api_key,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
