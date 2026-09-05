//! Reasoning-effort normalization and Anthropic thinking injection.
//!
//! Upstreams disagree on which effort levels they accept, and clients send values
//! that mean nothing upstream, so every effort value is normalized before it
//! leaves the gateway. Thinking injection is gated on measured upstream support
//! rather than assumed; see `inject_thinking_if_needed`.

use polydeck_core::types::ThinkingSupport;
use serde_json::Value;
use tracing::{debug, warn};

pub fn effort_to_budget_tokens(effort: &str) -> Option<u64> {
    match effort.trim().to_ascii_lowercase().as_str() {
        "none" | "off" | "0" | "false" => None,
        "low" => Some(2048),
        "medium" => Some(8192),
        "high" => Some(16384),
        "xhigh" => Some(32768),
        "max" => Some(63999),
        other => other.parse::<u64>().ok().filter(|&b| b >= 1024),
    }
}

/// Turn on extended thinking, but only when the upstream is known to support it.
///
/// Both gates must pass. Injecting against an upstream that returns thinking
/// blocks without a `signature` breaks the client outright: it cannot persist an
/// unsigned block, so the turn never finalizes, its `tool_use` never gets a
/// `tool_result`, and every later request in that session carries an orphaned
/// `tool_use` the upstream rejects. Not injecting only costs reasoning depth, so
/// missing information must mean off.
pub fn inject_thinking_if_needed(
    body: &mut Value,
    default_effort_level: Option<&str>,
    thinking_support: ThinkingSupport,
) {
    if body.get("thinking").is_some() {
        return;
    }

    let Some(effort) = default_effort_level else {
        return;
    };
    if !thinking_support.is_injectable() {
        debug!(
            "thinking injection skipped: upstream thinking support is {:?}",
            thinking_support
        );
        return;
    }
    let Some(budget_tokens) = effort_to_budget_tokens(effort) else {
        return;
    };

    let max_tokens = body.get("max_tokens").and_then(|v| v.as_u64());

    let effective_budget = match max_tokens {
        Some(max) if max <= 1024 => {
            return;
        }
        Some(max) if budget_tokens >= max => max.saturating_sub(1).max(1024),
        _ => budget_tokens,
    };

    body["thinking"] = serde_json::json!({
        "type": "enabled",
        "budget_tokens": effective_budget
    });

    if let Some(temp) = body.get_mut("temperature") {
        *temp = serde_json::json!(1.0);
    }
}

pub(super) fn inject_max_price(body: &mut Value, max_price: Option<f64>) {
    if let Some(price) = max_price {
        if let Some(obj) = body.as_object_mut() {
            obj.entry("max_price_per_request")
                .or_insert_with(|| Value::from(price));
        }
    }
}

/// The effort values every supported upstream is expected to accept.
const EFFORT_WHITELIST: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// Capability-normalize a reasoning effort before it leaves the gateway.
///
/// Upstreams 400 on effort values they do not support (DeepSeek rejects
/// anything outside low/medium/high/xhigh/max), and Claude Code sends values
/// like `minimal` or `none` that mean nothing upstream. Returns the rewritten
/// effort to send, or `None` when the field should be dropped.
pub(super) fn normalize_effort(effort: &str, model: &str) -> Option<String> {
    let clean = effort.trim().to_ascii_lowercase();
    // Auto-thinking models (DeepSeek reasoner/R1, QwQ) ignore effort and error
    // if forced; drop it before the whitelist so even a valid level is stripped.
    let model_lower = model.to_ascii_lowercase();
    if model_lower.contains("reasoner") || model_lower.contains("r1") || model_lower.contains("qwq")
    {
        warn!("Model {model} runs its own thinking; dropping reasoning_effort={effort}");
        return None;
    }
    if EFFORT_WHITELIST.iter().any(|&l| l == clean) {
        return Some(clean);
    }
    match clean.as_str() {
        "minimal" => Some("low".to_string()),
        // DeepSeek non-reasoner models accept the full range; unknown values
        // here are an upstream contract violation and must not 400.
        "none" | "off" | "0" | "false" => None,
        other => {
            warn!("Dropping unsupported reasoning_effort={other} for model {model}");
            None
        }
    }
}

/// Sanitize reasoning effort on an Anthropic Messages body (top-level field).
/// Returns the rewritten effort value if the field was kept.
pub(super) fn sanitize_messages_effort(body: &mut Value, model: &str) -> Option<String> {
    let effort = body.get("reasoning_effort").and_then(Value::as_str)?;
    match normalize_effort(effort, model) {
        Some(clean) => {
            body["reasoning_effort"] = Value::String(clean.clone());
            Some(clean)
        }
        None => {
            body.as_object_mut().map(|o| o.remove("reasoning_effort"));
            None
        }
    }
}
