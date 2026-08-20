//! Reasoning verification via real API calls.
//!
//! Verifies that reasoning parameters actually produce reasoning tokens,
//! recording token usage for cost estimation.

use crate::error::AppResult;
use crate::types::ReasoningConfidence;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResult {
    pub model: String,
    pub verified: bool,
    pub confidence: ReasoningConfidence,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
    pub message: String,
}

/// Verify reasoning capability by making a real API call and checking for
/// reasoning tokens in the response.
pub async fn verify(
    base_url: &str,
    api_key: &str,
    model: &str,
    effort: &str,
    accept_invalid_certs: bool,
) -> AppResult<VerificationResult> {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(accept_invalid_certs)
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let url = crate::protocol::normalize_url(base_url);
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "Explain why 17 is prime. Show your reasoning."}],
        "max_tokens": 200,
        "reasoning_effort": effort,
    });

    let resp = client
        .post(format!("{url}/v1/chat/completions"))
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        return Ok(VerificationResult {
            model: model.into(),
            verified: false,
            confidence: ReasoningConfidence::Unknown,
            input_tokens: 0,
            output_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 0,
            message: format!("验证请求失败 HTTP {status}"),
        });
    }

    let json: serde_json::Value = resp.json().await?;
    let usage = json.get("usage").cloned().unwrap_or_default();

    let input_tokens = usage.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let output_tokens = usage.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let reasoning_tokens = usage
        .pointer("/completion_tokens_details/reasoning_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0);

    let verified = reasoning_tokens > 0;
    let confidence = if verified {
        ReasoningConfidence::Verified
    } else {
        ReasoningConfidence::Declared
    };

    Ok(VerificationResult {
        model: model.into(),
        verified,
        confidence,
        input_tokens,
        output_tokens,
        reasoning_tokens,
        total_tokens,
        message: if verified {
            format!("推理验证成功：{reasoning_tokens} 推理 tokens")
        } else {
            "未检测到推理 tokens".into()
        },
    })
}
