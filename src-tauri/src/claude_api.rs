//! Claude Code CLI integration for document analysis

use std::process::Command;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct AnalysisResult {
    pub status: String,
    pub message: String,
    pub details: Option<String>,
}

/// ガイドラインファイルのデフォルトパス
fn get_guidelines_path() -> Option<String> {
    // アプリのデータディレクトリにあるガイドラインファイル
    let data_dir = dirs::data_local_dir()?
        .join("ShoruiChecker")
        .join("guidelines.md");

    if data_dir.exists() {
        Some(data_dir.to_string_lossy().to_string())
    } else {
        None
    }
}

/// Claude Code CLIを使って文書を解析
pub async fn analyze_document(text: &str) -> Result<AnalysisResult, String> {
    analyze_document_with_file(text, None).await
}

/// Claude Code CLIを使って文書を解析（ガイドラインファイル指定可能）
pub async fn analyze_document_with_file(text: &str, guidelines_path: Option<&str>) -> Result<AnalysisResult, String> {
    // ガイドラインファイルの参照を構築
    let guidelines_ref = guidelines_path
        .map(|p| p.to_string())
        .or_else(get_guidelines_path);

    let prompt = if let Some(ref guide_path) = guidelines_ref {
        if Path::new(guide_path).exists() {
            format!(
                "以下の文書の整合性を検証してください。ガイドライン: {}\n\n---\n{}",
                guide_path, text
            )
        } else {
            format!("以下の文書の整合性を検証してください。\n\n---\n{}", text)
        }
    } else {
        format!("以下の文書の整合性を検証してください。\n\n---\n{}", text)
    };

    // Claude Code CLI を呼び出し
    let output = Command::new("claude")
        .args(["-p", &prompt, "--output-format", "text"])
        .output()
        .map_err(|e| format!("Claude CLI実行エラー: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Claude CLI失敗: {}", stderr));
    }

    let response = String::from_utf8_lossy(&output.stdout).to_string();

    // レスポンスからステータスを推定
    let status = detect_status(&response);

    Ok(AnalysisResult {
        status,
        message: response.lines().next().unwrap_or("検証完了").to_string(),
        details: Some(response),
    })
}

/// レスポンスからステータスを推定
fn detect_status(response: &str) -> String {
    let lower = response.to_lowercase();

    if lower.contains("問題") || lower.contains("エラー") || lower.contains("不整合") || lower.contains("誤り") {
        if lower.contains("重大") || lower.contains("致命") {
            "error".to_string()
        } else {
            "warning".to_string()
        }
    } else if lower.contains("ok") || lower.contains("問題なし") || lower.contains("整合") {
        "ok".to_string()
    } else {
        "ok".to_string()  // デフォルトはOK
    }
}

/// ガイドラインファイルを作成/更新
pub fn save_guidelines(content: &str) -> Result<String, String> {
    let data_dir = dirs::data_local_dir()
        .ok_or("データディレクトリが見つかりません")?
        .join("ShoruiChecker");

    std::fs::create_dir_all(&data_dir)
        .map_err(|e| format!("ディレクトリ作成エラー: {}", e))?;

    let path = data_dir.join("guidelines.md");
    std::fs::write(&path, content)
        .map_err(|e| format!("ファイル書き込みエラー: {}", e))?;

    Ok(path.to_string_lossy().to_string())
}

/// ガイドラインファイルを読み込み
pub fn load_guidelines() -> Option<String> {
    let path = get_guidelines_path()?;
    std::fs::read_to_string(path).ok()
}
