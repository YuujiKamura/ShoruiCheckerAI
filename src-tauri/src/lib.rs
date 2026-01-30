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

/// PDFに解析結果とカスタム指示をメタデータとして埋め込む
fn embed_result_in_pdf_with_instruction(pdf_path: &str, result: &str, custom_instruction: &str) -> Result<(), String> {
    use lopdf::{Document, Object, StringFormat};

    let mut doc = Document::load(pdf_path).map_err(|e| format!("PDF読み込みエラー: {}", e))?;

    // Get or create Info dictionary
    let info_id = if let Some(info_ref) = doc.trailer.get(b"Info").ok().and_then(|o| o.as_reference().ok()) {
        info_ref
    } else {
        // Create new Info dictionary
        let info_dict = lopdf::Dictionary::new();
        let info_id = doc.add_object(Object::Dictionary(info_dict));
        doc.trailer.set("Info", Object::Reference(info_id));
        info_id
    };

    // Add custom metadata
    if let Ok(Object::Dictionary(ref mut info)) = doc.get_object_mut(info_id) {
        // Store analysis result (base64 encoded to avoid encoding issues)
        let encoded = base64_encode(result);
        info.set("ShoruiCheckerResult", Object::String(encoded.into_bytes(), StringFormat::Literal));

        // Store custom instruction if provided
        if !custom_instruction.is_empty() {
            let encoded_instruction = base64_encode(custom_instruction);
            info.set("ShoruiCheckerInstruction", Object::String(encoded_instruction.into_bytes(), StringFormat::Literal));
        }

        // Store analysis timestamp
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        info.set("ShoruiCheckerDate", Object::String(timestamp.into_bytes(), StringFormat::Literal));

        // Store version
        info.set("ShoruiCheckerVersion", Object::String(b"1.0".to_vec(), StringFormat::Literal));
    }

    doc.save(pdf_path).map_err(|e| format!("PDF保存エラー: {}", e))?;
    Ok(())
}

/// 後方互換性のためのラッパー
fn embed_result_in_pdf(pdf_path: &str, result: &str) -> Result<(), String> {
    embed_result_in_pdf_with_instruction(pdf_path, result, "")
}

/// PDF埋め込みデータ
#[derive(Clone, Serialize, Deserialize)]
struct PdfEmbeddedData {
    result: String,
    instruction: Option<String>,
    date: String,
}

/// PDFから埋め込まれた解析結果を読み取る
fn read_result_from_pdf(pdf_path: &str) -> Option<(String, String)> {
    let data = read_embedded_data_from_pdf(pdf_path)?;
    Some((data.result, data.date))
}

/// PDFから全埋め込みデータを読み取る
fn read_embedded_data_from_pdf(pdf_path: &str) -> Option<PdfEmbeddedData> {
    use lopdf::{Document, Object};

    let doc = Document::load(pdf_path).ok()?;

    let info_ref = doc.trailer.get(b"Info").ok()?.as_reference().ok()?;
    if let Ok(Object::Dictionary(info)) = doc.get_object(info_ref) {
        let result = info.get(b"ShoruiCheckerResult").ok()
            .and_then(|o| {
                if let Object::String(bytes, _) = o {
                    String::from_utf8(bytes.clone()).ok()
                        .and_then(|s| base64_decode(&s))
                } else {
                    None
                }
            })?;

        let instruction = info.get(b"ShoruiCheckerInstruction").ok()
            .and_then(|o| {
                if let Object::String(bytes, _) = o {
                    String::from_utf8(bytes.clone()).ok()
                        .and_then(|s| base64_decode(&s))
                } else {
                    None
                }
            });

        let date = info.get(b"ShoruiCheckerDate").ok()
            .and_then(|o| {
                if let Object::String(bytes, _) = o {
                    String::from_utf8(bytes.clone()).ok()
                } else {
                    None
                }
            })
            .unwrap_or_default();

        return Some(PdfEmbeddedData { result, instruction, date });
    }

    None
}

/// Base64エンコード
fn base64_encode(s: &str) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut encoder = base64_writer(&mut buf);
        encoder.write_all(s.as_bytes()).unwrap();
    }
    String::from_utf8(buf).unwrap_or_default()
}

