//! Reasoning verification via real API calls.
//!
//! Verifies that reasoning parameters actually produce reasoning tokens,
//! recording token usage for cost estimation.

use crate::error::AppResult;
use crate::types::{ReasoningConfidence, ThinkingSupport};
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

    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let reasoning_tokens = usage
        .pointer("/completion_tokens_details/reasoning_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

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

/// Classify the thinking blocks in a non-streaming `/v1/messages` response body.
///
/// Split out from the HTTP call so the signature rule is testable without a
/// network. A block counts as signed only if `signature` is present *and*
/// non-empty: some relays echo the field back as `""`, which the client rejects
/// exactly as it rejects a missing one.
pub fn classify_thinking_blocks(body: &serde_json::Value) -> ThinkingSupport {
    let Some(content) = body.get("content").and_then(|c| c.as_array()) else {
        return ThinkingSupport::Absent;
    };
    let thinking: Vec<&serde_json::Value> = content
        .iter()
        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("thinking"))
        .collect();
    if thinking.is_empty() {
        return ThinkingSupport::Absent;
    }
    let all_signed = thinking.iter().all(|b| {
        b.get("signature")
            .and_then(|s| s.as_str())
            .is_some_and(|s| !s.trim().is_empty())
    });
    if all_signed {
        ThinkingSupport::Signed
    } else {
        ThinkingSupport::Unsigned
    }
}

/// Probe whether an upstream returns *signed* Anthropic thinking blocks.
///
/// This is the only question that licenses thinking injection, and it is a
/// different question from [`verify`], which measures the OpenAI
/// `reasoning_effort` path. An upstream can pass that and still return unsigned
/// thinking here.
///
/// Deliberately non-streaming: in a stream the signature arrives as a trailing
/// `signature_delta`, so a truncated read looks identical to an unsigned block.
pub async fn probe_anthropic_thinking(
    base_url: &str,
    api_key: &str,
    model: &str,
    accept_invalid_certs: bool,
) -> AppResult<ThinkingSupport> {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(accept_invalid_certs)
        .timeout(std::time::Duration::from_secs(60))
        .build()?;

    let url = crate::protocol::normalize_url(base_url);
    // budget_tokens must be under max_tokens, and both over the 1024 floor the
    // Anthropic API enforces on extended thinking.
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 2048,
        "stream": false,
        "thinking": { "type": "enabled", "budget_tokens": 1024 },
        "messages": [{"role": "user", "content": "Explain why 17 is prime. Show your reasoning."}],
    });

    let resp = client
        .post(format!("{url}/v1/messages"))
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        // A rejected request is not evidence of unsigned thinking, but it is also
        // not permission to inject. Unprobed keeps the gate closed and keeps the
        // "never established" meaning intact.
        return Ok(ThinkingSupport::Unprobed);
    }

    let json: serde_json::Value = resp.json().await?;
    Ok(classify_thinking_blocks(&json))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(content: serde_json::Value) -> serde_json::Value {
        serde_json::json!({ "content": content })
    }

    #[test]
    fn signed_thinking_is_injectable() {
        let got = classify_thinking_blocks(&body(serde_json::json!([
            {"type": "thinking", "thinking": "...", "signature": "abc123"},
            {"type": "text", "text": "17 is prime."}
        ])));
        assert_eq!(got, ThinkingSupport::Signed);
        assert!(got.is_injectable());
    }

    #[test]
    fn thinking_without_signature_is_unsigned() {
        // The Agnes shape: thinking present, signature absent. This is the case
        // that broke the client and the case the old probe scored as supported.
        let got = classify_thinking_blocks(&body(serde_json::json!([
            {"type": "thinking", "thinking": "..."},
            {"type": "text", "text": "17 is prime."}
        ])));
        assert_eq!(got, ThinkingSupport::Unsigned);
        assert!(!got.is_injectable());
    }

    #[test]
    fn empty_signature_counts_as_unsigned() {
        let got = classify_thinking_blocks(&body(serde_json::json!([
            {"type": "thinking", "thinking": "...", "signature": "   "}
        ])));
        assert_eq!(got, ThinkingSupport::Unsigned);
    }

    #[test]
    fn one_unsigned_block_taints_the_whole_response() {
        let got = classify_thinking_blocks(&body(serde_json::json!([
            {"type": "thinking", "thinking": "a", "signature": "sig"},
            {"type": "thinking", "thinking": "b"}
        ])));
        assert_eq!(got, ThinkingSupport::Unsigned);
    }

    #[test]
    fn no_thinking_block_is_absent() {
        let got = classify_thinking_blocks(&body(serde_json::json!([
            {"type": "text", "text": "17 is prime."}
        ])));
        assert_eq!(got, ThinkingSupport::Absent);
        assert!(!got.is_injectable());
    }

    #[test]
    fn missing_content_is_absent() {
        assert_eq!(
            classify_thinking_blocks(&serde_json::json!({})),
            ThinkingSupport::Absent
        );
    }

    #[test]
    fn unprobed_is_not_injectable() {
        assert!(!ThinkingSupport::default().is_injectable());
        assert_eq!(ThinkingSupport::default(), ThinkingSupport::Unprobed);
    }
}
