use std::process::Command;
use std::path::{Path, PathBuf};
use std::fs;
use std::sync::{Arc, Mutex};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
use tauri::{AppHandle, Emitter, Manager};
use tauri::tray::{TrayIconBuilder, MouseButton, MouseButtonState, TrayIconEvent};
use tauri::menu::{Menu, MenuItem};
use serde::{Serialize, Deserialize};
use notify::{Watcher, RecursiveMode, Event, EventKind};
use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;

#[derive(Clone, Serialize)]
struct LogEvent {
    message: String,
    level: String,
}

#[derive(Clone, Serialize)]
struct PdfDetectedEvent {
    path: String,
    name: String,
}

#[derive(Clone, Serialize, Deserialize, Default)]
struct AppSettings {
    watch_folder: Option<String>,
    model: Option<String>,
}

const DEFAULT_MODEL: &str = "gemini-2.5-pro";

// Global state for watcher
static WATCHER_HANDLE: Mutex<Option<notify::RecommendedWatcher>> = Mutex::new(None);

fn emit_log(app: &AppHandle, message: &str, level: &str) {
    let _ = app.emit("log", LogEvent {
        message: message.to_string(),
        level: level.to_string(),
    });
}

fn get_settings_path() -> PathBuf {
    let config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    config_dir.join("shoruichecker").join("settings.json")
}

fn load_settings() -> AppSettings {
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

fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let path = get_settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_watch_folder() -> Option<String> {
    load_settings().watch_folder
}

#[tauri::command]
fn set_watch_folder(app: AppHandle, folder: String) -> Result<(), String> {
    let mut settings = load_settings();
    settings.watch_folder = Some(folder.clone());
    save_settings(&settings)?;

    // Restart watcher with new folder
    start_watcher(app, &folder)?;
    Ok(())
}

#[tauri::command]
fn stop_watching() -> Result<(), String> {
    let mut handle = WATCHER_HANDLE.lock().map_err(|e| e.to_string())?;
    *handle = None;
    Ok(())
}

#[tauri::command]
fn get_model() -> String {
    load_settings().model.unwrap_or_else(|| DEFAULT_MODEL.to_string())
}

#[tauri::command]
fn set_model(model: String) -> Result<(), String> {
    let mut settings = load_settings();
    settings.model = Some(model);
    save_settings(&settings)?;
    Ok(())
}

/// Open external terminal for Gemini authentication
#[tauri::command]
fn open_gemini_auth() -> Result<(), String> {
    let gemini_path = std::env::var("APPDATA")
        .map(|p| format!("{}\\npm\\gemini.cmd", p))
        .unwrap_or_else(|_| "gemini".to_string());

    // Open new PowerShell window with gemini CLI
    Command::new("cmd")
        .args(["/c", "start", "powershell", "-NoExit", "-Command", &format!("& '{}'", gemini_path)])
        .spawn()
        .map_err(|e| format!("ターミナル起動エラー: {}", e))?;

    Ok(())
}

/// Check if Gemini CLI is authenticated
#[tauri::command]
fn check_gemini_auth() -> Result<bool, String> {
    let gemini_path = std::env::var("APPDATA")
        .map(|p| format!("{}\\npm\\gemini.cmd", p))
        .unwrap_or_else(|_| "gemini".to_string());

    // Try running gemini with a simple command
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-Command", &format!("& '{}' --version", gemini_path)]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd.output().map_err(|e| format!("確認エラー: {}", e))?;

    // If it succeeds, we're authenticated
    Ok(output.status.success())
}

fn start_watcher(app: AppHandle, folder: &str) -> Result<(), String> {
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
    }).map_err(|e| e.to_string())?;

    watcher.watch(&folder_path, RecursiveMode::Recursive)
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
                    if path.extension().map(|e| e == "pdf" || e == "PDF").unwrap_or(false) {
                        let path_str = path.to_string_lossy().to_string();
                        let name = path.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "unknown.pdf".to_string());

                        // Emit event to frontend
                        let _ = app_clone.emit("pdf-detected", PdfDetectedEvent {
                            path: path_str.clone(),
                            name: name.clone(),
                        });

                        // Show notification
                        let _ = app_clone.emit("show-notification", serde_json::json!({
                            "title": "PDF検出",
                            "body": format!("新しいPDF: {}", name),
                            "path": path_str
                        }));
                    }
                }
            }
        }
    });

    Ok(())
}

