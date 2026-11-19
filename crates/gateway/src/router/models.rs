//! The `/v1/models` handler and the catalogue it synthesizes.
//!
//! An upstream model list says which ids exist but not which reasoning efforts
//! they accept, so the gateway adds missing capability metadata.

use super::passthrough_verbatim;
use super::respond::json_error;
use super::{AppState, ConnectionGuard};
use axum::{
    body::Body,
    extract::Extension,
    http::{header, Response, StatusCode},
};
use polydeck_core::types::ThinkingSupport;
use serde_json::Value;
use std::sync::Arc;
use tracing::debug;

pub(super) async fn handle_models(Extension(state): Extension<Arc<AppState>>) -> Response<Body> {
    state.health.increment_connections();
    let _guard = ConnectionGuard(&state.health);
    debug!("Processing GET /models request");
    let upstream_resp = match state.upstream.get_models().await {
        Ok(resp) => resp,
        Err(e) => {
            return json_error(
                StatusCode::BAD_GATEWAY,
                format!("Failed to fetch models: {}", e),
            )
        }
    };
    if !upstream_resp.status().is_success() {
        return passthrough_verbatim(upstream_resp).await;
    }
    let json: Value = match upstream_resp.json().await {
        Ok(v) => v,
        Err(_) => return json_error(StatusCode::BAD_GATEWAY, "Invalid models upstream response"),
    };
    let response =
        synthesize_models_response(json, state.thinking_support, state.claude_max_output_tokens);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(response.to_string()))
        .unwrap()
}

/// Default capability blob for models that carry no capability metadata.
///
/// Claude Code decides whether a model gets an effort picker from
/// `capabilities.effort.supported`, not from the model name, so third-party
/// names (gpt-5.6-luna, deepseek-v4-pro-0813) need this synthesized or the
/// picker never appears.
pub(super) fn synthesize_capabilities(thinking_support: ThinkingSupport) -> Value {
    serde_json::json!({
        "effort": {
            "supported": true,
            "low": {"supported": true},
            "medium": {"supported": true},
            "high": {"supported": true},
            "xhigh": {"supported": true},
            "max": {"supported": true}
        },
        "thinking": {
            "supported": thinking_support == ThinkingSupport::Signed,
            "types": {
                "enabled": {"supported": true},
                "adaptive": {"supported": true}
            }
        },
        "image_input": {"supported": true},
        "pdf_input": {"supported": false},
        "batch": {"supported": false},
        "citations": {"supported": false},
        "code_execution": {"supported": false},
        "context_management": {"supported": false}
    })
}

/// Whether Claude Code's gateway discovery keeps an id.
///
/// Claude Code 2.1.x filters `GET /v1/models` with `/(claude|anthropic)/i`
/// before building the picker. A relay catalog made only of `deepseek-*`,
/// `hy3`, or `kimi-*` therefore discovers zero models, even though every entry
/// parsed successfully.
fn is_visible_to_claude_code(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    lower.contains("claude") || lower.contains("anthropic")
}

/// Turn a raw upstream `/models` payload into the shape Claude Code consumes.
///
/// Upstreams return either an OpenAI list (`{"data":[{id,...}]}`) or an
/// Anthropic page (`{"data":[...], "has_more":...}`). We walk `data[]`, inject
/// capabilities, and always emit an Anthropic page.
///
/// Original ids stay in the page so Codex and other OpenAI clients can address
/// them. Each id Claude Code's discovery would filter out also gets a
/// `claude-code/<id>` alias with the same display name; the gateway strips the
/// prefix before forwarding (see `router::strip_claude_code_prefix`), so the
/// picker shows the real model name while requests still reach the upstream.
pub(super) fn synthesize_models_response(
    raw: Value,
    thinking_support: ThinkingSupport,
    claude_max_output_tokens: Option<u64>,
) -> Value {
    let data = raw
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut models: Vec<Value> = data
        .into_iter()
        .map(|mut m| {
            if !m.get("capabilities").is_some_and(Value::is_object) {
                m["capabilities"] = synthesize_capabilities(thinking_support);
            } else {
                let capabilities = &mut m["capabilities"];
                if !capabilities.get("thinking").is_some_and(Value::is_object) {
                    capabilities["thinking"] = serde_json::json!({});
                }
                capabilities["thinking"].as_object_mut().unwrap().insert(
                    "supported".to_string(),
                    Value::Bool(thinking_support == ThinkingSupport::Signed),
                );
                if !capabilities.get("effort").is_some_and(Value::is_object) {
                    capabilities["effort"] = serde_json::json!({});
                }
                let effort = capabilities["effort"].as_object_mut().unwrap();
                effort.entry("supported").or_insert(Value::Bool(true));
                for level in ["low", "medium", "high", "xhigh", "max"] {
                    effort
                        .entry(level)
                        .or_insert_with(|| serde_json::json!({ "supported": true }));
                }
            }
            if !m.get("max_input_tokens").and_then(Value::as_u64).is_some() {
                m["max_input_tokens"] = serde_json::json!(200000);
            }
            if let Some(max_output) = claude_max_output_tokens {
                m["max_tokens"] = serde_json::json!(max_output);
            } else if m.get("max_tokens").and_then(Value::as_u64).is_none() {
                m["max_tokens"] = serde_json::json!(32000);
            }
            // Claude Code's picker labels a discovered model with
            // `display_name` and falls back to the wire id. A missing label
            // would expose the `claude-code/` alias instead of the real name.
            if m.get("display_name").and_then(Value::as_str).is_none() {
                if let Some(id) = m.get("id").cloned() {
                    m["display_name"] = id;
                }
            }
            m
        })
        .collect();

    // Claude Code's discovery keeps only ids containing "claude" or
    // "anthropic". Relay catalogs rarely do, and the picker then shows only the
    // model already selected from settings. Keep the originals and add a
    // discoverable alias for each foreign id.
    let aliases: Vec<Value> = models
        .iter()
        .filter_map(|model| {
            let id = model.get("id")?.as_str()?;
            if is_visible_to_claude_code(id) || id.starts_with("claude-code/") {
                return None;
            }
            let mut alias = model.clone();
            alias["id"] = serde_json::json!(format!("claude-code/{id}"));
            Some(alias)
        })
        .collect();
    models.extend(aliases);

    let mut resp = serde_json::json!({
        "data": models,
        "has_more": false,
        "first_id": models.first().and_then(|m| m.get("id")).cloned().unwrap_or(Value::String(String::new())),
        "last_id": models.last().and_then(|m| m.get("id")).cloned().unwrap_or(Value::String(String::new())),
        "object": "list"
    });
    // Preserve an upstream pagination cursor when present.
    if let Some(last_id) = raw.get("last_id") {
        resp["last_id"] = last_id.clone();
    }
    resp
}
