//! History management module for ShoruiChecker
//!
//! This module handles the storage and retrieval of PDF analysis history,
//! organized by project folder.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use chrono::Local;
use serde::{Deserialize, Serialize};

/// Analysis history entry for a single file
#[derive(Clone, Serialize, Deserialize)]
pub struct AnalysisHistoryEntry {
    pub file_name: String,
    pub file_path: String,
    pub analyzed_at: String,
    pub document_type: Option<String>,
    pub summary: String,
    pub issues: Vec<String>,
}

/// Analysis history for a project folder
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct AnalysisHistory {
    pub project_folder: String,
    pub entries: Vec<AnalysisHistoryEntry>,
}

/// Get the history file path for a project folder
///
/// The history is stored in the user's config directory under
/// `shoruichecker/history/{folder_hash}.json`
pub fn get_history_path(project_folder: &str) -> PathBuf {
    let config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    let folder_hash = format!("{:x}", path_hash(project_folder));
    config_dir
        .join("shoruichecker")
        .join("history")
        .join(format!("{}.json", folder_hash))
}

/// Simple hash function to generate a unique filename from a folder path
///
/// Uses DefaultHasher to create a deterministic hash from the folder path string.
pub fn path_hash(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Load analysis history for a project folder
///
/// Returns an empty history if the file doesn't exist or can't be parsed.
pub fn load_history(project_folder: &str) -> AnalysisHistory {
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

/// Save analysis history to disk
///
/// Creates the history directory if it doesn't exist.
pub fn save_history(history: &AnalysisHistory) -> Result<(), String> {
    let path = get_history_path(&history.project_folder);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(history).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(())
}

/// Create a history entry from analysis results
///
/// Extracts document type, issues, and summary from the analysis result text.
pub fn create_history_entry(file_name: &str, file_path: &str, result: &str) -> AnalysisHistoryEntry {
    // Extract document type from result (simple parsing)
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

    // Extract issues (lines with warning markers)
    let issues: Vec<String> = result
        .lines()
        .filter(|line| {
            line.contains("⚠")
                || line.contains("警告")
                || line.contains("不整合")
                || line.contains("矛盾")
        })
        .map(|s| s.trim().to_string())
        .collect();

    // Create summary (first few lines)
    let summary: String = result.lines().take(10).collect::<Vec<_>>().join("\n");

    AnalysisHistoryEntry {
        file_name: file_name.to_string(),
        file_path: file_path.to_string(),
        analyzed_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        document_type,
        summary,
        issues,
    }
}

/// Build context string from history for use in prompts
///
/// Returns an empty string if history is empty.
/// Otherwise, returns a formatted string with the last 10 entries.
pub fn build_history_context(history: &AnalysisHistory) -> String {
    if history.entries.is_empty() {
        return String::new();
    }

    let mut context = String::from("\n\n## 過去の解析履歴（参考情報）\n");
    context.push_str(
        "以下は同じプロジェクトで過去に解析した書類の情報です。整合性チェック時に参照してください。\n\n",
    );

    for entry in history.entries.iter().rev().take(10) {
        context.push_str(&format!(
            "### {} ({})\n",
            entry.file_name, entry.analyzed_at
        ));
        if let Some(doc_type) = &entry.document_type {
            context.push_str(&format!("- 書類タイプ: {}\n", doc_type));
        }
        if !entry.issues.is_empty() {
            context.push_str("- 検出された問題:\n");
            for issue in &entry.issues {
                context.push_str(&format!("  - {}\n", issue));
            }
        }
        context.push_str(&format!(
            "- 要約: {}\n\n",
            entry.summary.lines().take(3).collect::<Vec<_>>().join(" ")
        ));
    }

    context
}

/// 全履歴を取得（フロントエンド用）
#[tauri::command]
pub fn get_all_history() -> Vec<AnalysisHistoryEntry> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_hash() {
        let hash1 = path_hash("test/folder/path");
        let hash2 = path_hash("test/folder/path");
        let hash3 = path_hash("different/path");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_create_history_entry() {
        let entry = create_history_entry(
            "test.pdf",
            "/path/to/test.pdf",
            "契約書の内容です\n⚠ 金額に不整合があります",
        );

        assert_eq!(entry.file_name, "test.pdf");
        assert_eq!(entry.document_type, Some("契約書".to_string()));
        assert!(!entry.issues.is_empty());
    }

    #[test]
    fn test_build_history_context_empty() {
        let history = AnalysisHistory {
            project_folder: "test".to_string(),
            entries: vec![],
        };

        let context = build_history_context(&history);
        assert!(context.is_empty());
    }
}
