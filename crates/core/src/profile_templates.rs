//! Built-in profile templates for common providers.

use crate::profile::{ProfileCreate, ProviderConfig};
use crate::types::{CodexToolCompat, ProtocolKind, ReasoningConfidence, ThinkingSupport};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ProfileTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub provider: ProviderConfig,
}

/// Agnes AI gateway, mainland China route. Pairs with a platform.agnes-ai.cn key.
pub const AGNES_BASE_URL_CN: &str = "https://api.agnes-ai.cn/v1";
/// Agnes AI gateway, international route.
///
/// Same model catalogue, separate key scope. Measured: a CN-issued key gets 200
/// from this host's `/v1/models` but 401 (`无效的令牌`) on
/// `/v1/chat/completions`, so a healthy model list is not proof the route will
/// serve inference. `probe` catches it because it validates with a chat call.
pub const AGNES_BASE_URL_GLOBAL: &str = "https://apihub.agnes-ai.com/v1";
/// Free tier, and the strongest coding/agent model of the two free ones.
pub const AGNES_DEFAULT_MODEL: &str = "agnes-2.5-flash";
/// Measured ceiling on a free key: the 17th request in a minute returns 429.
/// Agnes exposes no `RateLimit-*` response headers, so `probe_rate_limits`
/// cannot discover this and falls back to its generic 60 — which would sit
/// three times over the real limit. Hard-coded on purpose.
pub const AGNES_FREE_TIER_RPM: u32 = 20;

/// Text models only. `/v1/models` also lists `agnes-image-2.1-flash`,
/// `agnes-video-2.5` and `agnes-video-v2.0`, but those answer on
/// `/v1/images/generations` and `/v1/videos` — offering them as a chat model
/// would just hand the user a selection that 400s.
fn agnes_text_models() -> Vec<String> {
    vec![
        "agnes-2.5-flash".into(),
        "agnes-2.0-flash".into(),
        "agnes-2.5-pro".into(),
        "agnes-2.5-pro-alpha".into(),
    ]
}

/// Both Agnes routes share one provider shape; only id/name/base_url differ.
///
/// `protocol` is deliberately `OpenAI` rather than `Responses`, even though the
/// upstream does serve `/v1/responses`. Only Chat Completions accepts `tools`
/// beyond the plain `function` type, and `OpenAI` maps to `ResponsesMode::Auto`,
/// which lets the gateway probe and fall back. Pinning `Responses` would select
/// `Native` and lean on the tool-bridge override alone.
///
/// All three Claude tiers point at one model: Claude Code's `/model` picker only
/// lists ids starting with `claude-`, so an `agnes-*` id is reachable only
/// through a tier slot or an explicit `--model`. Display names are left `None`
/// so `profile_switch`'s built-in Anthropic ids are written, which is what makes
/// Claude Code apply a real context window and price.
fn agnes_provider(id: &str, name: &str, base_url: &str) -> ProviderConfig {
    ProviderConfig {
        id: id.into(),
        name: name.into(),
        base_url: base_url.into(),
        protocol: ProtocolKind::OpenAI,
        default_model: AGNES_DEFAULT_MODEL.into(),
        models: agnes_text_models(),
        is_primary: true,
        codex_compat: CodexToolCompat::ChatFunction,
        reasoning_confidence: ReasoningConfidence::Verified,
        // Measured on the streaming path: Agnes returns thinking blocks with no
        // `signature`, so extended thinking cannot be injected against it even
        // though the OpenAI-side reasoning probe passes — `Verified` above refers
        // to that other path. The non-streaming probe returns no thinking block at
        // all and so reports `Absent`; either way the gate stays shut, and a real
        // probe overwrites this seed value.
        thinking_support: ThinkingSupport::Unsigned,
        accept_invalid_certs: false,
        max_price_per_request: None,
        rate_limit: crate::profile::RateLimitSettings {
            enabled: true,
            rpm: AGNES_FREE_TIER_RPM,
            ..Default::default()
        },
        supports_1m_context: Some(false),
        default_effort_level: None,
        opus_model: Some(AGNES_DEFAULT_MODEL.into()),
        sonnet_model: Some(AGNES_DEFAULT_MODEL.into()),
        haiku_model: Some(AGNES_DEFAULT_MODEL.into()),
        opus_display_name: None,
        sonnet_display_name: None,
        haiku_display_name: None,
    }
}

