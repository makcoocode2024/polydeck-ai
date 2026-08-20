//! Reasoning capability discovery engine.
//!
//! Automatically discovers whether a model supports extended reasoning (o1/o3/
//! Gemini Thinking etc.) using a 4-level confidence system: Unknown → Declared
//! → Validated → Verified.

use crate::error::AppResult;
use crate::types::ReasoningConfidence;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningDiscovery {
    pub model: String,
    pub supports_reasoning: bool,
    pub confidence: ReasoningConfidence,
    pub effort_levels: Vec<String>,
    pub evidence: Vec<String>,
    pub provider_type: String,
}

/// Discover reasoning capabilities for a model via real HTTP probing.
pub async fn discover(
    base_url: &str,
    api_key: &str,
    model: &str,
    accept_invalid_certs: bool,
) -> AppResult<ReasoningDiscovery> {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(accept_invalid_certs)
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let url = crate::protocol::normalize_url(base_url);
    let mut evidence = Vec::new();
    let mut supports = false;
    let mut confidence = ReasoningConfidence::Unknown;
    let mut effort_levels = Vec::new();

    // Phase 1: Check model name patterns (Declared confidence)
    let model_lower = model.to_lowercase();
    let reasoning_patterns = ["o1", "o3", "o4-mini", "thinking", "deepseek-reasoner"];
    if reasoning_patterns.iter().any(|p| model_lower.contains(p)) {
        supports = true;
        confidence = ReasoningConfidence::Declared;
        evidence.push(format!("模型名称 {model} 匹配推理模式"));
    }

    // Phase 2: Probe with reasoning_effort parameter (Validated)
    let probe_body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "Reply OK"}],
        "max_tokens": 10,
        "reasoning_effort": "low",
    });

    if let Ok(resp) = client
        .post(format!("{url}/v1/chat/completions"))
        .bearer_auth(api_key)
        .json(&probe_body)
        .send()
        .await
    {
        if resp.status().is_success() {
            supports = true;
            if confidence < ReasoningConfidence::Validated {
                confidence = ReasoningConfidence::Validated;
            }
            effort_levels = vec!["low".into(), "medium".into(), "high".into()];
            evidence.push("reasoning_effort=low 参数被接受".into());
        } else {
            let status = resp.status();
            evidence.push(format!("reasoning_effort 探测返回 HTTP {status}"));
        }
    }

    // Phase 3: Check for reasoning tokens in response (Verified)
    if supports {
        let verify_body = serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": "What is 2+2? Think step by step."}],
            "max_tokens": 100,
            "reasoning_effort": "low",
        });

        if let Ok(resp) = client
            .post(format!("{url}/v1/chat/completions"))
            .bearer_auth(api_key)
            .json(&verify_body)
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    let has_reasoning_tokens = json
                        .pointer("/usage/completion_tokens_details/reasoning_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0)
                        > 0;
                    if has_reasoning_tokens {
                        confidence = ReasoningConfidence::Verified;
                        evidence.push("响应包含 reasoning_tokens > 0".into());
                    }
                }
            }
        }
    }

    let provider_type = if url.contains("anthropic") {
        "anthropic"
    } else if url.contains("googleapis") || url.contains("generativelanguage") {
        "gemini"
    } else {
        "openai"
    };

    Ok(ReasoningDiscovery {
        model: model.to_string(),
        supports_reasoning: supports,
        confidence,
        effort_levels,
        evidence,
        provider_type: provider_type.to_string(),
    })
}
