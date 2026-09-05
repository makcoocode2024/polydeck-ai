//! The `/v1/models` handler and the catalogue it synthesizes.
//!
//! An upstream model list says which ids exist but not which reasoning efforts
//! they accept, so the gateway adds a `capabilities` block. Whether `max` is
//! offered is inferred from the ids themselves; see `upstream_serves_max_effort`
//! for why the inference requires a matching base model.

use super::passthrough_verbatim;
use super::respond::json_error;
use super::{AppState, ConnectionGuard};
use axum::{
    body::Body,
    extract::Extension,
    http::{header, Response, StatusCode},
};
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
    let response = synthesize_models_response(json);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(response.to_string()))
        .unwrap()
}

/// Separators relays put between a model name and an effort suffix.
const EFFORT_SUFFIX_SEPARATORS: [char; 3] = ['-', '_', ':'];

/// Whether this upstream deals in `max` effort at all.
///
/// Relays that serve the level expose it as its own model id, so the signal is a
/// `…-max` id **whose base model is also advertised** — `claude-opus-5` next to
/// `claude-opus-5-max` means the suffix selects an effort level. Requiring the
/// pair is what separates that from a product name: `qwen-max` and `glm-4-max`
/// end the same way but are model tiers, and offering `max` there would only
/// produce requests the upstream rejects.
///
/// Asked per response rather than assumed, because most upstreams have no `max`.
pub(super) fn upstream_serves_max_effort(data: &[Value]) -> bool {
    let ids: Vec<String> = data
        .iter()
        .filter_map(|m| m.get("id").and_then(Value::as_str))
        .map(|id| id.trim().to_ascii_lowercase())
        .collect();

    // A context marker such as `[1m]` sits outside the effort suffix, so compare
    // on the part before it: a relay may advertise only `claude-opus-5-max[1m]`.
    let core = |id: &str| id.split('[').next().unwrap_or(id).to_string();

    ids.iter().any(|id| {
        EFFORT_SUFFIX_SEPARATORS.iter().any(|sep| {
            core(id)
                .strip_suffix(&format!("{sep}max"))
                .is_some_and(|base| !base.is_empty() && ids.iter().any(|other| core(other) == base))
        })
    })
}

/// Default capability blob for models that carry no capability metadata.
///
/// Claude Code decides whether a model gets an effort picker from
/// `capabilities.effort.supported`, not from the model name, so third-party
/// names (gpt-5.6-luna, deepseek-v4-pro-0813) need this synthesized or the
/// picker never appears.
pub(super) fn synthesize_capabilities(serves_max: bool) -> Value {
    serde_json::json!({
        "effort": {
            "supported": true,
            "low": {"supported": true},
            "medium": {"supported": true},
            "high": {"supported": true},
            "xhigh": {"supported": true},
            "max": {"supported": serves_max}
        },
        "thinking": {
            "supported": true,
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

/// Turn a raw upstream `/models` payload into the shape Claude Code consumes.
///
/// Upstreams return either an OpenAI list (`{"data":[{id,...}]}`) or an
/// Anthropic page (`{"data":[...], "has_more":...}`). We walk `data[]`, inject
/// capabilities, prefix non-Claude model ids with `claude-code/` (Claude Code's
/// model picker filters on that prefix), and always emit an Anthropic page.
/// Build the Anthropic-shaped `/v1/models` page Claude Code's discovery expects.
///
/// Ids are passed through verbatim. A `claude-code/` namespace prefix was tried
/// here first, on the assumption Claude Code needed third-party names marked as
/// foreign; the picker silently dropped every prefixed entry instead, so a
/// 7-model catalog showed only the 3 whose names already began with `claude-`.
/// Requests still accept the prefix (see `strip_claude_code_prefix`) so a name
/// persisted by an older build keeps resolving.
pub(super) fn synthesize_models_response(raw: Value) -> Value {
    let data = raw
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let serves_max = upstream_serves_max_effort(&data);
    let models: Vec<Value> = data
        .into_iter()
        .map(|mut m| {
            if m.get("capabilities").is_none() {
                m["capabilities"] = synthesize_capabilities(serves_max);
            }
            if !m.get("max_input_tokens").and_then(Value::as_u64).is_some() {
                m["max_input_tokens"] = serde_json::json!(200000);
            }
            if !m.get("max_tokens").and_then(Value::as_u64).is_some() {
                m["max_tokens"] = serde_json::json!(32000);
            }
            m
        })
        .collect();

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
