//! The model catalogue written into `~/.codex/config.toml`.
//!
//! Codex decides a model's context window, reasoning ladder, and effort submenu
//! from this catalogue rather than by asking the upstream, so an entry that
//! overstates a window fails requests and an empty reasoning ladder leaves the
//! effort submenu impossible to dismiss. The per-family rules and their reasoning
//! live here; `profile_switch` only writes the result.

use std::collections::HashSet;

/// The conservative effort ladder handed to models no rule recognises.
///
/// An empty `supported_reasoning_levels` leaves Codex's effort submenu with
/// nothing to select, and the menu then cannot be committed or dismissed except
/// with Esc. Third-party relay catalogs are full of names no pattern here
/// matches (`deepseek-v4-pro-0813`, `qwen3.8-max`), so they get low/medium/high
/// — the subset essentially every reasoning-capable upstream accepts.
pub(super) fn fallback_reasoning_levels() -> serde_json::Value {
    serde_json::json!([
        { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
        { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
        { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" }
    ])
}

/// A model's documented token limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ModelWindow {
    /// Total window, input plus output.
    pub(super) context: u64,
    /// Ceiling on a single response.
    pub(super) max_output: u64,
}

impl ModelWindow {
    /// The whole-session window to hand Claude Code.
    ///
    /// Auto-compact fires in the low-90s percent of whatever number it is given,
    /// so passing the full context leaves less than one max-length response
    /// between the compaction point and the upstream's hard ceiling. Reserving
    /// the output budget moves compaction early enough that a full-length reply
    /// still fits.
    pub(super) fn claude_budget(self) -> u64 {
        self.context.saturating_sub(self.max_output)
    }
}

/// A model's published limits, or `None` when no rule here covers the name.
///
/// Only names with a documented figure belong here. A guess would be worse than
/// the fallback: it travels into `CLAUDE_CODE_MAX_CONTEXT_TOKENS`, and one that
/// reads high lets a session run past what the upstream accepts.
pub(super) fn model_window(slug: &str) -> Option<ModelWindow> {
    let lower = slug.to_ascii_lowercase();

    // Agnes AI. The two families differ, so `supports_1m_context` cannot express
    // this catalogue with one flag: the Pro pair is 1M, the Flash pair 512K.
    if lower.starts_with("agnes-") {
        if lower.contains("-pro") {
            return Some(ModelWindow {
                context: 1_000_000,
                max_output: 65_536,
            });
        }
        if lower.contains("-flash") {
            return Some(ModelWindow {
                context: 512_000,
                max_output: 65_536,
            });
        }
    }

    None
}

/// The context window to advertise to Codex for `slug`.
///
/// Codex applies its own `effective_context_window_percent`, so it gets the full
/// context rather than the reserved budget Claude Code needs.
pub(super) fn codex_context_window(slug: &str, supports_1m: bool) -> i64 {
    let fallback = if supports_1m { 1_000_000 } else { 200_000 };
    model_window(slug).map(|w| w.context).unwrap_or(fallback) as i64
}

pub(super) fn get_model_reasoning_config(
    slug: &str,
) -> (serde_json::Value, serde_json::Value, bool) {
    let lower = slug.to_ascii_lowercase();

    // Check if explicitly non-reasoning model
    let is_explicit_non_reasoning = lower.starts_with("gpt-4o")
        || lower.starts_with("gpt-4-")
        || lower.starts_with("gpt-3.5")
        || lower.starts_with("claude-3-5")
        || lower.starts_with("claude-3.5")
        || lower.starts_with("claude-3-opus")
        || lower.starts_with("claude-3-haiku")
        || lower.starts_with("deepseek-chat")
        || lower.starts_with("deepseek-v3")
        || lower.starts_with("deepseek-coder")
        || lower.starts_with("glm-4")
        || lower.starts_with("qwen-2.5")
        || lower.starts_with("llama");

    if is_explicit_non_reasoning {
        // Still hand Codex one selectable entry: `supported_reasoning_levels: []`
        // leaves its effort submenu empty and the menu stops responding to
        // anything but Esc. `none` is the honest level for these models.
        let levels = serde_json::json!([
            { "effort": "none", "description": "关闭推理思考 (No reasoning)" }
        ]);
        return (serde_json::json!("none"), levels, false);
    }

    // 1. Sol family (旗舰: 支持 none, low, medium, high, xhigh, max)
    if lower.contains("sol") {
        let levels = serde_json::json!([
            { "effort": "none", "description": "关闭推理思考 (No reasoning)" },
            { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
            { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
            { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" },
            { "effort": "xhigh", "description": "极限深度推理 (Extended reasoning depth for hard tasks)" },
            { "effort": "max", "description": "最大极限推理 (Maximum reasoning budget for toughest challenges)" }
        ]);
        return (serde_json::json!("high"), levels, true);
    }

    // 2. Terra family (均衡: 支持 none, low, medium, high, xhigh，不支持 max)
    if lower.contains("terra") {
        let levels = serde_json::json!([
            { "effort": "none", "description": "关闭推理思考 (No reasoning)" },
            { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
            { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
            { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" },
            { "effort": "xhigh", "description": "极限深度推理 (Extended reasoning depth for hard tasks)" }
        ]);
        return (serde_json::json!("high"), levels, true);
    }

    // 3. Luna family (高速低成本: 支持 none, low, medium, high，不支持 xhigh/max)
    if lower.contains("luna") {
        let levels = serde_json::json!([
            { "effort": "none", "description": "关闭推理思考 (No reasoning)" },
            { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
            { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
            { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" }
        ]);
        return (serde_json::json!("medium"), levels, true);
    }

    // 4. Other GPT-5 series (如 gpt-5.4, gpt-5.5)
    if lower.starts_with("gpt-5") {
        let levels = serde_json::json!([
            { "effort": "none", "description": "关闭推理思考 (No reasoning)" },
            { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
            { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
            { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" },
            { "effort": "xhigh", "description": "极限深度推理 (Extended reasoning depth for hard tasks)" }
        ]);
        return (serde_json::json!("high"), levels, true);
    }

    // 5. Google Gemini family (只支持 low, medium, high；不支持 minimal/none/xhigh/max)
    if lower.contains("gemini") {
        let levels = serde_json::json!([
            { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
            { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
            { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" }
        ]);
        return (serde_json::json!("high"), levels, true);
    }

    // 6. Claude 3.7+ / Claude 4+ / Claude 5+ / Opus / Sonnet / Extended Thinking models (支持 none, low, medium, high, xhigh, max)
    let is_claude_thinking = lower.contains("claude-3-7")
        || lower.contains("claude-3.7")
        || lower.contains("claude-4")
        || lower.contains("claude-5")
        || lower.contains("claude-opus")
        || lower.contains("claude-sonnet")
        || lower.contains("sonnet-3-7")
        || lower.contains("sonnet-3.7")
        || lower.contains("sonnet-4")
        || lower.contains("sonnet-5")
        || lower.contains("opus-4")
        || lower.contains("opus-5")
        || lower.contains("model-s")
        || lower.contains("model-o");

    if is_claude_thinking {
        let levels = serde_json::json!([
            { "effort": "none", "description": "关闭推理思考 (No reasoning)" },
            { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
            { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
            { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" },
            { "effort": "xhigh", "description": "极限深度推理 (Extended reasoning depth for hard tasks)" },
            { "effort": "max", "description": "最大极限推理 (Maximum reasoning budget for toughest challenges)" }
        ]);
        return (serde_json::json!("high"), levels, true);
    }

    // 7. Other reasoning patterns (o1, o3, o4, thinking, reasoner, r1, qwq)
    let is_reasoning = lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
        || lower.contains("thinking")
        || lower.contains("reasoner")
        || lower.contains("reasoning")
        || lower.contains("r1")
        || lower.contains("qwq");

    if is_reasoning {
        let levels = serde_json::json!([
            { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
            { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
            { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" },
            { "effort": "xhigh", "description": "极限深度推理 (Extended reasoning depth for hard tasks)" }
        ]);
        (serde_json::json!("high"), levels, true)
    } else {
        // Unrecognised name: assume a reasoning model with the safe three-level
        // ladder rather than declaring no levels, which hangs Codex's menu.
        (
            serde_json::json!("medium"),
            fallback_reasoning_levels(),
            true,
        )
    }
}

pub fn build_codex_catalog(
    provider_name: &str,
    default_model: &str,
    models: &[String],
) -> serde_json::Value {
    build_codex_catalog_with_1m(provider_name, default_model, models, false)
}

pub fn build_codex_catalog_with_1m(
    provider_name: &str,
    default_model: &str,
    models: &[String],
    supports_1m: bool,
) -> serde_json::Value {
    let mut catalog_models: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let primary_model = if !default_model.trim().is_empty()
        && default_model.trim() != "codex-auto-review"
        && !default_model.trim().ends_with("auto-review")
    {
        default_model.trim()
    } else if let Some(first) = models.iter().find(|s| {
        !s.trim().is_empty()
            && s.trim() != "codex-auto-review"
            && !s.trim().ends_with("auto-review")
    }) {
        first.trim()
    } else {
        "gpt-4o"
    };

    if !primary_model.is_empty() && seen.insert(primary_model.to_string()) {
        catalog_models.push(primary_model.to_string());
    }

    for m in models {
        let trimmed = m.trim();
        if trimmed == "codex-auto-review" || trimmed.ends_with("auto-review") {
            continue;
        }
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            catalog_models.push(trimmed.to_string());
        }
    }

    if catalog_models.is_empty() {
        catalog_models.push("gpt-4o".to_string());
    }

    let models_json: Vec<serde_json::Value> = catalog_models
        .iter()
        .filter(|slug| *slug != "codex-auto-review" && !slug.ends_with("auto-review"))
        .enumerate()
        .map(|(i, slug)| {
            let context_window = codex_context_window(slug, supports_1m);
            let (default_reasoning, supported_reasoning, supports_reasoning) = get_model_reasoning_config(slug);
            serde_json::json!({
                "slug": slug,
                "display_name": slug,
                "description": format!("{slug} via {provider_name}"),
                "default_reasoning_level": default_reasoning,
                "default_reasoning_summary": "none",
                "default_verbosity": "medium",
                "context_window": context_window,
                "max_context_window": context_window,
                "effective_context_window_percent": 95,
                "priority": i,
                "input_modalities": ["text"],
                "service_tiers": [],
                "additional_speed_tiers": [],
                "shell_type": "shell_command",
                "apply_patch_tool_type": "freeform",
                "web_search_tool_type": "text",
                "supported_in_api": true,
                "support_verbosity": true,
                "supports_image_detail_original": false,
                "supports_parallel_tool_calls": true,
                "supports_reasoning_summaries": supports_reasoning,
                "supports_search_tool": true,
                "tool_mode": null,
                "upgrade": null,
                "visibility": "list",
                "availability_nux": null,
                "minimal_client_version": "0.0.1",
                "use_responses_lite": false,
                "available_in_plans": ["free", "pro", "team", "enterprise", "edu", "anon"],
                "truncation_policy": {
                    "limit": 10000,
                    "mode": "tokens"
                },
                "supported_reasoning_levels": supported_reasoning,
                "base_instructions": "You are Codex, a coding agent. Work carefully in the user's current workspace, follow the user's instructions, inspect existing code before editing, preserve unrelated changes, use available tools when needed, and verify completed work before reporting it.",
                "experimental_supported_tools": []
            })
        })
        .collect();

    serde_json::json!({
        "models": models_json
    })
}

/// The `wire_api` Codex should use, which is what picks the endpoint it POSTs to:
/// `responses` -> `{base_url}/responses`, `chat` -> `{base_url}/chat/completions`.
///
/// Gateway mode is always `responses`. The gateway serves that endpoint whatever
/// the upstream speaks, translating to Chat Completions when it has to, and Codex
/// only gets its `custom`-type tools (`apply_patch`) through the Responses shape.
///
/// Direct mode has no translator, so this has to name the endpoint the upstream
/// actually serves. Writing `responses` unconditionally is what sent Codex at
/// `/v1/responses` on chat-only upstreams, which answer
/// `model_not_supported_on_endpoint` — a failure whose cause is invisible from the
/// app, since the user picked Chat in the protocol selector and the error arrives
/// in the provider's own words.
///
/// Precedence matches `build_gateway_config`: the compat verdict wins over the
/// protocol field, because it is measured against `/v1/responses` itself while
/// the protocol field can be hand-picked.
pub(super) fn codex_wire_api(
    provider: &crate::profile::ProviderConfig,
    gateway_enabled: bool,
) -> &'static str {
    use crate::types::{CodexToolCompat, ProtocolKind};

    if gateway_enabled {
        return "responses";
    }

    match provider.protocol {
        // Chat Completions is all this upstream serves tools on, so Responses is
        // not reachable regardless of what the protocol field says.
        _ if provider.codex_compat == CodexToolCompat::ChatFunction => "chat",
        // Probed to answer on /v1/responses.
        _ if provider.codex_compat == CodexToolCompat::ResponsesCustom
            || provider.codex_compat == CodexToolCompat::ResponsesFunction =>
        {
            "responses"
        }
        ProtocolKind::Responses => "responses",
        // No usable compat verdict, so the protocol field is the only signal, and
        // it says /v1/chat/completions.
        ProtocolKind::OpenAI => "chat",
        // Codex speaks neither Anthropic nor Gemini, and an Azure deployment path
        // is not this shape either. None of them are served by guessing here.
        _ => "responses",
    }
}
