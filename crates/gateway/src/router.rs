//! Request routing and handlers

use crate::{
    client::{Endpoint, UpstreamClient},
    config::ResponsesMode,
    failover::FailoverSlot,
    health::{health_check, HealthState},
    middleware::{auth_middleware, logging_middleware, loopback_only_middleware, MiddlewareState},
    model_rewrite::ModelRewriter,
    replay::{classify, ReplayDecision},
    stream_adapter::StreamAdapter,
};
use polydeck_core::responses_chat::{chat_to_response, responses_to_chat, ToolMap};
use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, Response, StatusCode},
    middleware,
    routing::{get, post},
    Json, Router,
};
use bytes::Bytes;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde_json::Value;
use std::{
    convert::Infallible,
    sync::{Arc, OnceLock},
};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info, warn};

#[derive(Clone)]
pub struct AppState {
    pub upstream: UpstreamClient,
    pub rewriter: ModelRewriter,
    pub health: HealthState,
    pub failover: Option<FailoverSlot>,
    pub responses_mode: ResponsesMode,
    pub responses_native: Arc<OnceLock<bool>>,
    pub max_price_per_request: Option<f64>,
    pub rate_limiter_registry: Arc<crate::rate_limiter::RateLimiterRegistry>,
    pub primary_provider_id: String,
    pub rate_limit_settings: polydeck_core::profile::RateLimitSettings,
    pub max_retries: u32,
    pub default_effort_level: Option<String>,
}

impl AppState {
    fn forward_responses_natively(&self) -> bool {
        match self.responses_mode {
            ResponsesMode::Native => true,
            ResponsesMode::Bridge => false,
            ResponsesMode::Auto => self.responses_native.get().copied().unwrap_or(true),
        }
    }

    fn remember_responses_support(&self, native: bool) {
        if self.responses_mode == ResponsesMode::Auto && self.responses_native.set(native).is_ok() {
            if native {
                debug!("Upstream serves /v1/responses natively; passthrough locked in");
            } else {
                info!("Upstream has no /v1/responses endpoint; bridging to /v1/chat/completions");
            }
        }
    }
}

struct UpstreamAttempt {
    response: reqwest::Response,
    switched_to: Option<String>,
}

enum SendError {
    Unavailable(String),
    NotReplayable { provider_id: String, reason: String, error: String },
}

impl SendError {
    fn into_response(self) -> Response<Body> {
        match self {
            SendError::Unavailable(error) => {
                error!("Upstream request failed: {}", error);
                json_error(StatusCode::BAD_GATEWAY, format!("Upstream error: {}", error))
            }
            SendError::NotReplayable { provider_id, reason, error } => {
                warn!("Provider '{}' failed and the request was not replayed ({}): {}", provider_id, reason, error);
                json_error(StatusCode::BAD_GATEWAY, format!(
                    "Provider '{}' failed ({}). AI Deck switched to a backup provider but did not retry this request because it is not safe to replay: {}. Resend the request to use the new provider.",
                    provider_id, error, reason
                ))
            }
        }
    }
}

async fn send_upstream(
    state: &AppState, endpoint: Endpoint, headers: &HeaderMap, body: Value,
) -> Result<UpstreamAttempt, SendError> {
    let estimated_tokens = crate::rate_limiter::estimate_tokens(&body);
    let failover = match state.failover.as_ref() {
        Some(slot) => slot.get().await,
        None => None,
    };

    let Some(failover) = failover else {
        let provider_id = &state.primary_provider_id;
        let limiter = state.rate_limiter_registry.get_or_create(provider_id, &state.rate_limit_settings).await;

        // 1. Acquire token from token bucket (queues asynchronously if limit reached)
        {
            let mut guard = limiter.lock().await;
            if let Err(err) = guard.acquire(estimated_tokens, std::time::Duration::from_secs(90)).await {
                return Err(SendError::Unavailable(err));
            }
        }

        // 2. Send upstream with internal 429 queueing & retry loop
        let mut last_error = String::new();
        for attempt in 0..=state.max_retries {
            let res = state.upstream.send(endpoint, body.clone()).await;
            match res {
                Ok(response) => {
                    let status = response.status();
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        let retry_after = response.headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok())
                            .map(std::time::Duration::from_secs);

                        {
                            let mut guard = limiter.lock().await;
                            guard.on_429(retry_after);
                        }

                        if attempt < state.max_retries {
                            let backoff = retry_after.unwrap_or_else(|| {
                                std::time::Duration::from_secs(2u64.saturating_pow(attempt + 1).clamp(2, 20))
                            });
                            warn!(
                                "Upstream returned 429 (attempt {}/{}); gateway queueing retry in {:.1}s...",
                                attempt + 1, state.max_retries + 1, backoff.as_secs_f64()
                            );
                            tokio::time::sleep(backoff).await;
                            let mut guard = limiter.lock().await;
                            let _ = guard.acquire(estimated_tokens, std::time::Duration::from_secs(60)).await;
                            continue;
                        }
                        return Ok(UpstreamAttempt { response, switched_to: None });
                    }

                    if !status.is_server_error() {
                        let mut guard = limiter.lock().await;
                        guard.on_success();
                    }
                    return Ok(UpstreamAttempt { response, switched_to: None });
                }
                Err(e) => {
                    last_error = e.message;
                    if attempt < state.max_retries {
                        let backoff = std::time::Duration::from_millis(200 * (1 << attempt));
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    return Err(SendError::Unavailable(last_error));
                }
            }
        }
        return Err(SendError::Unavailable(format!("Request failed after {} attempts: {}", state.max_retries + 1, last_error)));
    };

    let (provider_id, response, never_sent, error_text) = match failover.send(endpoint, body.clone()).await {
        Ok((_pid, response)) if !is_failure_status(response.status()) => {
            return Ok(UpstreamAttempt { response, switched_to: None });
        }
        Ok((pid, response)) => {
            let status = response.status();
            (pid, Some(response), false, format!("upstream returned {}", status))
        }
        Err(error) => {
            warn!("Upstream request failed on failover chain: {}", error.message);
            (String::new(), None, error.never_sent, error.message)
        }
    };
    let current = failover.status().await.current_provider_id;
    if current != provider_id {
        let decision = classify(headers, &body, never_sent);
        if let ReplayDecision::Unsafe(reason) = decision {
            return Err(SendError::NotReplayable {
                provider_id, reason: reason.to_string(), error: error_text,
            });
        }
        match failover.send(endpoint, body).await {
            Ok((_, retried)) => return Ok(UpstreamAttempt { response: retried, switched_to: Some(current) }),
            Err(error) => return Err(SendError::Unavailable(error.message)),
        }
    }
    match response {
        Some(response) => Ok(UpstreamAttempt { response, switched_to: None }),
        None => Err(SendError::Unavailable(format!("All providers unavailable: {}", error_text))),
    }
}

