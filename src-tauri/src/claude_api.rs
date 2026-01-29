//! Claude API integration for document analysis

use reqwest::Client;
use serde::{Deserialize, Serialize};

const CLAUDE_API_URL: &str = "https://api.anthropic.com/v1/messages";
const MODEL: &str = "claude-sonnet-4-20250514";

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<Message>,
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    text: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AnalysisResult {
    pub status: String,
    pub message: String,
    pub details: Option<String>,
}

/// Analyze a document using Claude API
pub async fn analyze_document(text: &str) -> Result<AnalysisResult, String> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY not set")?;

    let prompt = format!(
        r#"あなたは建設工事の書類チェッカーです。以下の文書内容を分析し、問題点や不整合を指摘してください。

## 文書内容:
{}

## 確認項目:
1. 日付の整合性（作成日、提出日など）
2. 数値の妥当性（数量、金額など）
3. 記載漏れや空欄
4. 書式の問題
5. その他の不整合

## 回答形式:
以下のJSON形式で回答してください:
{{
  "status": "ok" または "warning" または "error",
  "message": "簡潔な結果サマリー",
  "details": "詳細な指摘事項（あれば）"
}}"#,
        text
    );

    let client = Client::new();
    let request = ApiRequest {
        model: MODEL.to_string(),
        max_tokens: 2048,
        messages: vec![Message {
            role: "user".to_string(),
            content: prompt,
        }],
    };

    let response = client
        .post(CLAUDE_API_URL)
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        return Err(format!("API error {}: {}", status, error_text));
    }

    let api_response: ApiResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let response_text = api_response
        .content
        .first()
        .and_then(|c| c.text.as_ref())
        .ok_or("Empty response")?;

    // Parse JSON from response
    parse_analysis_result(response_text)
}

fn parse_analysis_result(text: &str) -> Result<AnalysisResult, String> {
    // Try to find JSON in the response
    let json_start = text.find('{');
    let json_end = text.rfind('}');

    if let (Some(start), Some(end)) = (json_start, json_end) {
        let json_str = &text[start..=end];
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
            return Ok(AnalysisResult {
                status: parsed["status"].as_str().unwrap_or("unknown").to_string(),
                message: parsed["message"].as_str().unwrap_or("").to_string(),
                details: parsed["details"].as_str().map(|s| s.to_string()),
            });
        }
    }

    // Fallback if JSON parsing fails
    Ok(AnalysisResult {
        status: "ok".to_string(),
        message: text.chars().take(200).collect(),
        details: Some(text.to_string()),
    })
}