fn base64_writer(w: &mut Vec<u8>) -> impl std::io::Write + '_ {
    struct B64Writer<'a>(&'a mut Vec<u8>);
    impl<'a> std::io::Write for B64Writer<'a> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            for chunk in buf.chunks(3) {
                let b0 = chunk[0] as usize;
                let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
                let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
                self.0.push(ALPHABET[b0 >> 2]);
                self.0.push(ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)]);
                if chunk.len() > 1 {
                    self.0.push(ALPHABET[((b1 & 0x0f) << 2) | (b2 >> 6)]);
                } else {
                    self.0.push(b'=');
                }
                if chunk.len() > 2 {
                    self.0.push(ALPHABET[b2 & 0x3f]);
                } else {
                    self.0.push(b'=');
                }
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    B64Writer(w)
}

/// Base64デコード
fn base64_decode(s: &str) -> Option<String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = Vec::new();
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != b'=').collect();

    for chunk in bytes.chunks(4) {
        if chunk.len() < 2 { break; }
        let b0 = ALPHABET.iter().position(|&c| c == chunk[0])? as u8;
        let b1 = ALPHABET.iter().position(|&c| c == chunk[1])? as u8;
        result.push((b0 << 2) | (b1 >> 4));
        if chunk.len() > 2 && chunk[2] != b'=' {
            let b2 = ALPHABET.iter().position(|&c| c == chunk[2])? as u8;
            result.push((b1 << 4) | (b2 >> 2));
            if chunk.len() > 3 && chunk[3] != b'=' {
                let b3 = ALPHABET.iter().position(|&c| c == chunk[3])? as u8;
                result.push((b2 << 6) | b3);
            }
        }
    }

    String::from_utf8(result).ok()
}

/// PDFに解析結果を埋め込む（コマンド）
#[tauri::command]
fn embed_pdf_result(path: String, result: String) -> Result<(), String> {
    embed_result_in_pdf(&path, &result)
}

/// PDFから解析結果を読み取る（コマンド）
#[tauri::command]
fn read_pdf_result(path: String) -> Option<(String, String)> {
    read_result_from_pdf(&path)
}

/// フォルダ内の全PDFから埋め込みデータを収集
fn collect_embedded_data_from_folder(folder: &str) -> Vec<(String, PdfEmbeddedData)> {
    let mut results = Vec::new();
    let folder_path = Path::new(folder);

    if let Ok(entries) = fs::read_dir(folder_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "pdf" || e == "PDF").unwrap_or(false) {
                if let Some(data) = read_embedded_data_from_pdf(&path.to_string_lossy()) {
                    let file_name = path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    results.push((file_name, data));
                }
            }
        }
    }

    results
}

