//! Resolve a profile's Claude Code launch parameters against the primary
//! provider's probed capabilities.
//!
//! Priority, as the panel advertises it: a value the user typed beats a value
//! the last probe reported, which beats the built-in table. Thinking support is
//! not a boolean — only [`ThinkingSupport::Signed`] is injectable, matching the
//! gateway's own `is_injectable`. An unsigned thinking block would let this
//! panel inject `MAX_THINKING_TOKENS` into a session that then cannot persist
//! the turn.

use crate::profile::{
    ClaudeCodeParams, ProviderConfig, OUTPUT_TOKENS_WITHOUT_THINKING, OUTPUT_TOKENS_WITH_THINKING,
    THINKING_TOKENS_DEFAULT,
};
#[cfg(test)]
use crate::types::ThinkingSupport;

/// What actually gets written into `~/.claude/settings.json` `env`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedClaudeCodeParams {
    pub max_output_tokens: u64,
    /// `None` means do not inject `MAX_THINKING_TOKENS` — either the model
    /// cannot take it, or we have nothing to recommend.
    pub max_thinking_tokens: Option<u64>,
    pub thinking_supported: bool,
    pub disable_autoupdater: bool,
}

pub fn thinking_is_supported(provider: &ProviderConfig) -> bool {
    provider.thinking_support.is_injectable()
}

pub fn resolve(provider: &ProviderConfig, params: &ClaudeCodeParams) -> ResolvedClaudeCodeParams {
    let thinking_supported = thinking_is_supported(provider);
    let max_output_tokens = params
        .max_output_tokens
        .or(provider.probed_max_output_tokens)
        .unwrap_or(if thinking_supported {
            OUTPUT_TOKENS_WITH_THINKING
        } else {
            OUTPUT_TOKENS_WITHOUT_THINKING
        });
    let max_thinking_tokens = if thinking_supported {
        Some(
            params
                .max_thinking_tokens
                .unwrap_or(THINKING_TOKENS_DEFAULT),
        )
    } else {
        None
    };
    ResolvedClaudeCodeParams {
        max_output_tokens,
        max_thinking_tokens,
        thinking_supported,
        disable_autoupdater: params.disable_autoupdater,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CodexToolCompat, ProtocolKind, ReasoningConfidence, RelayChatCompat};

    fn provider(thinking: ThinkingSupport, probed: Option<u64>) -> ProviderConfig {
        ProviderConfig {
            id: "p".into(),
            name: "p".into(),
            base_url: "https://example".into(),
            protocol: ProtocolKind::Anthropic,
            default_model: "m".into(),
            models: vec![],
            is_primary: true,
            codex_compat: CodexToolCompat::Unknown,
            reasoning_confidence: ReasoningConfidence::Unknown,
            thinking_support: thinking,
            relay_chat_compat: RelayChatCompat::default(),
            accept_invalid_certs: false,
            max_price_per_request: None,
            rate_limit: Default::default(),
            supports_1m_context: None,
            default_effort_level: None,
            opus_model: None,
            sonnet_model: None,
            haiku_model: None,
            opus_display_name: None,
            sonnet_display_name: None,
            haiku_display_name: None,
            probed_max_output_tokens: probed,
            extra_headers: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn unsigned_thinking_is_not_treated_as_supported() {
        let resolved = resolve(
            &provider(ThinkingSupport::Unsigned, None),
            &Default::default(),
        );
        assert!(!resolved.thinking_supported);
        assert_eq!(resolved.max_thinking_tokens, None);
        assert_eq!(resolved.max_output_tokens, OUTPUT_TOKENS_WITHOUT_THINKING);
    }

    #[test]
    fn unprobed_and_absent_match_unsigned() {
        for t in [ThinkingSupport::Unprobed, ThinkingSupport::Absent] {
            let resolved = resolve(&provider(t, None), &Default::default());
            assert!(!resolved.thinking_supported, "{t:?}");
            assert_eq!(resolved.max_thinking_tokens, None, "{t:?}");
        }
    }

    #[test]
    fn signed_thinking_gets_the_thinking_defaults() {
        let resolved = resolve(
            &provider(ThinkingSupport::Signed, None),
            &Default::default(),
        );
        assert!(resolved.thinking_supported);
        assert_eq!(resolved.max_thinking_tokens, Some(THINKING_TOKENS_DEFAULT));
        assert_eq!(resolved.max_output_tokens, OUTPUT_TOKENS_WITH_THINKING);
    }

    #[test]
    fn a_probed_ceiling_beats_the_table() {
        let resolved = resolve(
            &provider(ThinkingSupport::Absent, Some(16_384)),
            &Default::default(),
        );
        assert_eq!(resolved.max_output_tokens, 16_384);
    }

    #[test]
    fn a_manual_ceiling_beats_the_probe() {
        let params = ClaudeCodeParams {
            max_output_tokens: Some(65_536),
            ..Default::default()
        };
        let resolved = resolve(&provider(ThinkingSupport::Signed, Some(16_384)), &params);
        assert_eq!(resolved.max_output_tokens, 65_536);
        assert_eq!(resolved.max_thinking_tokens, Some(THINKING_TOKENS_DEFAULT));
    }

    #[test]
    fn a_manual_thinking_ceiling_is_kept_when_supported() {
        let params = ClaudeCodeParams {
            max_thinking_tokens: Some(12_288),
            ..Default::default()
        };
        let resolved = resolve(&provider(ThinkingSupport::Signed, None), &params);
        assert_eq!(resolved.max_thinking_tokens, Some(12_288));
    }

    #[test]
    fn a_manual_thinking_ceiling_is_not_injected_when_unsupported() {
        let params = ClaudeCodeParams {
            max_thinking_tokens: Some(12_288),
            ..Default::default()
        };
        let resolved = resolve(&provider(ThinkingSupport::Absent, None), &params);
        assert_eq!(resolved.max_thinking_tokens, None);
    }

    #[test]
    fn disable_autoupdater_is_passed_through() {
        let params = ClaudeCodeParams {
            disable_autoupdater: true,
            ..Default::default()
        };
        let resolved = resolve(&provider(ThinkingSupport::Unprobed, None), &params);
        assert!(resolved.disable_autoupdater);
    }
}
