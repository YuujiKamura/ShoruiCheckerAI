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

/// 解析履歴エントリ
#[derive(Clone, Serialize, Deserialize)]
struct AnalysisHistoryEntry {
    file_name: String,
    file_path: String,
    analyzed_at: String,
    document_type: Option<String>,
    summary: String,
    issues: Vec<String>,
}

/// 解析履歴（プロジェクト単位）
#[derive(Clone, Serialize, Deserialize, Default)]
struct AnalysisHistory {
    project_folder: String,
    entries: Vec<AnalysisHistoryEntry>,
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

/// 履歴ファイルのパスを取得（プロジェクトフォルダ単位）
fn get_history_path(project_folder: &str) -> PathBuf {
    let config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let folder_hash = format!("{:x}", md5_hash(project_folder));
    config_dir.join("shoruichecker").join("history").join(format!("{}.json", folder_hash))
}

/// 簡易MD5ハッシュ（フォルダパスからファイル名を生成）
fn md5_hash(s: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// 履歴を読み込む
fn load_history(project_folder: &str) -> AnalysisHistory {
    let path = get_history_path(project_folder);
    if path.exists() {
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| AnalysisHistory {
                project_folder: project_folder.to_string(),
                entries: vec![],
            })
    } else {
        AnalysisHistory {
            project_folder: project_folder.to_string(),
            entries: vec![],
        }
    }
}

/// 履歴を保存
fn save_history(history: &AnalysisHistory) -> Result<(), String> {
    let path = get_history_path(&history.project_folder);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(history).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

/// 解析結果から履歴エントリを作成
fn create_history_entry(file_name: &str, file_path: &str, result: &str) -> AnalysisHistoryEntry {
    // 結果から書類タイプを抽出（簡易パース）
    let document_type = if result.contains("契約書") {
        Some("契約書".to_string())
    } else if result.contains("見積") {
        Some("見積書".to_string())
    } else if result.contains("請求") {
        Some("請求書".to_string())
    } else if result.contains("配置実績") || result.contains("交通誘導") {
        Some("交通誘導員配置実績".to_string())
    } else {
        None
    };

    // 問題点を抽出（⚠マーク行）
    let issues: Vec<String> = result.lines()
        .filter(|line| line.contains("⚠") || line.contains("警告") || line.contains("不整合") || line.contains("矛盾"))
        .map(|s| s.trim().to_string())
        .collect();

    // 要約を作成（最初の数行）
    let summary: String = result.lines()
        .take(10)
        .collect::<Vec<_>>()
        .join("\n");

    AnalysisHistoryEntry {
        file_name: file_name.to_string(),
        file_path: file_path.to_string(),
        analyzed_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        document_type,
        summary,
        issues,
    }
}

/// 履歴からコンテキストを生成
fn build_history_context(history: &AnalysisHistory) -> String {
    if history.entries.is_empty() {
        return String::new();
    }

    let mut context = String::from("\n\n## 過去の解析履歴（参考情報）\n");
    context.push_str("以下は同じプロジェクトで過去に解析した書類の情報です。整合性チェック時に参照してください。\n\n");

    for entry in history.entries.iter().rev().take(10) {
        context.push_str(&format!("### {} ({})\n", entry.file_name, entry.analyzed_at));
        if let Some(doc_type) = &entry.document_type {
            context.push_str(&format!("- 書類タイプ: {}\n", doc_type));
        }
        if !entry.issues.is_empty() {
            context.push_str("- 検出された問題:\n");
            for issue in &entry.issues {
                context.push_str(&format!("  - {}\n", issue));
            }
        }
        context.push_str(&format!("- 要約: {}\n\n", entry.summary.lines().take(3).collect::<Vec<_>>().join(" ")));
    }

    context
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

    // Get project folder (parent directory)
    let project_folder = pdf_path.parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    // Load history for this project
    let history = load_history(&project_folder);
    let history_context = build_history_context(&history);

    // Create temp directory for this task
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let temp_dir = home_dir.join(format!(".shoruichecker_temp_{}", task_id));
    fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;

    // Copy PDF to temp directory
    let dest_path = temp_dir.join(&file_name);
    fs::copy(path, &dest_path).map_err(|e| format!("ファイルコピーエラー: {}", e))?;

    // Build prompt with history context
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
- 過去の解析履歴がある場合、それとの整合性も確認すること
{}
ファイル: {}"#,
        history_context,
        file_name
    );

    let prompt_file = temp_dir.join("prompt.txt");
    fs::write(&prompt_file, &prompt).map_err(|e| e.to_string())?;

    let gemini_path = std::env::var("APPDATA")
        .map(|p| format!("{}\\npm\\gemini.cmd", p))
        .unwrap_or_else(|_| "gemini".to_string());

    // Use stdin pipe to pass multi-line prompt correctly
    let ps_script = format!(
        r#"$OutputEncoding = [Console]::OutputEncoding = [Text.Encoding]::UTF8
Get-Content -Raw -Encoding UTF8 'prompt.txt' | & '{}' -m {} -o text '{}'
"#,
        gemini_path.replace("'", "''"),
        model,
        file_name.replace("'", "''")
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

        // Save to history
        let entry = create_history_entry(&file_name, path, &result);
        let mut history = load_history(&project_folder);
        // Remove old entry for same file if exists
        history.entries.retain(|e| e.file_name != file_name);
        history.entries.push(entry);
        // Keep only last 50 entries
        if history.entries.len() > 50 {
            history.entries = history.entries.split_off(history.entries.len() - 50);
        }
        let _ = save_history(&history);

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

/// 複数PDFをまとめて照合解析
fn analyze_compare_pdfs(paths: &[String], model: &str) -> Result<String, String> {
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let temp_dir = home_dir.join(".shoruichecker_temp_compare");
    fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;

    // Get project folder from first file
    let project_folder = paths.first()
        .and_then(|p| Path::new(p).parent())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    // Load history
    let history = load_history(&project_folder);
    let history_context = build_history_context(&history);

    // Copy all PDFs
    let mut copied_files: Vec<String> = Vec::new();
    let mut file_names: Vec<String> = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        let pdf_path = Path::new(path);
        let file_name = pdf_path.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("file_{}.pdf", i));
        file_names.push(file_name.clone());

        let dest_path = temp_dir.join(&file_name);
        fs::copy(path, &dest_path).map_err(|e| format!("ファイルコピーエラー: {}", e))?;
        copied_files.push(dest_path.to_string_lossy().to_string());
    }

    // Build comparison prompt with history
    let prompt = format!(
        r#"あなたは日本語で回答するアシスタントです。必ず日本語で回答してください。

添付の複数PDF書類を照合し、書類間の整合性をチェックしてください。

## 照合対象ファイル
{}

## チェックポイント
- 書類間で当事者名（発注者・受注者・会社名）が一致しているか
- 金額が書類間で整合しているか（見積書と契約書の金額一致等）
- 日付の整合性（契約日、工期、納期等）
- 数量・単価の整合性
- 印影・署名の有無
- 過去の解析履歴との整合性

## 出力形式
1. 各書類の概要を簡潔に説明
2. 書類間で整合している項目は「✓」で示す
3. 不整合や矛盾がある項目は「⚠」で具体的に指摘
4. 総合判定（整合/要確認/不整合）
{}"#,
        file_names.join("\n"),
        history_context
    );

    let prompt_file = temp_dir.join("prompt.txt");
    fs::write(&prompt_file, &prompt).map_err(|e| e.to_string())?;

    let gemini_path = std::env::var("APPDATA")
        .map(|p| format!("{}\\npm\\gemini.cmd", p))
        .unwrap_or_else(|_| "gemini".to_string());

    // Use relative file names since current_dir is temp_dir
    let pdf_array = file_names.iter()
        .map(|f| format!("    '{}'", f.replace("'", "''")))
        .collect::<Vec<_>>()
        .join(",\n");

    // Use stdin pipe to pass multi-line prompt correctly
    let ps_script = format!(
        r#"$OutputEncoding = [Console]::OutputEncoding = [Text.Encoding]::UTF8
$pdfs = @(
{}
)
Get-Content -Raw -Encoding UTF8 'prompt.txt' | & '{}' -m {} -o text $pdfs
"#,
        pdf_array,
        gemini_path.replace("'", "''"),
        model
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

        // Save comparison result to history for each file
        let mut history = load_history(&project_folder);
        let comparison_summary = format!("【照合解析】対象: {}", file_names.join(", "));
        for (i, path) in paths.iter().enumerate() {
            let file_name = &file_names[i];
            let entry = AnalysisHistoryEntry {
                file_name: file_name.clone(),
                file_path: path.clone(),
                analyzed_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                document_type: Some("照合解析".to_string()),
                summary: comparison_summary.clone(),
                issues: result.lines()
                    .filter(|line| line.contains("⚠"))
                    .map(|s| s.trim().to_string())
                    .collect(),
            };
            history.entries.retain(|e| e.file_name != *file_name);
            history.entries.push(entry);
        }
        if history.entries.len() > 50 {
            history.entries = history.entries.split_off(history.entries.len() - 50);
        }
        let _ = save_history(&history);

        Ok(result)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// PDFを解析 (Gemini CLI使用)
#[tauri::command]
async fn analyze_pdfs(app: AppHandle, paths: Vec<String>, mode: String) -> Result<String, String> {
    if paths.is_empty() {
        return Err("ファイルが指定されていません".to_string());
    }

    let total = paths.len();
    let model = load_settings().model.unwrap_or_else(|| DEFAULT_MODEL.to_string());

    // 照合モード
    if mode == "compare" {
        emit_log(&app, &format!("=== PDF照合解析開始 ({} ファイル) ===", total), "info");
        for path in &paths {
            let file_name = Path::new(path).file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown.pdf".to_string());
            emit_log(&app, &format!("  - {}", file_name), "info");
        }
        emit_log(&app, &format!("{} で照合中...", model), "wave");

        match analyze_compare_pdfs(&paths, &model) {
            Ok(result) => {
                emit_log(&app, "✓ 照合完了", "success");
                Ok(result)
            }
            Err(e) => {
                emit_log(&app, &format!("照合エラー: {}", e), "error");
                Err(e)
            }
        }
    }
    // 個別モード
    else {
        emit_log(&app, &format!("=== PDF個別解析開始 ({} ファイル) ===", total), "info");

        if total == 1 {
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
