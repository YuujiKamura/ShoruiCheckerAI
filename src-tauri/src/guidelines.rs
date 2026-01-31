use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
use crate::CREATE_NO_WINDOW;

use crate::events::emit_log;
use crate::pdf_embed::{read_embedded_data_from_pdf, PdfEmbeddedData};
use crate::settings::{load_settings, DEFAULT_MODEL};

/// ガイドラインをJSON形式で保存（カテゴリ別）
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Guidelines {
    /// 書類タイプ別のチェックポイント
    pub categories: HashMap<String, Vec<String>>,
    /// 共通の注意事項（短いもののみ）
    pub common: Vec<String>,
}

/// ファイル名から書類タイプを推定
pub fn detect_document_type(file_name: &str) -> Vec<String> {
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
pub fn get_guidelines_path(folder: &str) -> PathBuf {
    Path::new(folder).join(".guidelines.json")
}

/// ガイドラインを読み込む
pub fn load_guidelines_json(folder: &str) -> Option<Guidelines> {
    let path = get_guidelines_path(folder);
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

/// ファイルに関連するガイドラインだけを取得
pub fn get_relevant_guidelines(folder: &str, file_name: &str) -> Option<String> {
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

/// ガイドラインを生成（Gemini使用）
#[tauri::command]
pub async fn generate_guidelines(
    app: AppHandle,
    paths: Vec<String>,
    folder: String,
    custom_instruction: Option<String>,
) -> Result<String, String> {
    // Collect embedded data from specified files only
    let mut collected: Vec<(String, PdfEmbeddedData)> = Vec::new();
    for path in &paths {
        if let Some(data) = read_embedded_data_from_pdf(path) {
            let file_name = Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            collected.push((file_name, data));
        }
    }

    if collected.is_empty() {
        return Err("選択ファイルに解析データがありません".to_string());
    }

    emit_log(
        &app,
        &format!("=== ガイドライン生成 ({} ファイル) ===", collected.len()),
        "info",
    );

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
        let issues: Vec<&str> = data
            .result
            .lines()
            .filter(|line| {
                line.contains("⚠")
                    || line.contains("警告")
                    || line.contains("不整合")
                    || line.contains("矛盾")
                    || line.contains("注意")
                    || line.contains("確認")
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

    let model = load_settings()
        .model
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

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

                let count = guidelines.common.len()
                    + guidelines.categories.values().map(|v| v.len()).sum::<usize>();
                emit_log(
                    &app,
                    &format!("✓ ガイドライン生成完了 ({} 項目)", count),
                    "success",
                );

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