/// ガイドラインを生成（Gemini使用）
#[tauri::command]
async fn generate_guidelines(app: AppHandle, paths: Vec<String>, folder: String, custom_instruction: Option<String>) -> Result<String, String> {
    // Collect embedded data from specified files only
    let mut collected: Vec<(String, PdfEmbeddedData)> = Vec::new();
    for path in &paths {
        if let Some(data) = read_embedded_data_from_pdf(path) {
            let file_name = Path::new(path).file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            collected.push((file_name, data));
        }
    }

    if collected.is_empty() {
        return Err("選択ファイルに解析データがありません".to_string());
    }

    emit_log(&app, &format!("=== ガイドライン生成 ({} ファイル) ===", collected.len()), "info");

    // Build context from collected data - focus on warnings/issues
    let mut all_issues: Vec<String> = Vec::new();
    let mut all_instructions: Vec<String> = Vec::new();

    // Add current custom instruction if provided
    if let Some(ref inst) = custom_instruction {
        if !inst.is_empty() {
            all_instructions.push(inst.clone());
        }
    }

    for (file_name, data) in &collected {
        // Extract warning lines (⚠, 警告, 不整合, 矛盾, エラー)
        let issues: Vec<&str> = data.result.lines()
            .filter(|line| {
                line.contains("⚠") || line.contains("警告") ||
                line.contains("不整合") || line.contains("矛盾") ||
                line.contains("注意") || line.contains("確認")
            })
            .collect();

        for issue in issues {
            let formatted = format!("[{}] {}", file_name, issue.trim());
            if !all_issues.contains(&formatted) {
                all_issues.push(formatted);
            }
        }

        if let Some(instruction) = &data.instruction {
            if !all_instructions.contains(instruction) {
                all_instructions.push(instruction.clone());
            }
        }
    }

    // Detect document types from file names
    let mut detected_types: Vec<String> = Vec::new();
    for (file_name, _) in &collected {
        for t in detect_document_type(file_name) {
            if !detected_types.contains(&t) {
                detected_types.push(t);
            }
        }
    }

    // Load existing guidelines
    let existing_guidelines = load_guidelines_json(&folder);
    let existing_json = existing_guidelines
        .as_ref()
        .map(|g| serde_json::to_string_pretty(g).unwrap_or_default())
        .unwrap_or_else(|| "（なし - 新規作成）".to_string());

    // Build prompt for guideline generation (JSON output)
    let prompt = format!(
        r#"あなたは書類チェックの専門家です。

既存のガイドラインを、新しいデータに基づいて改修してください。
既存の有用な項目は保持しつつ、新しいパターンを追加・統合してください。

## 既存のガイドライン
{}

## 今回検出された新しい問題・警告
{}

## ユーザーが重視しているチェック観点
{}

## 対象書類タイプ
{}

## タスク
1. 既存ガイドラインの有用な項目は保持
2. 新しい問題パターンがあれば追加
3. 重複は統合、古くなった項目は更新
4. 各カテゴリ最大10項目まで（重要度順）

## 出力形式（厳守）
JSON形式のみ出力。説明文不要。
項目は具体的に（「金額確認」ではなく「税込/税抜の混在に注意」のように）。

```json
{{
  "common": ["間違いパターン1", "パターン2"],
  "categories": {{
    "契約書": ["契約書で起きやすい間違い1"],
    "見積書": ["見積書で起きやすい間違い1"]
  }}
}}
```"#,
        existing_json,
        if all_issues.is_empty() {
            "（新規問題なし）".to_string()
        } else {
            all_issues.join("\n")
        },
        if all_instructions.is_empty() {
            "（なし）".to_string()
        } else {
            all_instructions.join("\n")
        },
        detected_types.join(", ")
    );

    emit_log(&app, "Geminiで要約中...", "wave");

    // Call Gemini
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let temp_dir = home_dir.join(".shoruichecker_temp_guidelines");
    fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;

    let prompt_file = temp_dir.join("prompt.txt");
    fs::write(&prompt_file, &prompt).map_err(|e| e.to_string())?;

    let gemini_path = std::env::var("APPDATA")
        .map(|p| format!("{}\\npm\\gemini.cmd", p))
        .unwrap_or_else(|_| "gemini".to_string());

    let model = load_settings().model.unwrap_or_else(|| DEFAULT_MODEL.to_string());

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

        // Extract JSON from response (may be wrapped in ```json ... ```)
        let json_str = if let Some(start) = result.find('{') {
            if let Some(end) = result.rfind('}') {
                &result[start..=end]
            } else {
                &result
            }
        } else {
            &result
        };

        // Parse and save as JSON
        let guidelines_path = get_guidelines_path(&folder);
        match serde_json::from_str::<Guidelines>(json_str) {
            Ok(guidelines) => {
                let json = serde_json::to_string_pretty(&guidelines).unwrap_or_default();
                let _ = fs::write(&guidelines_path, &json);

                let count = guidelines.common.len() +
                    guidelines.categories.values().map(|v| v.len()).sum::<usize>();
                emit_log(&app, &format!("✓ ガイドライン生成完了 ({} 項目)", count), "success");

                // Return human-readable summary
                let mut summary = String::from("## ガイドライン\n\n");
                if !guidelines.common.is_empty() {
                    summary.push_str("### 共通\n");
                    for item in &guidelines.common {
                        summary.push_str(&format!("- {}\n", item));
                    }
                }
                for (cat, items) in &guidelines.categories {
                    summary.push_str(&format!("\n### {}\n", cat));
                    for item in items {
                        summary.push_str(&format!("- {}\n", item));
                    }
                }
                Ok(summary)
            }
            Err(e) => {
                emit_log(&app, &format!("JSON解析エラー: {} - 生データ保存", e), "info");
                // Fallback: save raw result
                let _ = fs::write(&guidelines_path.with_extension("md"), &result);
                Ok(result)
            }
        }
    } else {
        let error = String::from_utf8_lossy(&output.stderr).to_string();
        emit_log(&app, &format!("エラー: {}", error), "error");
        Err(error)
    }
}

