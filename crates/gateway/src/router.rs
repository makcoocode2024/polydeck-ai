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

fn inject_max_price(body: &mut Value, max_price: Option<f64>) {
    if let Some(price) = max_price {
        if let Some(obj) = body.as_object_mut() {
            obj.entry("max_price_per_request").or_insert_with(|| Value::from(price));
        }
    }
}

async fn handle_models(State(state): State<Arc<AppState>>) -> Response<Body> {
    state.health.increment_connections();
    let _guard = ConnectionGuard(&state.health);
    debug!("Processing GET /models request");
    match state.upstream.get_models().await {
        Ok(resp) => {
            if resp.status().is_success() {
                let mut json: Value = match resp.json().await {
                    Ok(v) => v,
                    Err(_) => return json_error(StatusCode::BAD_GATEWAY, "Invalid models upstream response"),
                };
                if let Some(arr) = json.get_mut("data").and_then(|d| d.as_array_mut()) {
                    for item in arr.iter_mut() {
                        if let Some(id) = item.get("id").and_then(|i| i.as_str()) {
                            let original = state.rewriter.rewrite_response(id);
                            if original != id {
                                item["id"] = Value::String(original);
                            }
                        }
                    }
                }
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json.to_string()))
                    .unwrap()
            } else {
                passthrough_verbatim(resp).await
            }
        }
        Err(e) => json_error(StatusCode::BAD_GATEWAY, format!("Failed to fetch models: {}", e)),
    }
}

async fn handle_messages(
    State(state): State<Arc<AppState>>, headers: HeaderMap, Json(mut body): Json<Value>,
) -> Response<Body> {
    state.health.increment_connections();
    let _guard = ConnectionGuard(&state.health);
    debug!("Processing /messages request");
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        let rewritten = state.rewriter.rewrite_request(model);
        if rewritten != model {
            debug!("Rewrote model {} -> {}", model, rewritten);
            body["model"] = Value::String(rewritten);
        }
    }
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
    else { passthrough_nonstream(upstream_response, &state.rewriter).await }
}

async fn handle_count_tokens(
    State(state): State<Arc<AppState>>, headers: HeaderMap, Json(mut body): Json<Value>,
) -> Response<Body> {
    state.health.increment_connections();
    let _guard = ConnectionGuard(&state.health);
    debug!("Processing /messages/count_tokens request");
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        let rewritten = state.rewriter.rewrite_request(model);
        if rewritten != model {
            body["model"] = Value::String(rewritten);
        }
    }
    let attempt = match send_upstream(&state, Endpoint::CountTokens, &headers, body).await {
        Ok(a) => a,
        Err(e) => return e.into_response(),
    };
    passthrough_verbatim(attempt.response).await
}

async fn handle_responses(
    State(state): State<Arc<AppState>>, headers: HeaderMap, Json(body): Json<Value>,
) -> Response<Body> {
    state.health.increment_connections();
    let _guard = ConnectionGuard(&state.health);
    debug!("Processing /v1/responses request");
    let mut body = body;
    inject_max_price(&mut body, state.max_price_per_request);
    if state.forward_responses_natively() {
        return handle_native_responses(&state, &headers, body).await;
    }
    handle_bridged_responses(&state, &headers, body).await
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
    if state.responses_mode == ResponsesMode::Auto && !upstream_response.status().is_success() {
        let status = upstream_response.status();
        let resp_headers = upstream_response.headers().clone();
        let resp_body = upstream_response.bytes().await.unwrap_or_default();
        if is_missing_responses_error(status, &resp_body) {
            info!("Upstream rejected /v1/responses with {}; falling back to bridge", status);
            state.remember_responses_support(false);
            return handle_bridged_responses(state, headers, body).await;
        }
        state.remember_responses_support(true);
        return response_with_body(status, &resp_headers, resp_body);
    }
    state.remember_responses_support(true);
    if !upstream_response.status().is_success() {
        return passthrough_verbatim(upstream_response).await;
    }
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
    let client_model = chat_body.get("model").and_then(|m| m.as_str()).unwrap_or_default().to_string();
    if !client_model.is_empty() {
        let rewritten = state.rewriter.rewrite_request(&client_model);
        if rewritten != client_model {
            debug!("Rewrote model {} -> {}", client_model, rewritten);
            chat_body["model"] = Value::String(rewritten);
        }
    }
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
    if is_stream { handle_responses_stream(upstream_response, client_model).await }
    else { handle_responses_nonstream(upstream_response, &state.rewriter, &tools).await }
}

const POOL_REJECTION_MARKERS: [&str; 6] = [
    "no safe maximum price", "per-request maximum price",
    "continuation or media usage",
    "invalid url", "unsupported endpoint", "unknown endpoint",
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
    if let Some(model) = body.get("model").and_then(|m| m.as_str()) {
        let rewritten = state.rewriter.rewrite_request(model);
        if rewritten != model {
            debug!("Rewrote model {} -> {}", model, rewritten);
            body["model"] = Value::String(rewritten);
        }
    }
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
    if is_stream { passthrough_stream(upstream_response, &state.rewriter).await }
    else { passthrough_nonstream(upstream_response, &state.rewriter).await }
}

async fn handle_responses_nonstream(upstream_response: reqwest::Response, rewriter: &ModelRewriter, tools: &ToolMap) -> Response<Body> {
    let chat_response: Value = match upstream_response.json().await {
        Ok(v) => v,
        Err(e) => { error!("Failed to parse upstream JSON: {}", e); return json_error(StatusCode::BAD_GATEWAY, "Invalid upstream response"); }
    };
    let mut resp = match chat_to_response(&chat_response, tools) {
        Ok(r) => r,
        Err(e) => { error!("Failed to convert response: {}", e); return json_error(StatusCode::BAD_GATEWAY, e.to_string()); }
    };
    if let Some(model) = resp.get("model").and_then(|m| m.as_str()) {
        let original = rewriter.rewrite_response(model);
        if original != model { resp["model"] = Value::String(original); }
    }
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

async fn passthrough_nonstream(upstream_response: reqwest::Response, rewriter: &ModelRewriter) -> Response<Body> {
    let mut json: Value = match upstream_response.json().await {
        Ok(v) => v,
        Err(e) => { error!("Failed to parse upstream JSON: {}", e); return json_error(StatusCode::BAD_GATEWAY, "Invalid upstream response"); }
    };
    if let Some(model) = json.get("model").and_then(|m| m.as_str()) {
        let original = rewriter.rewrite_response(model);
        if original != model { json["model"] = Value::String(original); }
    }
    Response::builder().status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json.to_string())).unwrap()
}

async fn passthrough_stream(upstream_response: reqwest::Response, rewriter: &ModelRewriter) -> Response<Body> {
    let rewriter = rewriter.clone();
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
                    if let Some(model) = json.get("model").and_then(|m| m.as_str()) {
                        let original = rewriter.rewrite_response(model);
                        if original != model { json["model"] = Value::String(original); }
                    }
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
        }
    }

    #[test]
    fn native_mode_always_passes_through() {
        let s = state(ResponsesMode::Native);
        assert!(s.forward_responses_natively());
        s.remember_responses_support(false);
        assert!(s.forward_responses_natively());
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
}
