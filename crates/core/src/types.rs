//! Shared domain types used across the core crate.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Confidence level for protocol detection and reasoning discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Unknown,
    Low,
    Medium,
    High,
    Certain,
}

impl Confidence {
    pub fn is_usable(self) -> bool {
        matches!(self, Self::Medium | Self::High | Self::Certain)
    }
}

/// Detected AI protocol type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolKind {
    #[serde(rename = "openai")]
    #[serde(alias = "open_a_i")]
    #[ts(rename = "openai")]
    OpenAI,
    #[serde(rename = "responses")]
    #[serde(alias = "openai_responses")]
    #[ts(rename = "responses")]
    Responses,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "gemini")]
    Gemini,
    #[serde(rename = "azure")]
    Azure,
    #[serde(rename = "unknown")]
    Unknown,
}

impl std::fmt::Display for ProtocolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenAI => write!(f, "openai"),
            Self::Responses => write!(f, "responses"),
            Self::Anthropic => write!(f, "anthropic"),
            Self::Gemini => write!(f, "gemini"),
            Self::Azure => write!(f, "azure"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

impl ProtocolKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::Responses => "responses",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::Azure => "azure",
            Self::Unknown => "unknown",
        }
    }
}

/// Reasoning confidence level (4-tier from AI Deck).
///
/// This scale is an OpenAI-protocol signal: every level is set by probing
/// `/v1/chat/completions` with `reasoning_effort` and counting `reasoning_tokens`.
/// It says nothing about whether the upstream returns *signed* Anthropic thinking
/// blocks — see [`ThinkingSupport`] for that, and do not gate Anthropic behaviour
/// on this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningConfidence {
    Unknown,
    Declared,
    Validated,
    Verified,
}

/// Whether an upstream returns Anthropic thinking blocks a client can actually use.
///
/// Extended thinking is only usable if each thinking block carries a `signature`.
/// Without one the client cannot persist the assistant turn or replay it, so it
/// fails the turn outright — which then strands the turn's `tool_use` with no
/// `tool_result` and poisons the rest of the session. An upstream can therefore
/// return thinking and still be unusable, which is why `Unsigned` is distinct
/// from `Absent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingSupport {
    /// Never probed. Treated as unsupported, because injecting on a guess is the
    /// expensive direction of the error.
    #[default]
    Unprobed,
    /// Thinking blocks come back with a non-empty `signature`. Safe to inject.
    Signed,
    /// Thinking blocks come back, but with no `signature`. Must not inject.
    Unsigned,
    /// The upstream accepted the request but returned no thinking blocks.
    Absent,
}

impl ThinkingSupport {
    /// Whether the gateway may inject `thinking` for this upstream.
    pub fn is_injectable(self) -> bool {
        matches!(self, Self::Signed)
    }
}

/// Codex tool compatibility level (from Provider Deck).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum CodexToolCompat {
    /// Supports Responses API with custom tools natively.
    ResponsesCustom,
    /// Supports Responses API with function tools only.
    ResponsesFunction,
    /// Only supports Chat Completions with function tools.
    ChatFunction,
    /// No tool support detected.
    None,
    /// Not yet probed.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_kind_serde() {
        // Test deserializing "openai" and "open_a_i"
        let p1: ProtocolKind = serde_json::from_str(r#""openai""#).unwrap();
        assert_eq!(p1, ProtocolKind::OpenAI);

        let p2: ProtocolKind = serde_json::from_str(r#""open_a_i""#).unwrap();
        assert_eq!(p2, ProtocolKind::OpenAI);

        // Test responses
        let p_resp: ProtocolKind = serde_json::from_str(r#""responses""#).unwrap();
        assert_eq!(p_resp, ProtocolKind::Responses);
        let p_resp2: ProtocolKind = serde_json::from_str(r#""openai_responses""#).unwrap();
        assert_eq!(p_resp2, ProtocolKind::Responses);
        assert_eq!(
            serde_json::to_string(&ProtocolKind::Responses).unwrap(),
            r#""responses""#
        );

        // Test serialize produces "openai"
        let s = serde_json::to_string(&ProtocolKind::OpenAI).unwrap();
        assert_eq!(s, r#""openai""#);

        // Test other variants
        let p_ant: ProtocolKind = serde_json::from_str(r#""anthropic""#).unwrap();
        assert_eq!(p_ant, ProtocolKind::Anthropic);
        assert_eq!(
            serde_json::to_string(&ProtocolKind::Anthropic).unwrap(),
            r#""anthropic""#
        );
    }
}
