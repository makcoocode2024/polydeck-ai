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

/// Which runtime each client token selects.
///
/// One entry per bound client. Several clients bound to the same profile share one
/// `Arc<AppState>`, so they also share that profile's learned Auto-mode Responses
/// probe rather than each rediscovering it.
///
/// A `Vec` scanned linearly, not a `HashMap`: hashing a secret to index a table is
/// a timing oracle, `secret_eq` is not, and the entry count is the number of
/// configured clients — single digits.
#[derive(Default)]
pub struct RouteTable {
    entries: Vec<(String, Arc<crate::router::AppState>)>,
}

impl RouteTable {
    pub fn new(entries: Vec<(String, Arc<crate::router::AppState>)>) -> Self {
        Self { entries }
    }

    /// The runtime `token` selects, or `None` when it matches no client.
    pub fn resolve(&self, token: &str) -> Option<Arc<crate::router::AppState>> {
        self.entries
            .iter()
            .find(|(candidate, _)| secret_eq(token, candidate))
            .map(|(_, state)| Arc::clone(state))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A route table that can be swapped while the listener keeps running.
pub type SharedRouteTable = Arc<tokio::sync::RwLock<RouteTable>>;

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

/// Authenticate the caller and choose the runtime its request will use.
///
/// The token does both jobs at once: it says who is calling and, because each
/// bound client has its own, which profile to serve them. A request that matches no
/// client is rejected rather than defaulted, since defaulting would silently answer
/// from a profile nobody chose.
///
/// This replaces a two-mode scheme where the sentinel token `ai-deck-local` meant
/// "no token required" and the upstream provider's own API key was also accepted.
/// Neither survives per-client routing: the first cannot identify a caller at all,
/// and the second cannot pick between two profiles that happen to share an upstream
/// key. Loopback-only remains the outer boundary, but it is no longer the only one.
pub async fn route_auth(
    State(table): State<SharedRouteTable>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Response {
    let selected = match extract_auth_token(&headers) {
        Some(token) => table.read().await.resolve(token),
        None => None,
    };

    match selected {
        Some(state) => {
            // Handlers read this via `Extension` instead of `State`, which is what
            // lets one listener serve several profiles.
            request.extensions_mut().insert(state);
            next.run(request).await
        }
        None => json_error(
            StatusCode::UNAUTHORIZED,
            "No profile is bound to this token. Re-activate this client in PolyDeck.",
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

    /// A runtime distinguishable only by its provider id, which is all these tests
    /// need to tell one route's selection from another's.
    fn runtime(provider_id: &str) -> Arc<crate::router::AppState> {
        Arc::new(crate::router::AppState {
            upstream: crate::client::UpstreamClient::new(
                "http://127.0.0.1:1".into(),
                "k".into(),
                std::time::Duration::from_secs(1),
                0,
            )
            .unwrap(),
            rewriter: crate::model_rewrite::ModelRewriter::new(&[]).unwrap(),
            health: crate::health::HealthState::new(),
            failover: None,
            responses_mode: crate::config::ResponsesMode::Auto,
            responses_native: Arc::new(std::sync::OnceLock::new()),
            max_price_per_request: None,
            rate_limiter_registry: Arc::new(crate::rate_limiter::RateLimiterRegistry::new()),
            primary_provider_id: provider_id.into(),
            rate_limit_settings: Default::default(),
            max_retries: 0,
            default_effort_level: None,
            thinking_support: Default::default(),
        })
    }

    fn table_of(pairs: &[(&str, &str)]) -> SharedRouteTable {
        Arc::new(tokio::sync::RwLock::new(RouteTable::new(
            pairs
                .iter()
                .map(|(token, provider)| ((*token).to_string(), runtime(provider)))
                .collect(),
        )))
    }

    /// Drive the real middleware: the property under test is that a token both
    /// authenticates *and* selects, and only the middleware does both.
    async fn probe(table: SharedRouteTable, auth: Option<&str>) -> StatusCode {
        let app = Router::new()
            .route("/v1/models", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(table, route_auth));
        let mut req = Request::builder().uri("/v1/models");
        if let Some(a) = auth {
            req = req.header(header::AUTHORIZATION, format!("Bearer {a}"));
        }
        app.oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn a_known_token_is_accepted() {
        assert_eq!(
            probe(table_of(&[("adk_a", "prov-a")]), Some("adk_a")).await,
            StatusCode::OK
        );
    }

    /// The old sentinel meant "no token required". Under per-client routing it
    /// cannot identify a caller, so it must not be special any more.
    #[tokio::test]
    async fn the_old_sentinel_no_longer_opens_the_gate() {
        assert_eq!(
            probe(table_of(&[("adk_a", "prov-a")]), Some("ai-deck-local")).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn an_unknown_token_is_refused() {
        assert_eq!(
            probe(table_of(&[("adk_a", "prov-a")]), Some("adk_nope")).await,
            StatusCode::UNAUTHORIZED
        );
    }

    /// No token cannot mean "use the only route": with several routes there is no
    /// only route, and defaulting would answer from a profile nobody chose.
    #[tokio::test]
    async fn a_missing_token_is_refused_even_with_one_route() {
        assert_eq!(
            probe(table_of(&[("adk_a", "prov-a")]), None).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn an_empty_token_is_not_a_wildcard() {
        assert_eq!(
            probe(table_of(&[("adk_a", "prov-a")]), Some("")).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn an_empty_table_refuses_everything() {
        assert_eq!(
            probe(table_of(&[]), Some("adk_a")).await,
            StatusCode::UNAUTHORIZED
        );
    }

    /// The selection itself, which the status code cannot show.
    #[tokio::test]
    async fn each_token_selects_its_own_runtime() {
        let table = table_of(&[("adk_a", "prov-a"), ("adk_b", "prov-b")]);
        let guard = table.read().await;

        assert_eq!(
            guard.resolve("adk_a").unwrap().primary_provider_id,
            "prov-a"
        );
        assert_eq!(
            guard.resolve("adk_b").unwrap().primary_provider_id,
            "prov-b"
        );
        assert!(guard.resolve("adk_c").is_none());
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
