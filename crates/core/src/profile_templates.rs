//! Built-in profile templates for common providers.

use crate::profile::{ProfileCreate, ProviderConfig};
use crate::types::{CodexToolCompat, ProtocolKind, ReasoningConfidence};
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
                accept_invalid_certs: false,
                max_price_per_request: None,
                rate_limit: crate::profile::RateLimitSettings::default(),
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
                accept_invalid_certs: false,
                max_price_per_request: None,
                rate_limit: crate::profile::RateLimitSettings::default(),
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
                accept_invalid_certs: false,
                max_price_per_request: None,
                rate_limit: crate::profile::RateLimitSettings::default(),
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
                accept_invalid_certs: false,
                max_price_per_request: None,
                rate_limit: crate::profile::RateLimitSettings::default(),
            },
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
                accept_invalid_certs: false,
                max_price_per_request: None,
                rate_limit: crate::profile::RateLimitSettings::default(),
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