/// 単一PDFを解析する内部関数
fn analyze_single_pdf(path: &str, task_id: &str, model: &str) -> Result<String, String> {
    let pdf_path = Path::new(path);
    let file_name = pdf_path.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown.pdf".to_string());

    // Create temp directory for this task
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let temp_dir = home_dir.join(format!(".shoruichecker_temp_{}", task_id));
    fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;

    // Copy PDF to temp directory
    let dest_path = temp_dir.join(&file_name);
    fs::copy(path, &dest_path).map_err(|e| format!("ファイルコピーエラー: {}", e))?;

    // Build prompt
    let prompt = format!(
        r#"あなたは日本語で回答するアシスタントです。必ず日本語で回答してください。

添付のPDF書類の内容を読み取り、整合性をチェックしてください。

## 注意事項
- 文字は正確に読み取ること（特に地名、人名、会社名）
- 似た漢字を間違えないこと
- 数値は桁を間違えないこと

## 書類タイプ別チェックポイント

### 契約書の場合
- 契約当事者（発注者・受注者）の名称が書類内で一貫しているか
- 金額計算（工事価格 + 消費税 = 請負代金額）が正しいか
- 工期の日付が妥当か（着工日 < 完成日）
- 必要な署名・押印欄があるか
- 選択肢形式の項目は○（丸）がついている選択肢を読み取ること

### 交通誘導員配置実績の場合
- 人数欄の数値と、実際に列挙された名前の数が一致するか
- 集計表と伝票の人数・日付・時間が一致するか

### 測量図面の場合
- 縦断図と横断図の計画高・地盤高の照合

## 出力形式
- まず書類タイプを判定して報告
- 整合している項目は「✓」で示す
- 問題がある項目は「⚠」で具体的に指摘

ファイル: {}"#,
        file_name
    );

    let prompt_file = temp_dir.join("prompt.txt");
    fs::write(&prompt_file, &prompt).map_err(|e| e.to_string())?;

    let gemini_path = std::env::var("APPDATA")
        .map(|p| format!("{}\\npm\\gemini.cmd", p))
        .unwrap_or_else(|_| "gemini".to_string());

    let ps_script = format!(
        r#"$OutputEncoding = [Console]::OutputEncoding = [Text.Encoding]::UTF8
$prompt = Get-Content -Raw -Encoding UTF8 '{}'
& '{}' -m {} -o text $prompt '{}'
"#,
        prompt_file.to_string_lossy().replace("'", "''"),
        gemini_path.replace("'", "''"),
        model,
        dest_path.to_string_lossy().replace("'", "''")
    );

    let script_file = temp_dir.join("run.ps1");
    fs::write(&script_file, &ps_script).map_err(|e| e.to_string())?;

    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", &script_file.to_string_lossy()])
        .current_dir(&temp_dir);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(&temp_dir);

    if output.status.success() {
        let result = String::from_utf8_lossy(&output.stdout).to_string();
        let result = result.lines()
            .filter(|line| !line.contains("Loaded cached credentials") && !line.contains("Hook registry initialized"))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(result)
    } else {
        let error = String::from_utf8_lossy(&output.stderr).to_string();
        Err(error)
    }
}

#[derive(Clone, Serialize)]
struct AnalysisResult {
    file_name: String,
    path: String,
    result: Option<String>,
    error: Option<String>,
}

/// PDFを並列解析 (Gemini CLI使用)
#[tauri::command]
async fn analyze_pdfs(app: AppHandle, paths: Vec<String>) -> Result<String, String> {
    if paths.is_empty() {
        return Err("ファイルが指定されていません".to_string());
    }

    let total = paths.len();
    emit_log(&app, &format!("=== PDF整合性チェック開始 ({} ファイル) ===", total), "info");

    // Get model setting
    let model = load_settings().model.unwrap_or_else(|| DEFAULT_MODEL.to_string());

    if total == 1 {
        // Single file - simple execution
        let path = &paths[0];
        let file_name = Path::new(path).file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown.pdf".to_string());

        emit_log(&app, &format!("{} を解析中...", file_name), "wave");

        match analyze_single_pdf(path, "single", &model) {
            Ok(result) => {
                emit_log(&app, "✓ 解析完了", "success");
                Ok(result)
            }
            Err(e) => {
                emit_log(&app, &format!("解析エラー: {}", e), "error");
                Err(e)
            }
        }
    } else {
        // Multiple files - parallel execution
        emit_log(&app, &format!("{} で {} ファイルを並列解析中...", model, total), "wave");

        let mut handles = vec![];

        for (i, path) in paths.into_iter().enumerate() {
            let model_clone = model.clone();
            let task_id = format!("task_{}", i);
            let app_clone = app.clone();
            let file_name = Path::new(&path).file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("file_{}.pdf", i));

            let handle = thread::spawn(move || {
                let result = analyze_single_pdf(&path, &task_id, &model_clone);
                let _ = app_clone.emit("analysis-progress", serde_json::json!({
                    "file_name": file_name.clone(),
                    "completed": true,
                    "success": result.is_ok()
                }));
                AnalysisResult {
                    file_name,
                    path,
                    result: result.clone().ok(),
                    error: result.err(),
                }
            });
            handles.push(handle);
        }

        // Collect results
        let mut results: Vec<AnalysisResult> = vec![];
        for handle in handles {
            if let Ok(result) = handle.join() {
                results.push(result);
            }
        }

        // Format combined results
        let mut output = String::new();
        let success_count = results.iter().filter(|r| r.result.is_some()).count();

        for r in &results {
            output.push_str(&format!("\n## 📄 {}\n", r.file_name));
            output.push_str("---\n");
            if let Some(ref res) = r.result {
                output.push_str(res);
            } else if let Some(ref err) = r.error {
                output.push_str(&format!("⚠ エラー: {}", err));
            }
            output.push_str("\n\n");
        }

        emit_log(&app, &format!("✓ 解析完了 ({}/{})", success_count, total), "success");
        Ok(output)
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // Create system tray
            let quit = MenuItem::with_id(app, "quit", "終了", true, None::<&str>)?;
            let show = MenuItem::with_id(app, "show", "表示", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "quit" => {
                            app.exit(0);
                        }
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Start watcher if folder is configured
            let settings = load_settings();
            if let Some(folder) = settings.watch_folder {
                let app_handle = app.handle().clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(1));
                    let _ = start_watcher(app_handle, &folder);
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            analyze_pdfs,
            get_watch_folder,
            set_watch_folder,
            stop_watching,
            open_gemini_auth,
            check_gemini_auth,
            get_model,
            set_model
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
