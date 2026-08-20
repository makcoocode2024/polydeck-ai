//! API Key platform detection.
//!
//! Identifies the AI platform from key prefixes and suggests Base URL + default model.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct KeyDetection {
    pub platform: String,
    pub suggested_base_url: String,
    pub suggested_model: String,
    pub confidence: f32,
}

/// Detect the AI platform from an API key prefix.
pub fn detect(api_key: &str) -> Option<KeyDetection> {
    let key = api_key.trim();
    if key.len() < 3 {
        return None;
    }

    let detections: Vec<(&str, &str, &str, &str)> = vec![
        ("sk-", "OpenAI", "https://api.openai.com", "gpt-4o"),
        ("sk-ant-", "Anthropic", "https://api.anthropic.com", "claude-sonnet-4-20250514"),
        ("sk-or-", "OpenRouter", "https://openrouter.ai/api", "openai/gpt-4o"),
        ("xai-", "xAI", "https://api.x.ai", "grok-3"),
        ("AIza", "Google", "https://generativelanguage.googleapis.com", "gemini-2.5-flash"),
        ("sk-proj-", "OpenAI Project", "https://api.openai.com", "gpt-4o"),
        ("deepseek-", "DeepSeek", "https://api.deepseek.com", "deepseek-chat"),
        ("glm-", "智谱", "https://open.bigmodel.cn/api/paas", "glm-4-flash"),
    ];

    for (prefix, platform, base_url, model) in detections {
        if key.starts_with(prefix) {
            // Anthropic keys start with sk-ant-, must check before generic sk-
            if prefix == "sk-" && key.starts_with("sk-ant-") {
                continue;
            }
            if prefix == "sk-" && key.starts_with("sk-or-") {
                continue;
            }
            if prefix == "sk-" && key.starts_with("sk-proj-") {
                continue;
            }
            return Some(KeyDetection {
                platform: platform.to_string(),
                suggested_base_url: base_url.to_string(),
                suggested_model: model.to_string(),
                confidence: if prefix.len() > 3 { 0.95 } else { 0.7 },
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_anthropic_key() {
        let result = detect("sk-ant-api03-xxxx").unwrap();
        assert_eq!(result.platform, "Anthropic");
    }

    #[test]
    fn detects_openai_key() {
        let result = detect("sk-abc123def456").unwrap();
        assert_eq!(result.platform, "OpenAI");
    }

    #[test]
    fn detects_xai_key() {
        let result = detect("xai-abc123").unwrap();
        assert_eq!(result.platform, "xAI");
    }

    #[test]
    fn returns_none_for_unknown() {
        assert!(detect("unknown-key-format").is_none());
    }
}
