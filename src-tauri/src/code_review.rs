use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::channel;
use std::sync::Mutex;
use std::thread;
use std::time::Instant;

use notify::{Event, EventKind, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
use crate::CREATE_NO_WINDOW;

use crate::events::{CodeReviewEvent, LogEvent};
use crate::settings::{load_settings, save_settings, DEFAULT_MODEL};

// Debounce duration for code review (500ms)
const CODE_REVIEW_DEBOUNCE_MS: u64 = 500;

// Code file extensions to watch
const CODE_EXTENSIONS: &[&str] = &["rs", "ts", "tsx", "js", "py"];

// Global state for watcher
static CODE_WATCHER_HANDLE: Mutex<Option<notify::RecommendedWatcher>> = Mutex::new(None);
static CODE_REVIEW_STATE: Mutex<Option<CodeWatcherState>> = Mutex::new(None);

/// コード監視の状態管理
struct CodeWatcherState {
    last_review: HashMap<PathBuf, Instant>,
    review_log: PathBuf,
}

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

    // Start watcher if enabled
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

/// コードレビュー結果ログのパスを取得
fn get_code_review_log_path(folder: &str) -> PathBuf {
    Path::new(folder).join(".code-reviews.log")
}

/// ファイルがコード監視対象か判定
fn is_code_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| CODE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// git diffを取得（unstaged変更）
fn get_git_diff(file_path: &Path) -> Option<String> {
    let file_str = file_path.to_string_lossy();
    let parent = file_path.parent()?;

    let mut cmd = Command::new("git");
    cmd.args(["diff", "--", &file_str]).current_dir(parent);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().ok()?;
    if output.status.success() {
        let diff = String::from_utf8_lossy(&output.stdout).to_string();
        if diff.trim().is_empty() {
            // No unstaged changes, try staged
            let mut cmd2 = Command::new("git");
            cmd2
                .args(["diff", "--cached", "--", &file_str])
                .current_dir(parent);
            #[cfg(target_os = "windows")]
            cmd2.creation_flags(CREATE_NO_WINDOW);

            let output2 = cmd2.output().ok()?;
            if output2.status.success() {
                let diff2 = String::from_utf8_lossy(&output2.stdout).to_string();
                if !diff2.trim().is_empty() {
                    return Some(diff2);
                }
            }
            None
        } else {
            Some(diff)
        }
    } else {
        None
    }
}

/// ファイル全体を読み取る（gitリポジトリ外用）
fn read_file_content(file_path: &Path) -> Option<String> {
    fs::read_to_string(file_path).ok()
}

/// コード変更をGemini CLIでレビュー
fn review_code_change(file_path: &Path, content: &str, model: &str) -> Result<String, String> {
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let temp_dir = home_dir.join(".shoruichecker_code_review_temp");
    fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;

    let file_name = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let prompt = format!(
        r#"以下のコード変更をアーキテクチャの観点からレビューしてください。

ファイル: {}

```
{}
```

## レビュー観点（優先度順）
1. 設計・アーキテクチャ
   - この変更はこのファイルにあるべきか（責務の分離）
   - 関数/モジュールの肥大化につながっていないか
   - 適切な抽象化がされているか
2. コード品質
   - 関数が長すぎないか（50行超えは要注意）
   - 重複コードはないか
   - 命名は適切か
3. バグ・セキュリティ（明らかな問題のみ）

## 出力形式
- 問題がある場合は「⚠」で具体的に指摘
- 設計改善の提案があれば「💡」で提案
- 問題がない場合は「✓ 問題なし」
- 簡潔に（5行以内）"#,
        file_name,
        content
    );

    let prompt_file = temp_dir.join("prompt.txt");
    fs::write(&prompt_file, &prompt).map_err(|e| e.to_string())?;

    let gemini_path = std::env::var("APPDATA")
        .map(|p| format!("{}\\npm\\gemini.cmd", p))
        .unwrap_or_else(|_| "gemini".to_string());

    let ps_script = format!(
        r#"$OutputEncoding = [Console]::OutputEncoding = [Text.Encoding]::UTF8
Get-Content -Raw -Encoding UTF8 'prompt.txt' | & '{}' -m {} -o text
"#,
        gemini_path.replace("'", "''"),
        model
    );

    let script_file = temp_dir.join("run.ps1");
    fs::write(&script_file, &ps_script).map_err(|e| e.to_string())?;

    let mut cmd = Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        &script_file.to_string_lossy(),
    ])
    .current_dir(&temp_dir);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(&temp_dir);

    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout).to_string();
        let result = result
            .lines()
            .filter(|line| {
                !line.contains("Loaded cached credentials")
                    && !line.contains("Hook registry initialized")
            })
            .collect::<Vec<_>>()
            .join("\n");
        Ok(result)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// レビュー結果をログに追記（JSON Lines形式）
