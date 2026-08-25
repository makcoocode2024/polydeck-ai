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

/// The sentinel `local_token` meaning "local desktop mode, no token required".
///
/// `build_gateway_config` writes this, and `profile_switch` writes the same
/// string into each client's config as its bearer token, so this is the value
/// seen in every normal run.
pub const LOCAL_DESKTOP_TOKEN: &str = "ai-deck-local";

/// Compare two secrets without leaking their common prefix length via timing.
///
/// Hand-rolled to avoid a dependency for eight lines. The length check leaks
/// only the length, which is not secret.
fn secret_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Gate the API routes.
///
/// Two modes, and the distinction is the whole point:
///
/// - **Local desktop mode** (`local_token` empty or [`LOCAL_DESKTOP_TOKEN`]):
///   no token is required. The security boundary is
///   [`loopback_only_middleware`] plus the loopback-only bind in
///   `GatewayServer::start`, per the project rule that the gateway never
///   listens off-loopback. This is the configuration every normal run uses.
///
/// - **Explicit token mode** (any other `local_token`): the token is enforced.
///   Previously a catch-all arm accepted *any* non-empty token here, so a
///   deliberately configured token was unenforceable while still appearing to
///   be a control. That is worse than no check, because it reads as one.
pub async fn auth_middleware(
    State(state): State<Arc<MiddlewareState>>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    if state.local_token.is_empty() || state.local_token == LOCAL_DESKTOP_TOKEN {
        return next.run(request).await;
    }

    // The upstream key is accepted too: a client configured to talk straight to
    // the provider keeps working when the gateway is put in front of it.
    let accepted = extract_auth_token(&headers).is_some_and(|t| {
        secret_eq(t, &state.local_token)
            || (!state.upstream_api_key.is_empty() && secret_eq(t, &state.upstream_api_key))
    });

    if accepted {
        next.run(request).await
    } else {
        json_error(
            StatusCode::UNAUTHORIZED,
            "Missing or invalid Authorization / x-api-key header",
        )
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

/// Not mounted by [`crate::router::build_router`], deliberately.
///
/// Nothing in the app needs a browser to read gateway responses: the clients are
/// CLIs and the Tauri webview talks over IPC. Leaving CORS off means a web page
/// cannot preflight `POST /v1/chat/completions` (no `OPTIONS` route answers, so
/// the browser blocks the request), which is a useful barrier against a page
/// spending the user's upstream key. Mounting this would remove that barrier, so
/// it should only be wired up alongside a real token requirement.
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
    use axum::{body::Body, routing::get, Router};
    use tower::ServiceExt;

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

    /// Drive the real middleware, since the bug being guarded against lived in
    /// its match arms rather than in token extraction.
    async fn probe(state: MiddlewareState, auth: Option<&str>) -> StatusCode {
        let app = Router::new()
            .route("/v1/models", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                Arc::new(state),
                auth_middleware,
            ));
        let mut req = Request::builder().uri("/v1/models");
        if let Some(a) = auth {
            req = req.header(header::AUTHORIZATION, format!("Bearer {a}"));
        }
        app.oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    fn state(local: &str, upstream: &str) -> MiddlewareState {
        MiddlewareState {
            local_token: local.into(),
            upstream_api_key: upstream.into(),
        }
    }

    #[tokio::test]
    async fn local_desktop_mode_needs_no_token() {
        for local in ["", LOCAL_DESKTOP_TOKEN] {
            assert_eq!(
                probe(state(local, "sk-upstream"), None).await,
                StatusCode::OK,
                "local_token={local:?} must stay open; loopback is the boundary"
            );
        }
    }

    #[tokio::test]
    async fn explicit_token_is_accepted() {
        assert_eq!(
            probe(state("secret-token", ""), Some("secret-token")).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn upstream_key_is_accepted_when_token_configured() {
        assert_eq!(
            probe(state("secret-token", "sk-upstream"), Some("sk-upstream")).await,
            StatusCode::OK
        );
    }

    /// The regression. A catch-all arm used to accept any non-empty token, so a
    /// configured token was decorative.
    #[tokio::test]
    async fn explicit_token_rejects_wrong_token() {
        assert_eq!(
            probe(state("secret-token", "sk-upstream"), Some("wrong-token")).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn explicit_token_rejects_missing_token() {
        assert_eq!(
            probe(state("secret-token", ""), None).await,
            StatusCode::UNAUTHORIZED
        );
    }

    /// An empty upstream key must not turn into a wildcard via `Some("")`.
    #[tokio::test]
    async fn empty_upstream_key_is_not_a_wildcard() {
        assert_eq!(
            probe(state("secret-token", ""), Some("")).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn secret_eq_matches_only_identical_strings() {
        assert!(secret_eq("abc", "abc"));
        assert!(!secret_eq("abc", "abd"));
        assert!(!secret_eq("abc", "ab"));
        assert!(!secret_eq("", "a"));
        assert!(secret_eq("", ""));
    }
}
