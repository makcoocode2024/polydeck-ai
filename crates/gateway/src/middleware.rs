//! Middleware for authentication, logging, and security

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info};

#[derive(Clone, Default)]
pub struct MiddlewareState {
    pub local_token: String,
    pub upstream_api_key: String,
}

pub async fn auth_middleware(
    State(state): State<Arc<MiddlewareState>>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    // If local_token is empty or the default local desktop identifier "ai-deck-local",
    // allow all loopback-verified local requests.
    if state.local_token.is_empty() || state.local_token == "ai-deck-local" {
        return next.run(request).await;
    }

    let token = extract_auth_token(&headers);
    match token {
        Some(t)
            if t == state.local_token
                || (!state.upstream_api_key.is_empty() && t == state.upstream_api_key) =>
        {
            next.run(request).await
        }
        // In local desktop mode, allow requests that supply any non-empty auth key
        Some(_) => next.run(request).await,
        None => json_error(
            StatusCode::UNAUTHORIZED,
            "Missing or invalid Authorization / x-api-key header",
        ),
    }
}

pub fn extract_auth_token(headers: &HeaderMap) -> Option<&str> {
    if let Some(auth) = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    {
        if let Some(bearer) = auth.strip_prefix("Bearer ") {
            return Some(bearer.trim());
        }
        return Some(auth.trim());
    }
    if let Some(x_key) = headers.get("x-api-key").and_then(|h| h.to_str().ok()) {
        return Some(x_key.trim());
    }
    None
}

pub async fn logging_middleware(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let start = Instant::now();
    debug!("-> {} {}", method, uri);
    let response = next.run(request).await;
    let latency = start.elapsed();
    let status = response.status();
    info!("<- {} {} {} {:?}", method, uri, status, latency);
    response
}

pub async fn cors_middleware(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        "http://127.0.0.1".parse().unwrap(),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        "GET, POST, OPTIONS".parse().unwrap(),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        "Content-Type, Authorization, x-api-key, anthropic-version"
            .parse()
            .unwrap(),
    );
    response
}

pub async fn loopback_only_middleware(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    if !peer.ip().is_loopback() {
        return json_error(StatusCode::FORBIDDEN, "Loopback peer required");
    }
    next.run(request).await
}

fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
    let body = json!({
        "error": { "message": message.into(), "type": "gateway_error", "code": status.as_u16() }
    });
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        Json(body),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer test-token-123".parse().unwrap(),
        );
        assert_eq!(extract_auth_token(&headers), Some("test-token-123"));
    }

    #[test]
    fn extracts_x_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", "test-key-456".parse().unwrap());
        assert_eq!(extract_auth_token(&headers), Some("test-key-456"));
    }

    #[test]
    fn handles_missing_auth_header() {
        let headers = HeaderMap::new();
        assert_eq!(extract_auth_token(&headers), None);
    }
}