fn is_failure_status(status: reqwest::StatusCode) -> bool {
    status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS
}

pub fn build_router(app_state: Arc<AppState>, middleware_state: Arc<MiddlewareState>) -> Router {
    let health_router = Router::new()
        .route("/health", get(health_check))
        .with_state(Arc::new(app_state.health.clone()));
    let api_router = Router::new()
        .route("/v1/responses", post(handle_responses))
        .route("/responses", post(handle_responses))
        .route("/v1/chat/completions", post(handle_chat_completions))
        .route("/chat/completions", post(handle_chat_completions))
        .route("/v1/messages", post(handle_messages))
        .route("/messages", post(handle_messages))
        .route("/v1/messages/count_tokens", post(handle_count_tokens))
        .route("/messages/count_tokens", post(handle_count_tokens))
        .route("/v1/models", get(handle_models))
        .route("/models", get(handle_models))
        .layer(middleware::from_fn_with_state(middleware_state, auth_middleware))
        .with_state(app_state);
    health_router.merge(api_router)
        .layer(middleware::from_fn(logging_middleware))
        .layer(middleware::from_fn(loopback_only_middleware))
}


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

pub fn inject_thinking_if_needed(body: &mut Value, default_effort_level: Option<&str>) {
    if body.get("thinking").is_some() {
        return;
    }

    let effort = default_effort_level.unwrap_or("high");
    let Some(budget_tokens) = effort_to_budget_tokens(effort) else {
        return;
    };

    let max_tokens = body.get("max_tokens").and_then(|v| v.as_u64());

    let effective_budget = match max_tokens {
        Some(max) if max <= 1024 => {
            return;
        }
        Some(max) if budget_tokens >= max => {
            max.saturating_sub(1).max(1024)
        }
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

fn inject_max_price(body: &mut Value, max_price: Option<f64>) {
    if let Some(price) = max_price {
        if let Some(obj) = body.as_object_mut() {
            obj.entry("max_price_per_request").or_insert_with(|| Value::from(price));
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
fn normalize_effort(effort: &str, model: &str) -> Option<String> {
    let clean = effort.trim().to_ascii_lowercase();
    // Auto-thinking models (DeepSeek reasoner/R1, QwQ) ignore effort and error
    // if forced; drop it before the whitelist so even a valid level is stripped.
    let model_lower = model.to_ascii_lowercase();
    if model_lower.contains("reasoner") || model_lower.contains("r1") || model_lower.contains("qwq") {
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
fn sanitize_messages_effort(body: &mut Value, model: &str) -> Option<String> {
    let effort = body.get("reasoning_effort").and_then(Value::as_str)?;
    match normalize_effort(effort, model) {
        Some(clean) => { body["reasoning_effort"] = Value::String(clean.clone()); Some(clean) }
        None => { body.as_object_mut().map(|o| o.remove("reasoning_effort")); None }
    }
}

async fn handle_models(State(state): State<Arc<AppState>>) -> Response<Body> {
    state.health.increment_connections();
    let _guard = ConnectionGuard(&state.health);
    debug!("Processing GET /models request");
    let upstream_resp = match state.upstream.get_models().await {
        Ok(resp) => resp,
        Err(e) => return json_error(StatusCode::BAD_GATEWAY, format!("Failed to fetch models: {}", e)),
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
fn upstream_serves_max_effort(data: &[Value]) -> bool {
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
fn synthesize_capabilities(serves_max: bool) -> Value {
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
fn synthesize_models_response(raw: Value) -> Value {
    let data = raw.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
    let serves_max = upstream_serves_max_effort(&data);
    let models: Vec<Value> = data.into_iter().map(|mut m| {
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
    }).collect();

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

/// Strip a `claude-code/` discovery prefix from a model id, returning the
/// upstream-facing name and whether the prefix was present.
fn strip_claude_code_prefix(model: &str) -> (String, bool) {
    match model.strip_prefix("claude-code/") {
        Some(rest) if !rest.is_empty() => (rest.to_string(), true),
        _ => (model.to_string(), false),
    }
}

/// Point `body["model"]` at the upstream model, returning the name the client
/// sent.
///
/// Responses have to echo that original string back: the mapping is many-to-one,
/// so the upstream name cannot be translated back, and handing the client an
/// unfamiliar (or retired) model name makes it show a deprecation warning and
/// persist the wrong model on `/resume`.
fn rewrite_model_in_place(body: &mut Value, rewriter: &ModelRewriter) -> Option<String> {
    let client_model = body.get("model")?.as_str()?.to_string();
    // The `claude-code/` prefix is a picker-only alias, not part of the model
    // name. Strip it before rewriting so `claude-code/gpt-5.6-luna` maps to
    // whatever upstream serves `gpt-5.6-luna`.
    let (bare, had_prefix) = strip_claude_code_prefix(&client_model);
    let rewritten = rewriter.rewrite_request(&bare);
    if had_prefix && rewritten == bare {
        // No rewrite rule touched it; keep the bare name, drop the prefix.
        body["model"] = Value::String(bare);
    } else if rewritten != client_model {
        debug!("Rewrote model {} -> {}", client_model, rewritten);
        body["model"] = Value::String(rewritten);
    }
    Some(client_model)
}

async fn handle_messages(
    State(state): State<Arc<AppState>>, headers: HeaderMap, Json(mut body): Json<Value>,
) -> Response<Body> {
    state.health.increment_connections();
    let _guard = ConnectionGuard(&state.health);
    debug!("Processing /messages request");
    let client_model = rewrite_model_in_place(&mut body, &state.rewriter);
    inject_thinking_if_needed(&mut body, state.default_effort_level.as_deref());
    let upstream_model = body.get("model").and_then(Value::as_str).map(str::to_string).unwrap_or_default();
    sanitize_messages_effort(&mut body, &upstream_model);
    let is_stream = body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let start = std::time::Instant::now();
    let attempt = match send_upstream(&state, Endpoint::Messages, &headers, body).await {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    if let Some(p) = &attempt.switched_to { info!("Request retried on failover provider '{}'", p); }
    let upstream_response = attempt.response;
    state.health.record_request(start.elapsed().as_millis() as u64);
    if !upstream_response.status().is_success() { return passthrough_verbatim(upstream_response).await; }
    if is_stream { passthrough_raw_stream(upstream_response) }
    else { passthrough_nonstream(upstream_response, client_model).await }
}

async fn handle_count_tokens(
    State(state): State<Arc<AppState>>, headers: HeaderMap, Json(mut body): Json<Value>,
) -> Response<Body> {
    state.health.increment_connections();
    let _guard = ConnectionGuard(&state.health);
    debug!("Processing /messages/count_tokens request");
    rewrite_model_in_place(&mut body, &state.rewriter);
    match send_upstream(&state, Endpoint::CountTokens, &headers, body.clone()).await {
        Ok(attempt) => {
            if attempt.response.status().is_success() {
                passthrough_verbatim(attempt.response).await
            } else {
                let tokens = crate::rate_limiter::estimate_tokens(&body).max(1);
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::json!({ "input_tokens": tokens }).to_string()))
                    .unwrap()
            }
        }
        Err(_) => {
            let tokens = crate::rate_limiter::estimate_tokens(&body).max(1);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::json!({ "input_tokens": tokens }).to_string()))
                .unwrap()
        }
    }
}

async fn handle_responses(
    State(state): State<Arc<AppState>>, headers: HeaderMap, Json(mut body): Json<Value>,
) -> Response<Body> {
    state.health.increment_connections();
    let _guard = ConnectionGuard(&state.health);
    debug!("Processing /v1/responses request");
    inject_max_price(&mut body, state.max_price_per_request);
    sanitize_responses_effort(&mut body);
    // A tool type outside the narrow set real upstreams implement has to go
    // through the bridge: `responses_to_chat` renames the tool and records the
    // mapping so the reply can be rewritten back. Native passthrough returns
    // the upstream reply verbatim, so converting only the request would hand
    // the client a tool name it never registered.
    if requires_tool_bridge(&body) {
        debug!("Responses body carries non-native tool types; bridging");
        return handle_bridged_responses(&state, &headers, body).await;
    }
    if state.forward_responses_natively() {
        return handle_native_responses(&state, &headers, body).await;
    }
    handle_bridged_responses(&state, &headers, body).await
}

/// Tool types an OpenAI-compatible Responses upstream can be expected to accept.
///
/// The published spec has far more (`custom`, `namespace`, `apply_patch`,
/// `local_shell`, `file_search`, ...), but real relays implement a subset and
/// reject the rest outright with a deserialization error. Probed against
/// Agnes: `function` is accepted; `custom`, `namespace`, `apply_patch` and
/// `file_search` all fail with `unknown variant`.
const NATIVE_RESPONSES_TOOL_TYPES: [&str; 4] =
    ["function", "web_search_preview", "code_interpreter", "mcp"];

/// True when `tools[]` holds a type native passthrough cannot safely carry.
fn requires_tool_bridge(body: &Value) -> bool {
    let Some(tools) = body.get("tools").and_then(Value::as_array) else { return false };
    tools.iter().any(|tool| {
        // A tool with no `type` is malformed; leave the verdict to the upstream
        // rather than forcing a bridge on it.
        tool.get("type")
            .and_then(Value::as_str)
            .is_some_and(|t| !NATIVE_RESPONSES_TOOL_TYPES.contains(&t))
    })
}

/// Sanitize reasoning effort on an OpenAI Responses body (`/reasoning/effort`).
/// The model lives at `/model`; missing it means no upstream name to judge the
/// effort against, so pass through untouched.
fn sanitize_responses_effort(body: &mut Value) {
    let Some(effort) = body.pointer("/reasoning/effort").and_then(Value::as_str) else { return };
    let model = body.get("model").and_then(Value::as_str).unwrap_or("");
    match normalize_effort(effort, model) {
        Some(clean) => {
            if let Some(reasoning) = body.get_mut("reasoning").and_then(|r| r.as_object_mut()) {
                reasoning.insert("effort".to_string(), Value::String(clean));
            }
        }
        None => {
            let now_empty = match body.get_mut("reasoning").and_then(|r| r.as_object_mut()) {
                Some(reasoning) => {
                    reasoning.remove("effort");
                    reasoning.is_empty()
                }
                None => false,
            };
            // Don't ship a bare `reasoning: {}`; picky relays reject it.
            if now_empty {
                body.as_object_mut().map(|o| o.remove("reasoning"));
            }
        }
    }
}

async fn handle_native_responses(state: &AppState, headers: &HeaderMap, body: Value) -> Response<Body> {
    let is_stream = body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let start = std::time::Instant::now();
    let attempt = match send_upstream(state, Endpoint::Responses, headers, body.clone()).await {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    if let Some(p) = &attempt.switched_to { info!("Request retried on failover provider '{}'", p); }
    let upstream_response = attempt.response;
    state.health.record_request(start.elapsed().as_millis() as u64);
    // Bridge-on-rejection applies in Native too, not just Auto: a provider
    // pinned to Native still gets 400'd by an upstream that only implements
    // part of the Responses shape, and passing that through just fails the
    // user's request. Bridge mode is excluded — it never reaches here.
    if !upstream_response.status().is_success() {
        let status = upstream_response.status();
        let resp_headers = upstream_response.headers().clone();
        let resp_body = upstream_response.bytes().await.unwrap_or_default();
        if is_missing_responses_error(status, &resp_body) {
            info!("Upstream rejected /v1/responses with {}; falling back to bridge", status);
            state.remember_responses_support(false);
            return handle_bridged_responses(state, headers, body).await;
        }
        if state.responses_mode == ResponsesMode::Auto {
            state.remember_responses_support(true);
        }
        return response_with_body(status, &resp_headers, resp_body);
    }
    state.remember_responses_support(true);
    if is_stream { passthrough_raw_stream(upstream_response) }
    else { passthrough_verbatim(upstream_response).await }
}

async fn handle_bridged_responses(state: &AppState, headers: &HeaderMap, body: Value) -> Response<Body> {
    let converted = match responses_to_chat(&body, None) {
        Ok(c) => c,
        Err(e) => { error!("Failed to convert request: {}", e); return json_error(StatusCode::UNPROCESSABLE_ENTITY, e.to_string()); }
    };
    let mut chat_body = converted.body;
    let tools = converted.tools;
    let client_model = rewrite_model_in_place(&mut chat_body, &state.rewriter);
    let is_stream = chat_body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let start = std::time::Instant::now();
    let attempt = match send_upstream(state, Endpoint::ChatCompletions, headers, chat_body).await {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    if let Some(p) = &attempt.switched_to { info!("Request retried on failover provider '{}'", p); }
    let upstream_response = attempt.response;
    state.health.record_request(start.elapsed().as_millis() as u64);
    if !upstream_response.status().is_success() { return passthrough_verbatim(upstream_response).await; }
    if is_stream { handle_responses_stream(upstream_response, client_model.unwrap_or_default()).await }
    else { handle_responses_nonstream(upstream_response, client_model, &tools).await }
}

const POOL_REJECTION_MARKERS: [&str; 6] = [
    "no safe maximum price", "per-request maximum price",
    "continuation or media usage",
    "invalid url", "unsupported endpoint", "unknown endpoint",
];

/// Markers for an upstream that speaks Responses but rejects part of the request
/// shape it was handed — most often a tool type it never implemented.
///
/// `requires_tool_bridge` catches the known-bad types up front; this is the net
/// for upstreams whose accepted set differs from the one we assume. Retrying
/// through the bridge downgrades the request to Chat Completions, which every
/// OpenAI-compatible upstream implements.
const SHAPE_REJECTION_MARKERS: [&str; 4] = [
    "unknown variant",
    "json_parse_error",
    "failed to deserialize the json body",
    "unknown field",
];

fn is_missing_responses_error(status: reqwest::StatusCode, body: &[u8]) -> bool {
    if matches!(status, reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED | reqwest::StatusCode::NOT_IMPLEMENTED) {
        return true;
    }
    if !matches!(status, reqwest::StatusCode::BAD_REQUEST | reqwest::StatusCode::UNPROCESSABLE_ENTITY) {
        return false;
    }
    let detail = String::from_utf8_lossy(body).to_ascii_lowercase();
    POOL_REJECTION_MARKERS.iter().any(|m| detail.contains(m))
        || SHAPE_REJECTION_MARKERS.iter().any(|m| detail.contains(m))
}

/// Universal SSE stream idle timeout (25s without data chunk).
pub const SSE_STREAM_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(25);

fn passthrough_raw_stream(upstream_response: reqwest::Response) -> Response<Body> {
    let status = StatusCode::from_u16(upstream_response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream_response.headers().get(header::CONTENT_TYPE).cloned();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, Infallible>>(100);
    tokio::spawn(async move {
        let mut byte_stream = upstream_response.bytes_stream();
        loop {
            let next_item = tokio::time::timeout(SSE_STREAM_IDLE_TIMEOUT, byte_stream.next()).await;
            match next_item {
                Ok(Some(Ok(bytes))) => {
                    if tx.send(Ok(bytes)).await.is_err() { return; }
                }
                Ok(Some(Err(e))) => {
                    let err_msg = serde_json::json!({"error": format!("Stream read error: {e}")}).to_string();
                    let _ = tx.send(Ok(Bytes::from(format!("data: {err_msg}\n\n")))).await;
                    return;
                }
                Ok(None) => break,
                Err(_elapsed) => {
                    warn!("Upstream raw stream idle timeout (25s without data); closing connection");
                    let err_msg = serde_json::json!({
                        "error": {
                            "message": "Upstream stream idle timeout: no data chunk received for 25s",
                            "type": "timeout_error",
                            "code": 504
                        }
                    }).to_string();
                    let _ = tx.send(Ok(Bytes::from(format!("data: {err_msg}\n\n")))).await;
                    return;
                }
            }
        }
    });
    let builder = Response::builder().status(status)
        .header(header::CONTENT_TYPE, content_type.unwrap_or(header::HeaderValue::from_static("text/event-stream")))
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive");
    builder.body(Body::from_stream(ReceiverStream::new(rx))).unwrap()
}

async fn handle_chat_completions(
    State(state): State<Arc<AppState>>, headers: HeaderMap, Json(mut body): Json<Value>,
) -> Response<Body> {
    state.health.increment_connections();
    let _guard = ConnectionGuard(&state.health);
    debug!("Processing /v1/chat/completions request");
    let client_model = rewrite_model_in_place(&mut body, &state.rewriter);
    let is_stream = body.get("stream").and_then(|s| s.as_bool()).unwrap_or(false);
    let start = std::time::Instant::now();
    let attempt = match send_upstream(&state, Endpoint::ChatCompletions, &headers, body).await {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    if let Some(p) = &attempt.switched_to { info!("Request retried on failover provider '{}'", p); }
    let upstream_response = attempt.response;
    state.health.record_request(start.elapsed().as_millis() as u64);
    if !upstream_response.status().is_success() { return passthrough_verbatim(upstream_response).await; }
    if is_stream { passthrough_stream(upstream_response, client_model).await }
    else { passthrough_nonstream(upstream_response, client_model).await }
}

async fn handle_responses_nonstream(upstream_response: reqwest::Response, client_model: Option<String>, tools: &ToolMap) -> Response<Body> {
    let chat_response: Value = match upstream_response.json().await {
        Ok(v) => v,
        Err(e) => { error!("Failed to parse upstream JSON: {}", e); return json_error(StatusCode::BAD_GATEWAY, "Invalid upstream response"); }
    };
    let mut resp = match chat_to_response(&chat_response, tools) {
        Ok(r) => r,
        Err(e) => { error!("Failed to convert response: {}", e); return json_error(StatusCode::BAD_GATEWAY, e.to_string()); }
    };
    restore_client_model(&mut resp, client_model.as_deref());
    Response::builder().status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(resp.to_string())).unwrap()
}

async fn handle_responses_stream(upstream_response: reqwest::Response, client_model: String) -> Response<Body> {
    let mut adapter = StreamAdapter::new(client_model);
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, Infallible>>(100);
    for event in adapter.start() { let _ = tx.send(Ok(Bytes::from(event))).await; }
    tokio::spawn(async move {
        let mut upstream_events = upstream_response.bytes_stream().eventsource();
        loop {
            let next_item = tokio::time::timeout(SSE_STREAM_IDLE_TIMEOUT, upstream_events.next()).await;
            match next_item {
                Ok(Some(Ok(event))) if event.data == "[DONE]" => break,
                Ok(Some(Ok(event))) => match serde_json::from_str::<Value>(&event.data) {
                    Ok(chunk) => {
                        for converted in adapter.push_chat_chunk(&chunk) {
                            if tx.send(Ok(Bytes::from(converted))).await.is_err() { return; }
                        }
                    }
                    Err(e) => {
                        let err_msg = serde_json::json!({"error": format!("Parse error: {e}")}).to_string();
                        let _ = tx.send(Ok(Bytes::from(format!("event: error\ndata: {err_msg}\n\n")))).await;
                        return;
                    }
                },
                Ok(Some(Err(e))) => {
                    let err_msg = serde_json::json!({"error": format!("Stream error: {e}")}).to_string();
                    let _ = tx.send(Ok(Bytes::from(format!("event: error\ndata: {err_msg}\n\n")))).await;
                    return;
                }
                Ok(None) => break,
                Err(_elapsed) => {
                    warn!("Upstream responses SSE stream idle timeout (25s without data); closing connection");
                    let err_msg = serde_json::json!({
                        "error": {
                            "message": "Upstream SSE stream idle timeout: no data chunk received for 25s",
                            "type": "timeout_error",
                            "code": 504
                        }
                    }).to_string();
                    let _ = tx.send(Ok(Bytes::from(format!("event: error\ndata: {err_msg}\n\n")))).await;
                    return;
                }
            }
        }
        for event in adapter.finish() { let _ = tx.send(Ok(Bytes::from(event))).await; }
    });
    Response::builder().status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(Body::from_stream(ReceiverStream::new(rx))).unwrap()
}

/// Put the client's own model name back on a response body.
///
/// No-op when the client sent no model or the response carries none, so an
/// upstream that omits the field keeps omitting it.
fn restore_client_model(json: &mut Value, client_model: Option<&str>) {
    let Some(client_model) = client_model else { return };
    if json.get("model").and_then(|m| m.as_str()).is_some_and(|m| m != client_model) {
        json["model"] = Value::String(client_model.to_string());
    }
}

async fn passthrough_nonstream(upstream_response: reqwest::Response, client_model: Option<String>) -> Response<Body> {
    let mut json: Value = match upstream_response.json().await {
        Ok(v) => v,
        Err(e) => { error!("Failed to parse upstream JSON: {}", e); return json_error(StatusCode::BAD_GATEWAY, "Invalid upstream response"); }
    };
    restore_client_model(&mut json, client_model.as_deref());
    Response::builder().status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json.to_string())).unwrap()
}

async fn passthrough_stream(upstream_response: reqwest::Response, client_model: Option<String>) -> Response<Body> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, Infallible>>(100);
    tokio::spawn(async move {
        let mut events = upstream_response.bytes_stream().eventsource();
        loop {
            let next_item = tokio::time::timeout(SSE_STREAM_IDLE_TIMEOUT, events.next()).await;
            let chunk = match next_item {
                Ok(Some(Ok(event))) if event.data == "[DONE]" => {
                    let _ = tx.send(Ok(Bytes::from("data: [DONE]\n\n"))).await;
                    return;
                }
                Ok(Some(Ok(event))) => event.data,
                Ok(Some(Err(e))) => {
                    let err_msg = serde_json::json!({"error": format!("Stream error: {e}")}).to_string();
                    let _ = tx.send(Ok(Bytes::from(format!("data: {err_msg}\n\n")))).await;
                    return;
                }
                Ok(None) => return,
                Err(_elapsed) => {
                    warn!("Upstream chat completions SSE stream idle timeout (25s without data); closing connection");
                    let err_msg = serde_json::json!({
                        "error": {
                            "message": "Upstream SSE stream idle timeout: no data chunk received for 25s",
                            "type": "timeout_error",
                            "code": 504
                        }
                    }).to_string();
                    let _ = tx.send(Ok(Bytes::from(format!("data: {err_msg}\n\n")))).await;
                    return;
                }
            };
            let payload = match serde_json::from_str::<Value>(&chunk) {
                Ok(mut json) => {
                    restore_client_model(&mut json, client_model.as_deref());
                    format!("data: {}\n\n", json)
                }
                Err(_) => format!("data: {}\n\n", chunk),
            };
            if tx.send(Ok(Bytes::from(payload))).await.is_err() { return; }
        }
    });
    Response::builder().status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(Body::from_stream(ReceiverStream::new(rx))).unwrap()
}

async fn passthrough_verbatim(upstream_response: reqwest::Response) -> Response<Body> {
    let status = StatusCode::from_u16(upstream_response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream_response.headers().get(header::CONTENT_TYPE).cloned();
    let body_bytes = upstream_response.bytes().await.unwrap_or_default();
    let mut builder = Response::builder().status(status);
    if let Some(ct) = content_type { builder = builder.header(header::CONTENT_TYPE, ct); }
    builder.body(Body::from(body_bytes)).unwrap()
}

fn response_with_body(status: reqwest::StatusCode, headers: &HeaderMap, body: Bytes) -> Response<Body> {
    let status = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut builder = Response::builder().status(status);
    if let Some(ct) = headers.get(header::CONTENT_TYPE) { builder = builder.header(header::CONTENT_TYPE, ct); }
    builder.body(Body::from(body)).unwrap()
}

fn json_error(status: StatusCode, message: impl Into<String>) -> Response<Body> {
    let body = serde_json::json!({
        "error": { "message": message.into(), "type": "gateway_error", "code": status.as_u16() }
    });
    Response::builder().status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string())).unwrap()
}

struct ConnectionGuard<'a>(&'a HealthState);
impl Drop for ConnectionGuard<'_> {
    fn drop(&mut self) { self.0.decrement_connections(); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn state(responses_mode: ResponsesMode) -> AppState {
        AppState {
            upstream: UpstreamClient::new("https://api.example.com".into(), "key".into(), Duration::from_secs(5), 0).unwrap(),
            rewriter: ModelRewriter::new(&[]).unwrap(),
            health: HealthState::new(),
            failover: None,
            responses_mode,
            responses_native: Arc::new(OnceLock::new()),
            max_price_per_request: None,
            rate_limiter_registry: Arc::new(crate::rate_limiter::RateLimiterRegistry::new()),
            primary_provider_id: "test".into(),
            rate_limit_settings: polydeck_core::profile::RateLimitSettings::default(),
            max_retries: 3,
            default_effort_level: None,
        }
    }

    #[test]
    fn response_echoes_the_name_the_client_sent() {
        let rules = crate::model_rewrite::generate_provider_model_rewrites(
            &["glm-4.6".to_string()],
            false,
        );
        let rewriter = ModelRewriter::new(&rules).unwrap();

        let mut request = serde_json::json!({ "model": "claude-opus-5" });
        let client_model = rewrite_model_in_place(&mut request, &rewriter);
        assert_eq!(client_model.as_deref(), Some("claude-opus-5"));
        assert_eq!(request["model"], "glm-4.6");

        let mut response = serde_json::json!({ "model": "glm-4.6", "id": "msg_1" });
        restore_client_model(&mut response, client_model.as_deref());
        assert_eq!(response["model"], "claude-opus-5");
    }

    #[test]
    fn restore_client_model_leaves_a_modelless_response_alone() {
        let mut response = serde_json::json!({ "id": "msg_1" });
        restore_client_model(&mut response, Some("claude-opus-5"));
        assert!(response.get("model").is_none());
    }

    #[test]
    fn native_mode_always_passes_through() {
        let s = state(ResponsesMode::Native);
        assert!(s.forward_responses_natively());
        s.remember_responses_support(false);
        assert!(s.forward_responses_natively());
    }

    /// Probed against Agnes: only these four survive native passthrough.
    #[test]
    fn native_tool_types_do_not_force_a_bridge() {
        for t in ["function", "web_search_preview", "code_interpreter", "mcp"] {
            let body = serde_json::json!({ "model": "m", "tools": [{ "type": t }] });
            assert!(!requires_tool_bridge(&body), "{t} should stay native");
        }
    }

    /// The Agnes 400 listed `function`/`web_search_preview`/`code_interpreter`/`mcp`
    /// as the accepted set; every other spec type has to be bridged.
    #[test]
    fn spec_tool_types_outside_the_native_set_force_a_bridge() {
        for t in [
            "custom", "namespace", "apply_patch", "file_search", "local_shell",
            "shell", "computer", "computer_use_preview", "image_generation",
            "tool_search", "web_search",
        ] {
            let body = serde_json::json!({ "model": "m", "tools": [{ "type": t }] });
            assert!(requires_tool_bridge(&body), "{t} should bridge");
        }
    }

    #[test]
    fn one_bad_tool_among_many_bridges_the_whole_request() {
        // Codex sends the offending `custom` tool alongside plain functions;
        // the original report had it at tools[7].
        let mut tools: Vec<Value> = (0..7)
            .map(|i| serde_json::json!({ "type": "function", "name": format!("f{i}") }))
            .collect();
        tools.push(serde_json::json!({ "type": "custom", "name": "apply_patch" }));
        let body = serde_json::json!({ "model": "agnes-2.5-flash", "tools": tools });
        assert!(requires_tool_bridge(&body));
    }

    #[test]
    fn absent_or_empty_tools_stay_native() {
        assert!(!requires_tool_bridge(&serde_json::json!({ "model": "m" })));
        assert!(!requires_tool_bridge(&serde_json::json!({ "model": "m", "tools": [] })));
        // Not an array — nothing to inspect, leave the verdict upstream.
        assert!(!requires_tool_bridge(&serde_json::json!({ "model": "m", "tools": "x" })));
    }

    #[test]
    fn a_typeless_tool_is_left_for_the_upstream_to_reject() {
        let body = serde_json::json!({ "model": "m", "tools": [{ "name": "no_type" }] });
        assert!(!requires_tool_bridge(&body));
    }

    /// The verbatim Agnes rejection must trigger the bridge fallback.
    #[test]
    fn agnes_tool_type_rejection_triggers_bridge_fallback() {
        let agnes = br#"{"error":{"message":"***.BadRequestError: OpenAIException - {\"error\":{\"message\":\"Invalid JSON data: Failed to deserialize the JSON body into the target type: tools[7].type: unknown variant `custom`, expected one of `function`, `web_search_preview`, `code_interpreter`, `mcp` at line 1 column 46194\",\"type\":\"invalid_request_error\",\"code\":\"json_parse_error\"}}","type":"upstream_error","param":"","code":"400"}}"#;
        assert!(is_missing_responses_error(reqwest::StatusCode::BAD_REQUEST, agnes));
    }

    #[test]
    fn shape_rejections_are_recognised_generically() {
        for marker in [
            b"unknown variant `apply_patch`".as_slice(),
            b"json_parse_error".as_slice(),
            b"Failed to deserialize the JSON body".as_slice(),
            b"unknown field `defer_loading`".as_slice(),
        ] {
            assert!(
                is_missing_responses_error(reqwest::StatusCode::BAD_REQUEST, marker),
                "{}", String::from_utf8_lossy(marker)
            );
        }
    }

    /// An ordinary 400 must still surface to the client, not silently re-run.
    #[test]
    fn unrelated_bad_requests_do_not_trigger_fallback() {
        for body in [
            b"{\"error\":{\"message\":\"insufficient quota\"}}".as_slice(),
            b"{\"error\":{\"message\":\"model not found\"}}".as_slice(),
            b"{\"error\":{\"message\":\"context length exceeded\"}}".as_slice(),
        ] {
            assert!(!is_missing_responses_error(reqwest::StatusCode::BAD_REQUEST, body));
        }
        // 401/403/429/500 are never a shape problem regardless of body text.
        for status in [
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            assert!(!is_missing_responses_error(status, b"unknown variant `custom`"));
        }
    }

    #[test]
    fn bridge_mode_always_converts() {
        let s = state(ResponsesMode::Bridge);
        assert!(!s.forward_responses_natively());
        s.remember_responses_support(true);
        assert!(!s.forward_responses_natively());
    }

    #[test]
    fn auto_mode_tries_native_first() {
        assert!(state(ResponsesMode::Auto).forward_responses_natively());
    }

    #[test]
    fn auto_mode_keeps_first_answer() {
        let s = state(ResponsesMode::Auto);
        s.remember_responses_support(false);
        assert!(!s.forward_responses_natively());
        s.remember_responses_support(true);
        assert!(!s.forward_responses_natively());
    }

    #[test]
    fn endpoint_absence_triggers_bridge() {
        for status in [reqwest::StatusCode::NOT_FOUND, reqwest::StatusCode::METHOD_NOT_ALLOWED, reqwest::StatusCode::NOT_IMPLEMENTED] {
            assert!(is_missing_responses_error(status, b""), "{status}");
        }
    }
    #[test]
    fn test_inject_thinking_logic() {
        let mut body1 = serde_json::json!({
            "model": "claude-opus-5",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 32000,
            "temperature": 0.7
        });
        inject_thinking_if_needed(&mut body1, Some("high"));
        assert_eq!(body1["thinking"]["type"], "enabled");
        assert_eq!(body1["thinking"]["budget_tokens"], 16384);
        assert_eq!(body1["temperature"], 1.0);

        let mut body2 = serde_json::json!({
            "model": "claude-opus-5",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 2000
        });
        inject_thinking_if_needed(&mut body2, Some("max"));
        assert_eq!(body2["thinking"]["type"], "enabled");
        assert_eq!(body2["thinking"]["budget_tokens"], 1999);

        let mut body3 = serde_json::json!({
            "model": "claude-opus-5",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 500
        });
        inject_thinking_if_needed(&mut body3, Some("high"));
        assert!(body3.get("thinking").is_none());

        let mut body4 = serde_json::json!({
            "model": "claude-opus-5",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 4000
        });
        inject_thinking_if_needed(&mut body4, Some("none"));
        assert!(body4.get("thinking").is_none());

        let mut body5 = serde_json::json!({
            "model": "claude-opus-5",
            "messages": [{"role": "user", "content": "hello"}],
            "thinking": { "type": "enabled", "budget_tokens": 4000 }
        });
        inject_thinking_if_needed(&mut body5, Some("high"));
        assert_eq!(body5["thinking"]["budget_tokens"], 4000);
    }

    #[test]
    fn normalize_effort_keeps_whitelisted_levels() {
        for level in ["low", "medium", "high", "xhigh", "max"] {
            assert_eq!(normalize_effort(level, "gpt-5.6-luna"), Some(level.to_string()), "{level}");
            assert_eq!(normalize_effort(&level.to_uppercase(), "gpt-5.6-luna"), Some(level.to_string()), "{level} upper");
        }
    }

    #[test]
    fn normalize_effort_maps_minimal_to_low() {
        assert_eq!(normalize_effort("minimal", "gpt-5.6-luna"), Some("low".to_string()));
    }

    #[test]
    fn normalize_effort_drops_disabled_and_unknown() {
        assert_eq!(normalize_effort("none", "gpt-5.6-luna"), None);
        assert_eq!(normalize_effort("off", "gpt-5.6-luna"), None);
        assert_eq!(normalize_effort("bogus", "gpt-5.6-luna"), None);
    }

    #[test]
    fn normalize_effort_drops_for_autothinking_models() {
        // DeepSeek reasoner variants and QwQ auto-think; effort would 400.
        assert_eq!(normalize_effort("high", "deepseek-r1"), None);
        assert_eq!(normalize_effort("high", "deepseek-reasoner"), None);
        assert_eq!(normalize_effort("high", "qwen-qwq-32b"), None);
        // A plain DeepSeek model keeps a valid effort.
        assert_eq!(normalize_effort("high", "deepseek-v4-pro-0813"), Some("high".to_string()));
    }

    #[test]
    fn sanitize_messages_effort_removes_bad_field() {
        let mut body = serde_json::json!({ "model": "deepseek-v4-pro-0813", "reasoning_effort": "bogus" });
        sanitize_messages_effort(&mut body, "deepseek-v4-pro-0813");
        assert!(body.get("reasoning_effort").is_none());

        let mut body2 = serde_json::json!({ "model": "deepseek-v4-pro-0813", "reasoning_effort": "max" });
        sanitize_messages_effort(&mut body2, "deepseek-v4-pro-0813");
        assert_eq!(body2["reasoning_effort"], "max");
    }

    #[test]
    fn sanitize_responses_effort_rewrites_in_reasoning_object() {
        let mut body = serde_json::json!({
            "model": "gpt-5.6-luna",
            "reasoning": { "effort": "minimal" }
        });
        sanitize_responses_effort(&mut body);
        assert_eq!(body["reasoning"]["effort"], "low");

        // Effort was the only key, so the empty `reasoning` object goes too.
        let mut body2 = serde_json::json!({
            "model": "qwen-qwq-32b",
            "reasoning": { "effort": "high" }
        });
        sanitize_responses_effort(&mut body2);
        assert!(body2.get("reasoning").is_none());

        // Siblings survive: only `effort` is stripped.
        let mut body3 = serde_json::json!({
            "model": "qwen-qwq-32b",
            "reasoning": { "effort": "high", "summary": "none" }
        });
        sanitize_responses_effort(&mut body3);
        assert!(body3["reasoning"].get("effort").is_none());
        assert_eq!(body3["reasoning"]["summary"], "none");

        // `none` from a stale config.toml is what 400'd the relay; it must drop.
        let mut body4 = serde_json::json!({
            "model": "deepseek-v4-pro-0813",
            "reasoning": { "effort": "none", "summary": "none" }
        });
        sanitize_responses_effort(&mut body4);
        assert!(body4["reasoning"].get("effort").is_none());
    }

    /// Ids reach the picker verbatim. Namespacing third-party names under
    /// `claude-code/` made Claude Code drop them from the picker entirely, so a
    /// catalog of 7 offered only the 3 already named `claude-*`.
    #[test]
    fn synthesize_models_response_passes_ids_through_verbatim() {
        let raw = serde_json::json!({
            "data": [
                { "id": "gpt-5.6-luna" },
                { "id": "claude-opus-5" },
                { "id": "deepseek-v4-pro-0813" }
            ]
        });
        let resp = synthesize_models_response(raw);
        let ids: Vec<&str> = resp["data"].as_array().unwrap().iter()
            .map(|m| m["id"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["gpt-5.6-luna", "claude-opus-5", "deepseek-v4-pro-0813"]);

        // Every model gets capabilities with effort enabled and Anthropic page fields.
        assert_eq!(resp["has_more"], false);
        assert_eq!(resp["first_id"], "gpt-5.6-luna");
        assert_eq!(resp["last_id"], "deepseek-v4-pro-0813");
        for m in resp["data"].as_array().unwrap() {
            assert_eq!(m["capabilities"]["effort"]["supported"], true);
            // No `-max` id in this catalog, so the level stays off.
            assert_eq!(m["capabilities"]["effort"]["max"]["supported"], false);
        }
    }

    /// A relay that exposes `max` as its own model id does serve the level, so
    /// the picker must offer it. Hard-coding it off hid the level from exactly
    /// the upstreams that have it.
    #[test]
    fn max_effort_offered_when_upstream_advertises_a_max_model() {
        let raw = serde_json::json!({
            "data": [
                { "id": "claude-opus-5" },
                { "id": "claude-opus-5-max" },
                { "id": "model-T" }
            ]
        });
        let resp = synthesize_models_response(raw);
        for m in resp["data"].as_array().unwrap() {
            assert_eq!(m["capabilities"]["effort"]["max"]["supported"], true);
            assert_eq!(m["capabilities"]["effort"]["xhigh"]["supported"], true);
        }
    }

    fn ids_to_data(ids: &[&str]) -> Vec<Value> {
        ids.iter().map(|id| serde_json::json!({ "id": id })).collect()
    }

    #[test]
    fn max_effort_detection_spans_suffix_spellings() {
        // The separator and any trailing context marker must not hide the pair.
        for ids in [
            ["claude-opus-5", "claude-opus-5-max"],
            ["claude-opus-5", "claude-opus-5_max"],
            ["claude-opus-5", "claude-opus-5:max"],
            ["claude-opus-5", "claude-opus-5-max[1m]"],
            ["claude-opus-5[1m]", "claude-opus-5-max"],
            ["CLAUDE-OPUS-5", "Claude-Opus-5-MAX"],
        ] {
            assert!(
                upstream_serves_max_effort(&ids_to_data(&ids)),
                "{ids:?} advertises a max variant of an advertised base"
            );
        }
    }

    #[test]
    fn max_effort_not_inferred_from_product_names() {
        // `max` as part of a model tier's own name is not an effort level, and
        // offering it would only produce requests the upstream rejects.
        for ids in [
            vec!["qwen-max", "qwen-plus"],
            vec!["kimi-max", "kimi-k2"],
            vec!["glm-4-max", "glm-4-air"],
            vec!["gpt-maxi", "gpt"],
            // No base model advertised alongside it.
            vec!["claude-opus-5-max"],
            // Nothing resembling max at all.
            vec!["claude-opus-5", "claude-opus-5-xhigh"],
        ] {
            assert!(
                !upstream_serves_max_effort(&ids_to_data(&ids)),
                "{ids:?} carries no evidence of a max effort level"
            );
        }
    }

    #[test]
    fn synthesize_models_response_preserves_existing_capabilities() {
        let raw = serde_json::json!({
            "data": [
                { "id": "m1", "capabilities": { "effort": { "supported": false } } }
            ]
        });
        let resp = synthesize_models_response(raw);
        assert_eq!(resp["data"][0]["capabilities"]["effort"]["supported"], false);
    }

    #[test]
    fn rewrite_model_in_place_strips_claude_code_prefix() {
        let rules = crate::model_rewrite::generate_provider_model_rewrites(
            &["gpt-5.6-luna".to_string()],
            false,
        );
        let rewriter = ModelRewriter::new(&rules).unwrap();

        let mut body = serde_json::json!({ "model": "claude-code/gpt-5.6-luna" });
        let client_model = rewrite_model_in_place(&mut body, &rewriter);
        // Client echo keeps the full picker id.
        assert_eq!(client_model.as_deref(), Some("claude-code/gpt-5.6-luna"));
        // Upstream gets the bare name (self-map passthrough).
        assert_eq!(body["model"], "gpt-5.6-luna");
    }

    #[test]
    fn strip_claude_code_prefix_edge_cases() {
        assert_eq!(strip_claude_code_prefix("claude-code/gpt-5.6-luna").0, "gpt-5.6-luna");
        assert_eq!(strip_claude_code_prefix("claude-code/gpt-5.6-luna").1, true);
        assert_eq!(strip_claude_code_prefix("gpt-5.6-luna").0, "gpt-5.6-luna");
        assert_eq!(strip_claude_code_prefix("gpt-5.6-luna").1, false);
        assert_eq!(strip_claude_code_prefix("claude-code/").0, "claude-code/");
    }

}
