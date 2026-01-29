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
    pub pending_files: Arc<Mutex<Vec<PendingFile>>>,
    pub policy: Arc<Mutex<AnalyzePolicy>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PendingFile {
    pub path: String,
    pub name: String,
    pub detected_at: String,
    pub size_bytes: u64,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnalyzePolicy {
    pub auto_analyze: bool,
    pub include_patterns: Vec<String>,  // 自動解析するパターン
    pub exclude_patterns: Vec<String>,  // 除外するパターン
    pub min_size_bytes: u64,            // 最小サイズ
    pub max_size_bytes: u64,            // 最大サイズ
}

impl Default for AnalyzePolicy {
    fn default() -> Self {
        Self {
            auto_analyze: false,
            include_patterns: vec![],
            exclude_patterns: vec!["test".to_string(), "draft".to_string()],
            min_size_bytes: 1024,           // 1KB以上
            max_size_bytes: 50 * 1024 * 1024, // 50MB以下
        }
    }
}

impl AnalyzePolicy {
    /// ファイルがポリシーに合致するか判定
    pub fn should_auto_analyze(&self, file: &PendingFile) -> bool {
        if !self.auto_analyze {
            return false;
        }

        // サイズチェック
        if file.size_bytes < self.min_size_bytes || file.size_bytes > self.max_size_bytes {
            return false;
        }

        let name_lower = file.name.to_lowercase();

        // 除外パターンチェック
        for pattern in &self.exclude_patterns {
            if name_lower.contains(&pattern.to_lowercase()) {
                return false;
            }
        }

        // includeパターンが空なら全て対象、そうでなければマッチするもののみ
        if self.include_patterns.is_empty() {
            return true;
        }

        for pattern in &self.include_patterns {
            if name_lower.contains(&pattern.to_lowercase()) {
                return true;
            }
        }

        false
    }
}

// ========== Tauri Commands ==========

#[tauri::command]
async fn start_watching(
    state: State<'_, AppState>,
    folder_path: String,
) -> Result<String, String> {
    let mut watcher_guard = state.watcher.lock().await;

    match watcher::FolderWatcher::new(&folder_path) {
        Ok(w) => {
            *watcher_guard = Some(w);
            Ok(format!("監視開始: {}", folder_path))
        }
        Err(e) => Err(format!("監視開始エラー: {}", e))
    }
}

#[tauri::command]
async fn stop_watching(state: State<'_, AppState>) -> Result<String, String> {
    let mut watcher_guard = state.watcher.lock().await;
    *watcher_guard = None;
    Ok("監視停止".to_string())
}

/// 検出された新しいPDFをポーリング
#[tauri::command]
async fn poll_new_files(state: State<'_, AppState>) -> Result<Vec<PendingFile>, String> {
    let watcher_guard = state.watcher.lock().await;
    let mut pending_guard = state.pending_files.lock().await;
    let policy_guard = state.policy.lock().await;

    let mut new_files = Vec::new();
    let mut auto_analyze_files = Vec::new();

    if let Some(ref watcher) = *watcher_guard {
        let detected = watcher.poll_new_pdfs();

        for path in detected {
            if let Ok(metadata) = std::fs::metadata(&path) {
                let file = PendingFile {
                    path: path.clone(),
                    name: std::path::Path::new(&path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    detected_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    size_bytes: metadata.len(),
                };

                // ポリシーに基づいて自動解析判定
                if policy_guard.should_auto_analyze(&file) {
                    auto_analyze_files.push(file.clone());
                }

                pending_guard.push(file.clone());
                new_files.push(file);
            }
        }
    }

    Ok(new_files)
}

/// 保留中のファイル一覧を取得
#[tauri::command]
async fn get_pending_files(state: State<'_, AppState>) -> Result<Vec<PendingFile>, String> {
    let pending = state.pending_files.lock().await;
    Ok(pending.clone())
}

/// 保留リストからファイルを削除
#[tauri::command]
async fn remove_pending_file(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let mut pending = state.pending_files.lock().await;
    pending.retain(|f| f.path != path);
    Ok(())
}

/// 保留リストをクリア
#[tauri::command]
async fn clear_pending_files(state: State<'_, AppState>) -> Result<(), String> {
    let mut pending = state.pending_files.lock().await;
    pending.clear();
    Ok(())
}

/// PDFを解析
#[tauri::command]
async fn analyze_file(
    state: State<'_, AppState>,
    file_path: String,
) -> Result<CheckResult, String> {
    // PDFからテキスト抽出
    let text = pdf_processor::extract_text(&file_path)
        .map_err(|e| format!("PDF読み込みエラー: {}", e))?;

    // Claude CLIで解析
    let analysis = claude_api::analyze_document(&text).await
        .map_err(|e| format!("解析エラー: {}", e))?;

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

    // DBに保存
    let db = state.db.lock().await;
    db.save_result(&result).map_err(|e| e.to_string())?;

    // 保留リストから削除
    let mut pending = state.pending_files.lock().await;
    pending.retain(|f| f.path != file_path);

    Ok(result)
}

/// チェック履歴を取得
#[tauri::command]
async fn get_check_history(
    state: State<'_, AppState>,
    limit: Option<i32>,
) -> Result<Vec<CheckResult>, String> {
    let db = state.db.lock().await;
    db.get_recent_results(limit.unwrap_or(50))
        .map_err(|e| e.to_string())
}

// ========== Policy Commands ==========

#[tauri::command]
async fn get_policy(state: State<'_, AppState>) -> Result<AnalyzePolicy, String> {
    let policy = state.policy.lock().await;
    Ok(policy.clone())
}

#[tauri::command]
async fn save_policy(state: State<'_, AppState>, policy: AnalyzePolicy) -> Result<(), String> {
    let mut current = state.policy.lock().await;
    *current = policy.clone();

    // ファイルに保存
    let data_dir = dirs::data_local_dir()
        .ok_or("データディレクトリが見つかりません")?
        .join("ShoruiChecker");
    std::fs::create_dir_all(&data_dir).ok();

    let path = data_dir.join("policy.json");
    let json = serde_json::to_string_pretty(&policy)
        .map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())?;

    Ok(())
}

// ========== Guidelines Commands ==========

#[tauri::command]
fn get_guidelines() -> Option<String> {
    claude_api::load_guidelines()
}

#[tauri::command]
fn save_guidelines(content: String) -> Result<String, String> {
    claude_api::save_guidelines(&content)
}

#[tauri::command]
fn check_claude_cli() -> bool {
    std::process::Command::new("claude")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ========== App Entry ==========

fn load_policy() -> AnalyzePolicy {
    let path = dirs::data_local_dir()
        .map(|d| d.join("ShoruiChecker").join("policy.json"));

    if let Some(path) = path {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(policy) = serde_json::from_str(&content) {
                return policy;
            }
        }
    }

    AnalyzePolicy::default()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    let db = database::Database::new()
        .expect("Failed to initialize database");

    let policy = load_policy();

    let state = AppState {
        watcher: Arc::new(Mutex::new(None)),
        db: Arc::new(Mutex::new(db)),
        pending_files: Arc::new(Mutex::new(Vec::new())),
        policy: Arc::new(Mutex::new(policy)),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            start_watching,
            stop_watching,
            poll_new_files,
            get_pending_files,
            remove_pending_file,
            clear_pending_files,
            analyze_file,
            get_check_history,
            get_policy,
            save_policy,
            get_guidelines,
            save_guidelines,
            check_claude_cli,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