/// 起動時の解析対象ファイルを取得
#[tauri::command]
fn get_startup_file() -> Option<String> {
    std::env::var("ANALYZE_FILE").ok()
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

/// 全履歴を取得（フロントエンド用）
#[tauri::command]
fn get_all_history() -> Vec<AnalysisHistoryEntry> {
    let config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let history_dir = config_dir.join("shoruichecker").join("history");

    if !history_dir.exists() {
        return vec![];
    }

    let mut all_entries: Vec<AnalysisHistoryEntry> = vec![];

    if let Ok(entries) = fs::read_dir(&history_dir) {
        for entry in entries.flatten() {
            if entry.path().extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(content) = fs::read_to_string(entry.path()) {
                    if let Ok(history) = serde_json::from_str::<AnalysisHistory>(&content) {
                        all_entries.extend(history.entries);
                    }
                }
            }
        }
    }

    // Sort by analyzed_at descending
    all_entries.sort_by(|a, b| b.analyzed_at.cmp(&a.analyzed_at));
    all_entries
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

/// ガイドラインをJSON形式で保存（カテゴリ別）
#[derive(Clone, Serialize, Deserialize, Default)]
struct Guidelines {
    /// 書類タイプ別のチェックポイント
    categories: std::collections::HashMap<String, Vec<String>>,
    /// 共通の注意事項（短いもののみ）
    common: Vec<String>,
}

/// ファイル名から書類タイプを推定
fn detect_document_type(file_name: &str) -> Vec<String> {
    let name = file_name.to_lowercase();
    let mut types = Vec::new();

    if name.contains("契約") || name.contains("contract") {
        types.push("契約書".to_string());
    }
    if name.contains("見積") || name.contains("estimate") {
        types.push("見積書".to_string());
    }
    if name.contains("請求") || name.contains("invoice") {
        types.push("請求書".to_string());
    }
    if name.contains("交通誘導") || name.contains("配置") || name.contains("警備") {
        types.push("交通誘導員".to_string());
    }
    if name.contains("測量") || name.contains("横断") || name.contains("縦断") {
        types.push("測量図面".to_string());
    }
    if name.contains("施工") || name.contains("計画") {
        types.push("施工計画".to_string());
    }

    types
}

/// ガイドラインファイルのパス
fn get_guidelines_path(folder: &str) -> PathBuf {
    Path::new(folder).join(".guidelines.json")
}

/// ガイドラインを読み込む
fn load_guidelines_json(folder: &str) -> Option<Guidelines> {
    let path = get_guidelines_path(folder);
    fs::read_to_string(&path).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

/// ファイルに関連するガイドラインだけを取得
fn get_relevant_guidelines(folder: &str, file_name: &str) -> Option<String> {
    let guidelines = load_guidelines_json(folder)?;
    let doc_types = detect_document_type(file_name);

    let mut relevant = Vec::new();

    // 共通事項は常に含める（短いので）
    if !guidelines.common.is_empty() {
        relevant.push("【共通】".to_string());
        relevant.extend(guidelines.common.iter().take(5).cloned());
    }

    // 該当カテゴリのガイドラインだけ追加
    for doc_type in &doc_types {
        if let Some(items) = guidelines.categories.get(doc_type) {
            relevant.push(format!("【{}】", doc_type));
            relevant.extend(items.iter().take(5).cloned());
        }
    }

    if relevant.is_empty() {
        None
    } else {
        Some(relevant.join("\n"))
    }
}

/// 単一PDFを解析する内部関数
fn analyze_single_pdf(path: &str, task_id: &str, model: &str, custom_instruction: &str) -> Result<String, String> {
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

    // Load relevant guidelines only (based on file name)
    let guidelines_section = get_relevant_guidelines(&project_folder, &file_name)
        .map(|g| format!("\n## 該当ガイドライン\n{}\n", g))
        .unwrap_or_default();

    // Build custom instruction section
    let custom_section = if custom_instruction.is_empty() {
        String::new()
    } else {
        format!("\n## ユーザー指定のチェック項目\n以下の項目も必ず確認してください：\n{}\n", custom_instruction)
    };

    // Create temp directory for this task
    let home_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let temp_dir = home_dir.join(format!(".shoruichecker_temp_{}", task_id));
    fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;

    // Copy PDF to temp directory
    let dest_path = temp_dir.join(&file_name);
    fs::copy(path, &dest_path).map_err(|e| format!("ファイルコピーエラー: {}", e))?;

    // Build prompt with history context and custom instruction
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
{}
## 出力形式
- まず書類タイプを判定して報告
- 整合している項目は「✓」で示す
- 問題がある項目は「⚠」で具体的に指摘
- 過去の解析履歴がある場合、それとの整合性も確認すること
{}{}
ファイル: {}"#,
        guidelines_section,
        custom_section,
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

        // Embed result and custom instruction in PDF metadata (optional, ignore errors)
        let _ = embed_result_in_pdf_with_instruction(path, &result, custom_instruction);

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
fn analyze_compare_pdfs(paths: &[String], model: &str, custom_instruction: &str) -> Result<String, String> {
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

    // Load relevant guidelines for all files
    let mut all_types: Vec<String> = Vec::new();
    for path in paths {
        let name = Path::new(path).file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        for t in detect_document_type(&name) {
            if !all_types.contains(&t) {
                all_types.push(t);
            }
        }
    }
    let guidelines_section = if let Some(guidelines) = load_guidelines_json(&project_folder) {
        let mut relevant = Vec::new();
        if !guidelines.common.is_empty() {
            relevant.push("【共通】".to_string());
            relevant.extend(guidelines.common.iter().take(5).cloned());
        }
        for doc_type in &all_types {
            if let Some(items) = guidelines.categories.get(doc_type) {
                relevant.push(format!("【{}】", doc_type));
                relevant.extend(items.iter().take(5).cloned());
            }
        }
        if relevant.is_empty() {
            String::new()
        } else {
            format!("\n## 該当ガイドライン\n{}\n", relevant.join("\n"))
        }
    } else {
        String::new()
    };

    // Build custom instruction section
    let custom_section = if custom_instruction.is_empty() {
        String::new()
    } else {
        format!("\n## ユーザー指定のチェック項目\n以下の項目も必ず確認してください：\n{}\n", custom_instruction)
    };

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

    // Build comparison prompt with history and custom instruction
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
{}
## 出力形式
1. 各書類の概要を簡潔に説明
2. 書類間で整合している項目は「✓」で示す
3. 不整合や矛盾がある項目は「⚠」で具体的に指摘
4. 総合判定（整合/要確認/不整合）
{}{}"#,
        file_names.join("\n"),
        guidelines_section,
        custom_section,
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

        // Embed comparison result and instruction in all related PDFs
        for path in paths {
            let _ = embed_result_in_pdf_with_instruction(path, &result, custom_instruction);
        }

        Ok(result)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// PDFを解析 (Gemini CLI使用)
#[tauri::command]
async fn analyze_pdfs(app: AppHandle, paths: Vec<String>, mode: String, custom_instruction: Option<String>) -> Result<String, String> {
    if paths.is_empty() {
        return Err("ファイルが指定されていません".to_string());
    }

    let total = paths.len();
    let model = load_settings().model.unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let custom = custom_instruction.unwrap_or_default();

    // 照合モード
    if mode == "compare" {
        emit_log(&app, &format!("=== PDF照合解析開始 ({} ファイル) ===", total), "info");
        for path in &paths {
            let file_name = Path::new(path).file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown.pdf".to_string());
            emit_log(&app, &format!("  - {}", file_name), "info");
        }
        if !custom.is_empty() {
            emit_log(&app, &format!("カスタム指示: {}", custom.lines().next().unwrap_or("")), "info");
        }
        emit_log(&app, &format!("{} で照合中...", model), "wave");

        match analyze_compare_pdfs(&paths, &model, &custom) {
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
        if !custom.is_empty() {
            emit_log(&app, &format!("カスタム指示: {}", custom.lines().next().unwrap_or("")), "info");
        }

        if total == 1 {
            let path = &paths[0];
            let file_name = Path::new(path).file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown.pdf".to_string());

            emit_log(&app, &format!("{} を解析中...", file_name), "wave");

            match analyze_single_pdf(path, "single", &model, &custom) {
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
                let custom_clone = custom.clone();
                let task_id = format!("task_{}", i);
                let app_clone = app.clone();
                let file_name = Path::new(&path).file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("file_{}.pdf", i));

                let handle = thread::spawn(move || {
                    let result = analyze_single_pdf(&path, &task_id, &model_clone, &custom_clone);
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

/// ヘッドレスモード: GUIなしでPDFを解析
pub fn analyze_headless(path: &str) -> Result<(), String> {
    let model = load_settings().model.unwrap_or_else(|| DEFAULT_MODEL.to_string());

    println!("解析中: {}", path);

    match analyze_single_pdf(path, "headless", &model, "") {
        Ok(result) => {
            println!("\n{}", result);
            println!("\n✓ 結果をPDFに埋め込みました");
            Ok(())
        }
        Err(e) => {
            eprintln!("解析エラー: {}", e);
            Err(e)
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
            get_startup_file,
            get_watch_folder,
            set_watch_folder,
            stop_watching,
            open_gemini_auth,
            check_gemini_auth,
            get_model,
            set_model,
            get_all_history,
            embed_pdf_result,
            read_pdf_result,
            generate_guidelines
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
