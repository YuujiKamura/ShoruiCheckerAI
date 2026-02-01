//! Code review module using ai-code-review crate

use std::path::Path;
use std::sync::Mutex;

use ai_code_review::{Backend, CodeReviewer, PromptType};
use tauri::{AppHandle, Emitter};

use crate::events::{CodeReviewEvent, LogEvent};
use crate::settings::{load_settings, save_settings};

/// Global state for the code reviewer
static CODE_REVIEWER: Mutex<Option<CodeReviewer>> = Mutex::new(None);

#[tauri::command]
pub fn get_code_watch_folder() -> Option<String> {
    load_settings().code_watch_folder
}

#[tauri::command]
pub fn is_code_review_enabled() -> bool {
    load_settings().code_review_enabled
}

#[tauri::command]
pub fn set_code_watch_folder(app: AppHandle, folder: String) -> Result<(), String> {
    let mut settings = load_settings();
    settings.code_watch_folder = Some(folder.clone());
    save_settings(&settings)?;

    if settings.code_review_enabled {
        start_code_watcher(app, &folder)?;
    }
    Ok(())
}

#[tauri::command]
pub fn set_code_review_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut settings = load_settings();
    settings.code_review_enabled = enabled;
    save_settings(&settings)?;

    if enabled {
        if let Some(folder) = &settings.code_watch_folder {
            start_code_watcher(app, folder)?;
        }
    } else {
        stop_code_watcher()?;
    }
    Ok(())
}

#[tauri::command]
pub fn stop_code_watching() -> Result<(), String> {
    stop_code_watcher()
}

/// Start the code watcher using CodeReviewer
pub(crate) fn start_code_watcher(app: AppHandle, folder: &str) -> Result<(), String> {
    // Stop existing watcher first
    stop_code_watcher()?;

    let folder_path = Path::new(folder);
    if !folder_path.exists() {
        return Err("フォルダが存在しません".to_string());
    }

    let log_path = folder_path.join(".code-reviews.log");
    let app_clone = app.clone();

    let mut reviewer = CodeReviewer::new(folder_path)
        .map_err(|e| e.to_string())?
        .with_backend(Backend::Gemini)
        .with_extensions(&["rs", "ts", "tsx", "js", "py"])
        .with_prompt_type(PromptType::Default)
        .with_log_file(&log_path)
        .on_review(move |result| {
            let event = CodeReviewEvent {
                path: result.path.to_string_lossy().to_string(),
                name: result.name.clone(),
                review_result: result.review.clone(),
                timestamp: result.timestamp.clone(),
                has_issues: result.has_issues,
            };

            // Emit review complete event
            let _ = app_clone.emit("code-review-complete", event.clone());

            // Emit log event
            let _ = app_clone.emit(
                "log",
                LogEvent {
                    message: format!(
                        "✓ レビュー完了: {} {}",
                        result.name,
                        if result.has_issues { "(問題あり)" } else { "" }
                    ),
                    level: if result.has_issues { "info" } else { "success" }.to_string(),
                },
            );

            // Show notification only if issues found
            if result.has_issues {
                let _ = app_clone.emit(
                    "show-notification",
                    serde_json::json!({
                        "title": "コードレビュー",
                        "body": format!("{}: 問題が検出されました", result.name),
                        "path": result.path.to_string_lossy().to_string()
                    }),
                );
            }
        });

    reviewer.start().map_err(|e| e.to_string())?;

    // Store the reviewer
    let mut handle = CODE_REVIEWER.lock().map_err(|e| e.to_string())?;
    *handle = Some(reviewer);

    Ok(())
}

/// Stop the code watcher
fn stop_code_watcher() -> Result<(), String> {
    let mut handle = CODE_REVIEWER.lock().map_err(|e| e.to_string())?;
    if let Some(mut reviewer) = handle.take() {
        // Ignore NotRunning error
        let _ = reviewer.stop();
    }
    Ok(())
}
