//! Building the responses the gateway itself returns.
//!
//! Everything here produces a `Response<Body>` without contacting an upstream:
//! verbatim relays of an upstream reply, JSON errors, and the SSE-shaped errors a
//! streaming client needs. Split out of `router.rs`, which had grown to hold the
//! routing table, every handler, protocol bridging, and this.

use axum::{
    body::Body,
    http::{header, HeaderMap, Response, StatusCode},
};
use bytes::Bytes;
use serde_json::Value;
use tracing::warn;
use uuid::Uuid;

use super::passthrough_verbatim;

pub(super) fn response_with_body(
    status: reqwest::StatusCode,
    headers: &HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let status = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut builder = Response::builder().status(status);
    if let Some(ct) = headers.get(header::CONTENT_TYPE) {
        builder = builder.header(header::CONTENT_TYPE, ct);
    }
    builder.body(Body::from(body)).unwrap()
}

pub(super) fn json_error(status: StatusCode, message: impl Into<String>) -> Response<Body> {
    let body = serde_json::json!({
        "error": { "message": message.into(), "type": "gateway_error", "code": status.as_u16() }
    });
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Report a failure to a client that asked for `stream: true`.
///
/// Three shapes were tried against Codex, in this order:
///
/// 1. `application/json` with the error status — what the gateway did originally.
///    A streaming client is reading an SSE body, so it never saw a terminal event
///    and the session just stopped: the transcript ended after the tool output
///    with no assistant message and no error.
/// 2. `event: error` plus a terminal `event: response.failed`. Worse. Codex
///    treats `response.failed` as a *retryable* stream failure, not an end, and
///    reports `stream disconnected before completion: response.failed event
///    received`. Measured: it retried once a minute for 18 minutes, then gave up
///    with no message at all.
/// 3. This one. `response.completed` is the only event Codex accepts as a clean
///    end of turn, so the failure is delivered as a completed turn whose assistant
///    text *is* the error. The turn is still failed — nothing here pretends the
///    request succeeded — but the reason lands in front of the user in one turn
///    instead of after an 18-minute retry loop, and `error` is still emitted
///    first for clients that read it.
pub(super) fn sse_error(
    status: StatusCode,
    message: impl Into<String>,
    upstream_body: Option<&str>,
) -> Response<Body> {
    let message = message.into();
    let mut detail = serde_json::json!({
        "message": message,
        "type": "gateway_error",
        "code": status.as_u16(),
    });
    // Keep the upstream's own error verbatim when there is one, so the cause is
    // not lost in translation.
    if let Some(raw) = upstream_body {
        let parsed: Option<Value> = serde_json::from_str(raw).ok();
        detail["upstream"] =
            parsed.unwrap_or_else(|| Value::String(raw.chars().take(2000).collect()));
    }

    // Surface the upstream's own message when it has one; a bare "Upstream
    // returned 503" tells the user nothing actionable.
    let upstream_note = detail
        .get("upstream")
        .and_then(|u| u.pointer("/error/message").and_then(Value::as_str))
        .map(|m| format!("\n\n上游返回：{m}"))
        .unwrap_or_default();
    let visible = format!("⚠ 网关无法完成这一轮请求。\n\n{message}{upstream_note}");

    let response_id = format!("resp_ad_{}", Uuid::new_v4().simple());
    let item_id = format!("msg_ad_{}", Uuid::new_v4().simple());
    let message_item = serde_json::json!({
        "id": item_id,
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": [{ "type": "output_text", "annotations": [], "logprobs": [], "text": visible }]
    });
    let completed = serde_json::json!({
        "type": "response.completed",
        "response": {
            "id": response_id,
            "object": "response",
            "status": "completed",
            "output": [message_item],
            "parallel_tool_calls": true,
            "tools": [],
            // The failure is recorded here as well, so a client that inspects the
            // response rather than the text can still tell this turn failed.
            "gateway_error": detail.clone(),
        }
    });
    let body = format!(
        "event: error\ndata: {}\n\nevent: response.completed\ndata: {}\n\n",
        serde_json::json!({ "type": "error", "error": detail }),
        completed
    );
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(body))
        .unwrap()
}

/// Forward an upstream failure, framed as SSE when the client is streaming.
///
/// Same behaviour as [`passthrough_error`], but takes an already-read body so the
/// caller can log the upstream's error text before it is turned into a response.
pub(super) fn error_from_body(
    status: reqwest::StatusCode,
    raw: String,
    is_stream: bool,
) -> Response<Body> {
    let status = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if !is_stream {
        return Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(raw))
            .unwrap();
    }
    warn!(
        "Upstream returned {} on a streaming request; reporting it as SSE so the client sees a terminal event",
        status
    );
    sse_error(status, format!("Upstream returned {status}"), Some(&raw))
}

pub(super) async fn passthrough_error(
    upstream_response: reqwest::Response,
    is_stream: bool,
) -> Response<Body> {
    if !is_stream {
        return passthrough_verbatim(upstream_response).await;
    }
    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let raw = upstream_response.text().await.unwrap_or_default();
    warn!(
        "Upstream returned {} on a streaming request; reporting it as SSE so the client sees a terminal event",
        status
    );
    sse_error(status, format!("Upstream returned {status}"), Some(&raw))
}
