//! Gateway configuration types

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub listen_addr: Option<SocketAddr>,
    pub upstream: UpstreamConfig,
    pub model_rewrites: Vec<ModelRewriteRule>,
    #[serde(default = "default_timeout")]
    pub timeout: Duration,
    #[serde(default = "default_retries")]
    pub max_retries: u32,
}

fn default_timeout() -> Duration {
    Duration::from_secs(120)
}
fn default_retries() -> u32 {
    3
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResponsesMode {
    #[default]
    Auto,
    Native,
    Bridge,
}

impl ResponsesMode {
    pub fn from_protocol(protocol: &str) -> Self {
        match protocol.trim().to_ascii_lowercase().as_str() {
            "responses" | "openai_responses" | "native_responses" => Self::Native,
            "chat_completions" | "openai_chat_completions" => Self::Bridge,
            _ => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamConfig {
    #[serde(default)]
    pub provider_id: Option<String>,
    pub base_url: String,
    pub api_key: String,
    pub protocol: String,
    pub local_token: String,
    #[serde(default)]
    pub max_price_per_request: Option<f64>,
    #[serde(default)]
    pub responses_mode: ResponsesMode,
    #[serde(default)]
    pub rate_limit: polydeck_core::profile::RateLimitSettings,
    #[serde(default)]
    pub default_effort_level: Option<String>,
}

/// How `ModelRewriteRule::from` should be interpreted.
///
/// Model names routinely contain regex metacharacters (`claude-opus-5[1m]`,
/// `glm-4.6`), so guessing from the string alone misclassifies them. Generated
/// rules always state their kind; `Auto` only exists for hand-written configs
/// that predate this field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    #[default]
    Auto,
    Literal,
    Regex,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRewriteRule {
    pub from: String,
    pub to: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub description: Option<String>,
    #[serde(default)]
    pub match_kind: MatchKind,
}

fn default_true() -> bool {
    true
}

impl ModelRewriteRule {
    pub fn exact(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            enabled: true,
            description: None,
            match_kind: MatchKind::Literal,
        }
    }
    pub fn regex(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            enabled: true,
            description: None,
            match_kind: MatchKind::Regex,
        }
    }
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_exact_rule() {
        let rule = ModelRewriteRule::exact("claude-sonnet-4-5", "glm-5.2");
        assert_eq!(rule.from, "claude-sonnet-4-5");
        assert_eq!(rule.to, "glm-5.2");
        assert!(rule.enabled);
    }

    #[test]
    fn responses_markers_select_passthrough() {
        for m in ["responses", "openai_responses", "native_responses"] {
            assert_eq!(ResponsesMode::from_protocol(m), ResponsesMode::Native);
        }
    }

    #[test]
    fn chat_completions_markers_select_conversion() {
        for m in ["chat_completions", "OpenAI_Chat_Completions"] {
            assert_eq!(ResponsesMode::from_protocol(m), ResponsesMode::Bridge);
        }
    }

    #[test]
    fn dialect_markers_default_to_auto() {
        for m in ["openai", "anthropic", "gemini", "azure", ""] {
            assert_eq!(ResponsesMode::from_protocol(m), ResponsesMode::Auto);
        }
    }
}
