use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
pub struct LogEvent {
    pub message: String,
    pub level: String,
}

#[derive(Clone, Serialize)]
pub struct PdfDetectedEvent {
    pub path: String,
    pub name: String,
}

#[derive(Clone, Serialize)]
pub struct CodeReviewEvent {
    pub path: String,
    pub name: String,
    pub review_result: String,
    pub timestamp: String,
    pub has_issues: bool,
}

pub fn emit_log(app: &AppHandle, message: &str, level: &str) {
    let _ = app.emit("log", LogEvent {
        message: message.to_string(),
        level: level.to_string(),
    });
}