fn append_review_log(log_path: &Path, event: &CodeReviewEvent) -> Result<(), String> {
    use std::io::Write;
    let json = serde_json::to_string(event).map_err(|e| e.to_string())?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|e| e.to_string())?;
    writeln!(file, "{}", json).map_err(|e| e.to_string())?;
    Ok(())
}

/// コード監視を開始
pub(crate) fn start_code_watcher(app: AppHandle, folder: &str) -> Result<(), String> {
    // Stop existing watcher
    {
        let mut handle = CODE_WATCHER_HANDLE.lock().map_err(|e| e.to_string())?;
        *handle = None;
    }

    let folder_path = PathBuf::from(folder);
    if !folder_path.exists() {
        return Err("フォルダが存在しません".to_string());
    }

    // Initialize state
    {
        let mut state = CODE_REVIEW_STATE.lock().map_err(|e| e.to_string())?;
        *state = Some(CodeWatcherState {
            last_review: HashMap::new(),
            review_log: get_code_review_log_path(folder),
        });
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
        let mut handle = CODE_WATCHER_HANDLE.lock().map_err(|e| e.to_string())?;
        *handle = Some(watcher);
    }

    let app_clone = app.clone();
    let model = load_settings()
        .model
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

    thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            // Handle Create and Modify events for code files
            match event.kind {
                EventKind::Create(_) | EventKind::Modify(_) => {
                    for path in event.paths {
                        if !is_code_file(&path) {
                            continue;
                        }

                        // Check debounce
                        let should_review = {
                            let mut state_lock = match CODE_REVIEW_STATE.lock() {
                                Ok(s) => s,
                                Err(_) => continue,
                            };
                            if let Some(ref mut state) = *state_lock {
                                let now = Instant::now();
                                if let Some(last) = state.last_review.get(&path) {
                                    if now.duration_since(*last).as_millis()
                                        < CODE_REVIEW_DEBOUNCE_MS as u128
                                    {
                                        false
                                    } else {
                                        state.last_review.insert(path.clone(), now);
                                        true
                                    }
                                } else {
                                    state.last_review.insert(path.clone(), now);
                                    true
                                }
                            } else {
                                false
                            }
                        };

                        if !should_review {
                            continue;
                        }

                        // Get diff or file content
                        let content = get_git_diff(&path).or_else(|| read_file_content(&path));

                        let content = match content {
                            Some(c) if !c.trim().is_empty() => c,
                            _ => continue,
                        };

                        let path_str = path.to_string_lossy().to_string();
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "unknown".to_string());

                        // Log that we're reviewing
                        let _ = app_clone.emit(
                            "log",
                            LogEvent {
                                message: format!("コードレビュー中: {}", name),
                                level: "wave".to_string(),
                            },
                        );

                        // Review in background
                        let model_clone = model.clone();
                        let app_for_review = app_clone.clone();
                        let path_for_review = path.clone();

                        thread::spawn(move || match review_code_change(
                            &path_for_review,
                            &content,
                            &model_clone,
                        ) {
                            Ok(result) => {
                                let has_issues = result.contains("⚠");
                                let timestamp =
                                    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

                                let event = CodeReviewEvent {
                                    path: path_str.clone(),
                                    name: name.clone(),
                                    review_result: result.clone(),
                                    timestamp: timestamp.clone(),
                                    has_issues,
                                };

                                // Append to log
                                if let Ok(state_lock) = CODE_REVIEW_STATE.lock() {
                                    if let Some(ref state) = *state_lock {
                                        let _ = append_review_log(&state.review_log, &event);
                                    }
                                }

                                // Emit event to frontend
                                let _ = app_for_review.emit("code-review-complete", event.clone());

                                // Log completion
                                let _ = app_for_review.emit(
                                    "log",
                                    LogEvent {
                                        message: format!(
                                            "✓ レビュー完了: {} {}",
                                            name,
                                            if has_issues { "(問題あり)" } else { "" }
                                        ),
                                        level: if has_issues { "info" } else { "success" }
                                            .to_string(),
                                    },
                                );

                                // Show notification only if issues found
                                if has_issues {
                                    let _ = app_for_review.emit(
                                        "show-notification",
                                        serde_json::json!({
                                            "title": "コードレビュー",
                                            "body": format!("{}: 問題が検出されました", name),
                                            "path": path_str
                                        }),
                                    );
                                }
                            }
                            Err(e) => {
                                let _ = app_for_review.emit(
                                    "log",
                                    LogEvent {
                                        message: format!("レビューエラー: {} - {}", name, e),
                                        level: "error".to_string(),
                                    },
                                );
                            }
                        });
                    }
                }
                _ => {}
            }
        }
    });

    Ok(())
}

/// コード監視を停止
fn stop_code_watcher() -> Result<(), String> {
    let mut handle = CODE_WATCHER_HANDLE.lock().map_err(|e| e.to_string())?;
    *handle = None;

    let mut state = CODE_REVIEW_STATE.lock().map_err(|e| e.to_string())?;
    *state = None;

    Ok(())
}
