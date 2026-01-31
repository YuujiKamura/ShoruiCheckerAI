use std::fs;
use std::path::Path;
use std::thread;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::events::emit_log;
use crate::gemini_cli::{cleanup_temp_dir, create_temp_dir, run_gemini_with_prompt};
use crate::guidelines::{detect_document_type, get_relevant_guidelines, load_guidelines_json};
use crate::history::{
    build_history_context, create_history_entry, load_history, save_history,
    AnalysisHistoryEntry,
};
use crate::pdf_embed::embed_result_in_pdf_with_instruction;
use crate::settings::{load_settings, DEFAULT_MODEL};

#[derive(Clone, Serialize)]
struct AnalysisResult {
    file_name: String,
    path: String,
    result: Option<String>,
    error: Option<String>,
}

/// 単一PDFを解析する内部関数
fn analyze_single_pdf(
    path: &str,
    task_id: &str,
    model: &str,
    custom_instruction: &str,
) -> Result<String, String> {
    let pdf_path = Path::new(path);
    let file_name = pdf_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown.pdf".to_string());

    // Get project folder (parent directory)
    let project_folder = pdf_path
        .parent()
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
        format!(
            "\n## ユーザー指定のチェック項目\n以下の項目も必ず確認してください：\n{}\n",
            custom_instruction
        )
    };

    // Create temp directory for this task
    let temp_dir = create_temp_dir(&format!(".shoruichecker_temp_{}", task_id))?;

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

    let pdfs = vec![file_name.clone()];
    let output = run_gemini_with_prompt(&temp_dir, &prompt, model, Some(&pdfs));
    cleanup_temp_dir(&temp_dir);

    match output {
        Ok(result) => {
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
        }
        Err(error) => Err(error),
    }
}

/// 複数PDFをまとめて照合解析
fn analyze_compare_pdfs(paths: &[String], model: &str, custom_instruction: &str) -> Result<String, String> {
    let temp_dir = create_temp_dir(".shoruichecker_temp_compare")?;

    // Get project folder from first file
    let project_folder = paths
        .first()
        .and_then(|p| Path::new(p).parent())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    // Load history
    let history = load_history(&project_folder);
    let history_context = build_history_context(&history);

    // Load relevant guidelines for all files
    let mut all_types: Vec<String> = Vec::new();
    for path in paths {
        let name = Path::new(path)
            .file_name()
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
        format!(
            "\n## ユーザー指定のチェック項目\n以下の項目も必ず確認してください：\n{}\n",
            custom_instruction
        )
    };

    // Copy all PDFs
    let mut copied_files: Vec<String> = Vec::new();
    let mut file_names: Vec<String> = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        let pdf_path = Path::new(path);
        let file_name = pdf_path
            .file_name()
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

    let output = run_gemini_with_prompt(&temp_dir, &prompt, model, Some(&file_names));
    cleanup_temp_dir(&temp_dir);

    match output {
        Ok(result) => {
            // Save comparison result to history for each file
            let mut history = load_history(&project_folder);
            let comparison_summary = format!("【照合解析】対象: {}", file_names.join(", "));
            for (i, path) in paths.iter().enumerate() {
                let file_name = &file_names[i];
                let entry = AnalysisHistoryEntry {
                    file_name: file_name.clone(),
                    file_path: path.clone(),
                    analyzed_at: chrono::Local::now()
                        .format("%Y-%m-%d %H:%M:%S")
                        .to_string(),
                    document_type: Some("照合解析".to_string()),
                    summary: comparison_summary.clone(),
                    issues: result
                        .lines()
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
        }
        Err(error) => Err(error),
    }
}

/// PDFを解析 (Gemini CLI使用)
#[tauri::command]
pub async fn analyze_pdfs(
    app: AppHandle,
    paths: Vec<String>,
    mode: String,
    custom_instruction: Option<String>,
) -> Result<String, String> {
    if paths.is_empty() {
        return Err("ファイルが指定されていません".to_string());
    }

    let total = paths.len();
    let model = load_settings()
        .model
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let custom = custom_instruction.unwrap_or_default();

    // 照合モード
    if mode == "compare" {
        emit_log(
            &app,
            &format!("=== PDF照合解析開始 ({} ファイル) ===", total),
            "info",
        );
        for path in &paths {
            let file_name = Path::new(path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "unknown.pdf".to_string());
            emit_log(&app, &format!("  - {}", file_name), "info");
        }
        if !custom.is_empty() {
            emit_log(
                &app,
                &format!("カスタム指示: {}", custom.lines().next().unwrap_or("")),
                "info",
            );
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
        emit_log(
            &app,
            &format!("=== PDF個別解析開始 ({} ファイル) ===", total),
            "info",
        );
        if !custom.is_empty() {
            emit_log(
                &app,
                &format!("カスタム指示: {}", custom.lines().next().unwrap_or("")),
                "info",
            );
        }

        if total == 1 {
            let path = &paths[0];
            let file_name = Path::new(path)
                .file_name()
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
            emit_log(
                &app,
                &format!("{} で {} ファイルを並列解析中...", model, total),
                "wave",
            );

            let mut handles = vec![];

            for (i, path) in paths.into_iter().enumerate() {
                let model_clone = model.clone();
                let custom_clone = custom.clone();
                let task_id = format!("task_{}", i);
                let app_clone = app.clone();
                let file_name = Path::new(&path)
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("file_{}.pdf", i));

                let handle = thread::spawn(move || {
                    let result = analyze_single_pdf(&path, &task_id, &model_clone, &custom_clone);
                    let _ = app_clone.emit(
                        "analysis-progress",
                        serde_json::json!({
                            "file_name": file_name.clone(),
                            "completed": true,
                            "success": result.is_ok()
                        }),
                    );
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

            emit_log(
                &app,
                &format!("✓ 解析完了 ({}/{})", success_count, total),
                "success",
            );
            Ok(output)
        }
    }
}

/// ヘッドレスモード: GUIなしでPDFを解析
pub fn analyze_headless(path: &str) -> Result<(), String> {
    let model = load_settings()
        .model
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

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
