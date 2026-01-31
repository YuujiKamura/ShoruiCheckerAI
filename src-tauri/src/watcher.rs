use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::Mutex;
use std::thread;

use notify::{Event, EventKind, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};

use crate::events::PdfDetectedEvent;
use crate::settings::{load_settings, save_settings};

// Global state for watcher
static WATCHER_HANDLE: Mutex<Option<notify::RecommendedWatcher>> = Mutex::new(None);

/// 起動時の解析対象ファイルを取得
#[tauri::command]
pub fn get_startup_file() -> Option<String> {
    std::env::var("ANALYZE_FILE").ok()
}

#[tauri::command]
pub fn get_watch_folder() -> Option<String> {
    load_settings().watch_folder
}

#[tauri::command]
pub fn set_watch_folder(app: AppHandle, folder: String) -> Result<(), String> {
    let mut settings = load_settings();
    settings.watch_folder = Some(folder.clone());
    save_settings(&settings)?;

    // Restart watcher with new folder
    start_watcher(app, &folder)?;
    Ok(())
}

#[tauri::command]
pub fn stop_watching() -> Result<(), String> {
    let mut handle = WATCHER_HANDLE.lock().map_err(|e| e.to_string())?;
    *handle = None;
    Ok(())
}

pub(crate) fn start_watcher(app: AppHandle, folder: &str) -> Result<(), String> {
    // Stop existing watcher
    {
        let mut handle = WATCHER_HANDLE.lock().map_err(|e| e.to_string())?;
        *handle = None;
    }

    let folder_path = PathBuf::from(folder);
    if !folder_path.exists() {
        return Err("フォルダが存在しません".to_string());
    }

    let (tx, rx) = channel();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            let _ = tx.send(event);
        }
    })
    .map_err(|e| e.to_string())?;

    watcher
        .watch(&folder_path, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;

    // Store watcher handle
    {
        let mut handle = WATCHER_HANDLE.lock().map_err(|e| e.to_string())?;
        *handle = Some(watcher);
    }

    // Spawn thread to handle events
    let app_clone = app.clone();
    thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            if let EventKind::Create(_) = event.kind {
                for path in event.paths {
                    if path
                        .extension()
                        .map(|e| e == "pdf" || e == "PDF")
                        .unwrap_or(false)
                    {
                        let path_str = path.to_string_lossy().to_string();
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "unknown.pdf".to_string());

                        // Emit event to frontend
                        let _ = app_clone.emit(
                            "pdf-detected",
                            PdfDetectedEvent {
                                path: path_str.clone(),
                                name: name.clone(),
                            },
                        );

                        // Show notification
                        let _ = app_clone.emit(
                            "show-notification",
                            serde_json::json!({
                                "title": "PDF検出",
                                "body": format!("新しいPDF: {}", name),
                                "path": path_str
                            }),
                        );
                    }
                }
            }
        }
    });

    Ok(())
}