pub fn builtin_templates() -> Vec<ProfileTemplate> {
    vec![
        ProfileTemplate {
            id: "custom".into(),
            name: "自定义".into(),
            description: "通用自定义 API 端点，支持任意兼容 OpenAI / Anthropic 协议的服务商".into(),
            provider: ProviderConfig {
                id: "custom-provider".into(),
                name: "自定义节点".into(),
                base_url: "https://api.example.com/v1".into(),
                protocol: ProtocolKind::OpenAI,
                default_model: "gpt-4o".into(),
                models: vec!["gpt-4o".into()],
                is_primary: true,
                codex_compat: CodexToolCompat::ResponsesCustom,
                reasoning_confidence: ReasoningConfidence::Unknown,
                thinking_support: ThinkingSupport::Unprobed,
                accept_invalid_certs: false,
                max_price_per_request: None,
                rate_limit: crate::profile::RateLimitSettings::default(),
                supports_1m_context: None,
                default_effort_level: None,
                opus_model: None,
                sonnet_model: None,
                haiku_model: None,
                opus_display_name: None,
                sonnet_display_name: None,
                haiku_display_name: None,
            },
        },
        ProfileTemplate {
            id: "openai".into(),
            name: "OpenAI 官方".into(),
            description: "GPT 和 o 系列模型，使用 OpenAI 官方 API".into(),
            provider: ProviderConfig {
                id: "openai-official".into(),
                name: "OpenAI".into(),
                base_url: "https://api.openai.com/v1".into(),
                protocol: ProtocolKind::Responses,
                default_model: "gpt-4o".into(),
                models: vec!["gpt-4o".into()],
                is_primary: true,
                codex_compat: CodexToolCompat::ResponsesCustom,
                reasoning_confidence: ReasoningConfidence::Unknown,
                thinking_support: ThinkingSupport::Unprobed,
                accept_invalid_certs: false,
                max_price_per_request: None,
                rate_limit: crate::profile::RateLimitSettings::default(),
                supports_1m_context: None,
                default_effort_level: None,
                opus_model: None,
                sonnet_model: None,
                haiku_model: None,
                opus_display_name: None,
                sonnet_display_name: None,
                haiku_display_name: None,
            },
        },
        ProfileTemplate {
            id: "anthropic".into(),
            name: "Anthropic 官方".into(),
            description: "Claude 系列模型，使用 Anthropic 官方 API".into(),
            provider: ProviderConfig {
                id: "anthropic-official".into(),
                name: "Anthropic".into(),
                base_url: "https://api.anthropic.com/v1".into(),
                protocol: ProtocolKind::Anthropic,
                default_model: "claude-3-7-sonnet-20250219".into(),
                models: vec!["claude-3-7-sonnet-20250219".into()],
                is_primary: true,
                codex_compat: CodexToolCompat::Unknown,
                reasoning_confidence: ReasoningConfidence::Unknown,
                thinking_support: ThinkingSupport::Unprobed,
                accept_invalid_certs: false,
                max_price_per_request: None,
                rate_limit: crate::profile::RateLimitSettings::default(),
                supports_1m_context: None,
                default_effort_level: None,
                opus_model: None,
                sonnet_model: None,
                haiku_model: None,
                opus_display_name: None,
                sonnet_display_name: None,
                haiku_display_name: None,
            },
        },
        ProfileTemplate {
            id: "deepseek".into(),
            name: "DeepSeek 官方".into(),
            description: "DeepSeek Chat 和 Reasoner 模型".into(),
            provider: ProviderConfig {
                id: "deepseek-official".into(),
                name: "DeepSeek".into(),
                base_url: "https://api.deepseek.com/v1".into(),
                protocol: ProtocolKind::OpenAI,
                default_model: "deepseek-chat".into(),
                models: vec!["deepseek-chat".into(), "deepseek-reasoner".into()],
                is_primary: true,
                codex_compat: CodexToolCompat::ChatFunction,
                reasoning_confidence: ReasoningConfidence::Unknown,
                thinking_support: ThinkingSupport::Unprobed,
                accept_invalid_certs: false,
                max_price_per_request: None,
                rate_limit: crate::profile::RateLimitSettings::default(),
                supports_1m_context: None,
                default_effort_level: None,
                opus_model: None,
                sonnet_model: None,
                haiku_model: None,
                opus_display_name: None,
                sonnet_display_name: None,
                haiku_display_name: None,
            },
        },
        ProfileTemplate {
            id: "agnes-cn".into(),
            name: "Agnes AI (国内站)".into(),
            description: "Agnes 多模态网关，国内直连；Chat / Responses / Messages 三协议齐备，agnes-2.5-flash 现价免费"
                .into(),
            provider: agnes_provider("agnes-cn", "Agnes AI 国内站", AGNES_BASE_URL_CN),
        },
        ProfileTemplate {
            id: "agnes-global".into(),
            name: "Agnes AI (国际站)".into(),
            description: "Agnes 多模态网关，国际线路；与国内站模型一致，海外网络环境优选".into(),
            provider: agnes_provider("agnes-global", "Agnes AI 国际站", AGNES_BASE_URL_GLOBAL),
        },
        ProfileTemplate {
            id: "zhipu".into(),
            name: "智谱 GLM".into(),
            description: "GLM 系列模型，国内直连".into(),
            provider: ProviderConfig {
                id: "zhipu-official".into(),
                name: "智谱".into(),
                base_url: "https://open.bigmodel.cn/api/paas/v4".into(),
                protocol: ProtocolKind::OpenAI,
                default_model: "glm-4-flash".into(),
                models: vec!["glm-4-flash".into(), "glm-4-plus".into()],
                is_primary: true,
                codex_compat: CodexToolCompat::ChatFunction,
                reasoning_confidence: ReasoningConfidence::Unknown,
                thinking_support: ThinkingSupport::Unprobed,
                accept_invalid_certs: false,
                max_price_per_request: None,
                rate_limit: crate::profile::RateLimitSettings::default(),
                supports_1m_context: None,
                default_effort_level: None,
                opus_model: None,
                sonnet_model: None,
                haiku_model: None,
                opus_display_name: None,
                sonnet_display_name: None,
                haiku_display_name: None,
            },
        },
    ]
}

