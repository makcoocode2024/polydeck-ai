//! The model catalogue written into `~/.codex/config.toml`.
//!
//! Codex decides a model's context window, reasoning ladder, and effort submenu
//! from this catalogue rather than by asking the upstream, so an entry that
//! overstates a window fails requests and an empty reasoning ladder leaves the
//! effort submenu impossible to dismiss. The per-family rules and their reasoning
//! live here; `profile_switch` only writes the result.

use std::collections::HashSet;

/// The effort ladder every model is offered.
///
/// Codex decides the submenu from this list, not from the upstream, so the
/// catalogue must advertise every level the user may want to send. Whether the
/// upstream accepts a level is its own call: the gateway forwards what the user
/// picked and lets a 400 surface rather than silently narrowing the menu.
/// `none` stays selectable, so "no reasoning" remains reachable.
pub(super) fn full_reasoning_levels() -> serde_json::Value {
    serde_json::json!([
        { "effort": "none", "description": "关闭推理思考 (No reasoning)" },
        { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
        { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
        { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" },
        { "effort": "xhigh", "description": "极限深度推理 (Extended reasoning depth for hard tasks)" },
        { "effort": "max", "description": "最大极限推理 (Maximum reasoning budget for toughest challenges)" }
    ])
}

/// `(default, levels, supports_reasoning_summaries)` for a model.
///
/// One ladder for every model. Naming rules used to narrow it by family, which
/// meant any relay model whose name no rule recognised lost `xhigh`/`max` from
/// the menu entirely.
pub(super) fn get_model_reasoning_config(
    _slug: &str,
) -> (serde_json::Value, serde_json::Value, bool) {
    (serde_json::json!("high"), full_reasoning_levels(), true)
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

    // Agnes AI. `supports_1m_context` cannot express this catalogue with one
    // flag, and neither can a single `-flash` rule: the Pro pair is 1M, but the
    // Flash line splits by generation. 3.0 and 2.5 are 512K, while 2.0 and 1.5
    // publish 256K — 2.0 briefly ran at 1M and was rolled back in June 2026.
    // Matching `-flash` alone reported 512K for all of them, which travels into
    // `CLAUDE_CODE_MAX_CONTEXT_TOKENS` and lets a 2.0 session run twice as long
    // as the upstream accepts.
    if lower.starts_with("agnes-") {
        if lower.contains("-pro") {
            return Some(ModelWindow {
                context: 1_000_000,
                max_output: 65_536,
            });
        }
        if lower.contains("-flash") {
            // Unknown future generations fall through rather than inheriting
            // either figure: a guess here is worse than the caller's fallback.
            let context = if lower.starts_with("agnes-3.") || lower.starts_with("agnes-2.5") {
                512_000
            } else if lower.starts_with("agnes-2.0") || lower.starts_with("agnes-1.5") {
                256_000
            } else {
                return None;
            };
            return Some(ModelWindow {
                context,
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
                // Codex already renders `slug` in the name column. Repeating it
                // here doubles the width of every picker row and truncates long
                // relay descriptions.
                "description": format!("via {provider_name}"),
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

#[cfg(test)]
mod tests {
    use super::build_codex_catalog;

    #[test]
    fn catalog_description_does_not_repeat_the_model_slug() {
        let catalog = build_codex_catalog(
            "relay.example",
            "model-a",
            &["model-a".into(), "model-b".into()],
        );

        for model in catalog["models"].as_array().unwrap() {
            let slug = model["slug"].as_str().unwrap();
            let description = model["description"].as_str().unwrap();
            assert_eq!(description, "via relay.example");
            assert!(!description.contains(slug));
        }
    }
}
