//! Authenticated loopback bridge for fixed injected-script capabilities.

use axum::{
    extract::{ConnectInfo, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use getrandom::fill;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    net::TcpListener,
    sync::{oneshot, Mutex},
    task::JoinHandle,
};

const MAX_REQUEST_BYTES: usize = 32 * 1024;
const MAX_REQUESTS_PER_MINUTE: usize = 60;
const ALLOWED_ORIGIN: &str = "app://openai-codex";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BridgeCommand {
    Hello,
    Status,
    Configure,
    Clear,
    ExportSession,
    DeleteSession,
    GetStepwiseSuggestions,
    OpenInEditor,
    Ping,
    Ack,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BridgeRequest {
    pub id: String,
    pub command: BridgeCommand,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BridgeResponse {
    pub id: String,
    pub ok: bool,
    #[serde(default)]
    pub payload: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl BridgeResponse {
    pub fn ok(id: String, payload: serde_json::Value) -> Self {
        Self {
            id,
            ok: true,
            payload,
            error: None,
        }
    }
    pub fn error(id: String, error: impl Into<String>) -> Self {
        Self {
            id,
            ok: false,
            payload: serde_json::Value::Null,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeStatus {
    pub running: bool,
    pub address: Option<SocketAddr>,
    pub queued_notifications: usize,
}

pub type BridgeFuture = Pin<Box<dyn Future<Output = BridgeResponse> + Send>>;
pub type BridgeHandler = Arc<dyn Fn(BridgeRequest) -> BridgeFuture + Send + Sync>;

struct BridgeState {
    token: String,
    handler: BridgeHandler,
    notifications: Mutex<VecDeque<serde_json::Value>>,
    request_times: Mutex<HashMap<IpAddr, VecDeque<Instant>>>,
}

pub struct BridgeServer {
    address: SocketAddr,
    state: Arc<BridgeState>,
    shutdown: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl BridgeServer {
    pub async fn start(handler: BridgeHandler) -> Result<Self, std::io::Error> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
        let address = listener.local_addr()?;
        let state = Arc::new(BridgeState {
            token: bridge_token(),
            handler,
            notifications: Mutex::new(VecDeque::new()),
            request_times: Mutex::new(HashMap::new()),
        });
        let app = Router::new()
            .route("/bridge", post(handle_request))
            .with_state(state.clone())
            .layer(axum::extract::DefaultBodyLimit::max(MAX_REQUEST_BYTES));
        let (shutdown, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await;
        });
        Ok(Self {
            address,
            state,
            shutdown: Some(shutdown),
            task,
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    #[cfg(test)]
    pub fn test_token(&self) -> &str {
        &self.state.token
    }

    pub(crate) fn bootstrap_source(&self) -> String {
        let url = format!("http://{}", self.address);
        format!(
            "window.__AI_DECK_BRIDGE__={};",
            serde_json::json!({"url": url, "token": self.state.token})
        )
    }

    pub async fn notify(&self, notification: serde_json::Value) {
        let mut queue = self.state.notifications.lock().await;
        if queue.len() >= 100 {
            queue.pop_front();
        }
        queue.push_back(notification);
    }

    pub async fn status(&self) -> BridgeStatus {
        BridgeStatus {
            running: !self.task.is_finished(),
            address: Some(self.address),
            queued_notifications: self.state.notifications.lock().await.len(),
        }
    }

    pub async fn stop(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = self.task.await;
    }
}

async fn handle_request(
    State(state): State<Arc<BridgeState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<BridgeRequest>,
) -> impl IntoResponse {
    if !peer.ip().is_loopback() {
        return (
            StatusCode::FORBIDDEN,
            Json(BridgeResponse::error(request.id, "loopback peer required")),
        );
    }
    if !valid_origin(&headers) {
        return (
            StatusCode::FORBIDDEN,
            Json(BridgeResponse::error(request.id, "origin rejected")),
        );
    }
    if !valid_token(&headers, &state.token) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(BridgeResponse::error(request.id, "authentication required")),
        );
    }
    if !within_rate_limit(&state, peer.ip()).await {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(BridgeResponse::error(request.id, "rate limit exceeded")),
        );
    }
    if request.id.is_empty() || request.id.len() > 128 {
        return (
            StatusCode::BAD_REQUEST,
            Json(BridgeResponse::error(request.id, "invalid request id")),
        );
    }
    if !valid_payload(&request) {
        return (
            StatusCode::BAD_REQUEST,
            Json(BridgeResponse::error(request.id, "invalid command payload")),
        );
    }
    if request.command == BridgeCommand::Ping {
        return (
            StatusCode::OK,
            Json(BridgeResponse::ok(
                request.id,
                serde_json::json!({"pong": true}),
            )),
        );
    }
    if request.command == BridgeCommand::Hello {
        let notifications = state
            .notifications
            .lock()
            .await
            .drain(..)
            .collect::<Vec<_>>();
        return (
            StatusCode::OK,
            Json(BridgeResponse::ok(
                request.id,
                serde_json::json!({"notifications": notifications}),
            )),
        );
    }
    let response = (state.handler)(request).await;
    (StatusCode::OK, Json(response))
}

fn valid_origin(headers: &HeaderMap) -> bool {
    match headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        Some(origin) => origin == ALLOWED_ORIGIN,
        None => true,
    }
}

fn valid_token(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|provided| constant_time_eq(provided.as_bytes(), token.as_bytes()))
        .unwrap_or(false)
}

async fn within_rate_limit(state: &BridgeState, ip: IpAddr) -> bool {
    let now = Instant::now();
    let mut requests = state.request_times.lock().await;
    let times = requests.entry(ip).or_default();
    while times
        .front()
        .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60))
    {
        times.pop_front();
    }
    if times.len() >= MAX_REQUESTS_PER_MINUTE {
        return false;
    }
    times.push_back(now);
    true
}

fn valid_payload(request: &BridgeRequest) -> bool {
    if request.payload.to_string().len() > MAX_REQUEST_BYTES {
        return false;
    }
    match request.command {
        BridgeCommand::Configure => request.payload.is_object(),
        BridgeCommand::ExportSession | BridgeCommand::DeleteSession => request
            .payload
            .get("sessionId")
            .and_then(|v| v.as_str())
            .is_some(),
        BridgeCommand::GetStepwiseSuggestions => request
            .payload
            .get("context")
            .and_then(|v| v.as_str())
            .is_some_and(|v| v.len() <= 16_000),
        BridgeCommand::OpenInEditor => {
            request
                .payload
                .get("editor")
                .and_then(|v| v.as_str())
                .is_some_and(|v| matches!(v, "vscode" | "zed"))
                && request
                    .payload
                    .get("path")
                    .and_then(|v| v.as_str())
                    .is_some()
        }
        _ => request.payload.is_null() || request.payload.is_object(),
    }
}

fn bridge_token() -> String {
    let mut bytes = [0_u8; 32];
    fill(&mut bytes).expect("OS random source unavailable");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter().zip(right).fold(0_u8, |r, (a, b)| r | (a ^ b)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_comparison_checks_bytes() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"different"));
    }
}