pub fn find_template(id: &str) -> Option<ProfileTemplate> {
    builtin_templates().into_iter().find(|t| t.id == id)
}

pub fn template_to_create(template: &ProfileTemplate) -> ProfileCreate {
    ProfileCreate {
        name: template.name.clone(),
        providers: vec![template.provider.clone()],
        clients: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ships_both_agnes_routes() {
        let cn = find_template("agnes-cn").expect("agnes-cn template missing");
        let global = find_template("agnes-global").expect("agnes-global template missing");
        assert_eq!(cn.provider.base_url, AGNES_BASE_URL_CN);
        assert_eq!(global.provider.base_url, AGNES_BASE_URL_GLOBAL);
        // Same catalogue on both routes; only the host differs.
        assert_eq!(cn.provider.models, global.provider.models);
        assert_eq!(cn.provider.default_model, global.provider.default_model);
    }

    #[test]
    fn agnes_uses_chat_completions_not_responses() {
        // Agnes serves /v1/responses, but only Chat Completions accepts tool
        // types beyond plain `function`. OpenAI here also yields
        // ResponsesMode::Auto, keeping the gateway's bridge fallback live.
        for id in ["agnes-cn", "agnes-global"] {
            let t = find_template(id).unwrap();
            assert_eq!(t.provider.protocol, ProtocolKind::OpenAI);
            assert_eq!(t.provider.codex_compat, CodexToolCompat::ChatFunction);
        }
    }

    #[test]
    fn agnes_pins_the_measured_rate_limit() {
        // Agnes returns no RateLimit-* headers, so the probe cannot find this
        // and would leave the generic 60 in place.
        for id in ["agnes-cn", "agnes-global"] {
            let rl = find_template(id).unwrap().provider.rate_limit;
            assert!(rl.enabled, "{id}: rate limiting must be on");
            assert_eq!(
                rl.rpm, AGNES_FREE_TIER_RPM,
                "{id}: rpm must be the measured 20"
            );
        }
    }

    #[test]
    fn agnes_fills_every_claude_tier() {
        // Claude Code's picker drops ids that do not start with `claude-`, so a
        // tier slot is the only way an agnes-* model becomes selectable.
        for id in ["agnes-cn", "agnes-global"] {
            let p = find_template(id).unwrap().provider;
            assert_eq!(p.opus_model.as_deref(), Some(AGNES_DEFAULT_MODEL));
            assert_eq!(p.sonnet_model.as_deref(), Some(AGNES_DEFAULT_MODEL));
            assert_eq!(p.haiku_model.as_deref(), Some(AGNES_DEFAULT_MODEL));
            // Left blank so profile_switch writes the built-in Anthropic ids.
            assert!(p.opus_display_name.is_none());
            assert!(p.sonnet_display_name.is_none());
            assert!(p.haiku_display_name.is_none());
        }
    }

    #[test]
    fn agnes_lists_only_chat_capable_models() {
        let models = find_template("agnes-cn").unwrap().provider.models;
        assert!(models.contains(&AGNES_DEFAULT_MODEL.to_string()));
        // Image and video models answer on other endpoints entirely.
        for id in &models {
            assert!(
                !id.contains("image") && !id.contains("video"),
                "{id} is not a chat model and must not be offered as one"
            );
        }
    }

    #[test]
    fn default_model_is_in_the_model_list() {
        // A default outside `models` leaves the UI dropdown with nothing selected.
        for t in builtin_templates() {
            assert!(
                t.provider.models.contains(&t.provider.default_model),
                "template '{}' has default_model '{}' absent from its models list",
                t.id,
                t.provider.default_model
            );
        }
    }
}
