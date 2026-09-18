//! End-to-end checks for the two fixes this workspace batch ships, over the real
//! HTTP surface a Claude Code client hits: `GET /v1/models` and `POST /v1/messages`.
//!
//! The router-level unit tests (`unprobed_upstream_strips_client_thinking`,
//! `model_discovery_reflects_measured_thinking_support_and_output_limit`) pin the
//! functions; these tests pin the wiring — that a route's `thinking_support` and
//! `claude_max_output_tokens` actually reach the handlers that serve the client.
//!
//! The stub upstream records what arrived, so assertions read the upstream side,
//! not just the client-visible status code.

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use polydeck_core::types::ThinkingSupport;
use polydeck_gateway::{
    config::{GatewayConfig, ResponsesMode, RouteConfig, UpstreamConfig},
    server::GatewayServer,
};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const TOKEN: &str = "adk_e2e_thinking";

/// Everything the stub upstream saw, so tests assert on forwarded requests.
#[derive(Default)]
struct Seen {
    models_requests: usize,
    message_bodies: Vec<Value>,
}

type SeenLog = Arc<Mutex<Seen>>;

/// A stub relay that serves one model and records what the gateway forwarded.
async fn stub_upstream() -> (String, SeenLog) {
    let seen: SeenLog = Arc::new(Mutex::new(Seen::default()));

    async fn record_models(seen: State<SeenLog>) -> Json<Value> {
        seen.lock().unwrap().models_requests += 1;
        Json(json!({
            "object": "list",
            "data": [{ "id": "glm-5.3-flash", "object": "model" }]
        }))
    }

    async fn record_message(State(seen): State<SeenLog>, Json(body): Json<Value>) -> Json<Value> {
        seen.lock().unwrap().message_bodies.push(body.clone());
        Json(json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": body.get("model").cloned().unwrap_or_default(),
            "content": [{ "type": "text", "text": "ok" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        }))
    }

    let app = Router::new()
        .route("/v1/models", get(record_models))
        .route("/v1/messages", post(record_message))
        .with_state(Arc::clone(&seen));

    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), seen)
}

fn route(base_url: &str, thinking_support: ThinkingSupport) -> RouteConfig {
    RouteConfig {
        client_id: "claude-code".into(),
        upstream: UpstreamConfig {
            provider_id: Some("stub-relay".into()),
            base_url: base_url.to_string(),
            api_key: "stub-key".into(),
            protocol: "openai".into(),
            local_token: TOKEN.into(),
            max_price_per_request: None,
            responses_mode: ResponsesMode::Auto,
            relay_chat_compat: Default::default(),
            accept_invalid_certs: false,
            rate_limit: Default::default(),
            default_effort_level: None,
            thinking_support,
            // The value the user configured for the reported issue.
            claude_max_output_tokens: Some(262_144),
        },
        model_rewrites: vec![],
    }
}

async fn gateway_with(route: RouteConfig) -> (GatewayServer, String) {
    let mut config = GatewayConfig::single(
        UpstreamConfig {
            provider_id: None,
            base_url: String::new(),
            api_key: String::new(),
            protocol: String::new(),
            local_token: String::new(),
            max_price_per_request: None,
            responses_mode: ResponsesMode::Auto,
            relay_chat_compat: Default::default(),
            accept_invalid_certs: false,
            rate_limit: Default::default(),
            default_effort_level: None,
            thinking_support: ThinkingSupport::default(),
            claude_max_output_tokens: None,
        },
        vec![],
    );
    config.listen_addr = Some(SocketAddr::from(([127, 0, 0, 1], 0)));
    config.timeout = Duration::from_secs(30);
    config.max_retries = 0;
    config.routes = vec![route];

    let mut server = GatewayServer::new(config);
    let addr = server.start().await.expect("gateway did not start");
    (server, format!("http://{addr}"))
}

async fn fetch_models(base: &str) -> Value {
    reqwest::Client::new()
        .get(format!("{base}/v1/models"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .expect("gateway did not answer /v1/models")
        .json()
        .await
        .expect("/v1/models returned non-JSON")
}

/// What Claude Code sends after it read a `/models` reply that lied about
/// thinking: a client-generated thinking block on a plain message request.
async fn post_message_with_client_thinking(base: &str) {
    let status = reqwest::Client::new()
        .post(format!("{base}/v1/messages"))
        .bearer_auth(TOKEN)
        .json(&json!({
            "model": "glm-5.3-flash",
            "max_tokens": 8192,
            "thinking": { "type": "enabled", "budget_tokens": 8192 },
            "messages": [{ "role": "user", "content": "hi" }]
        }))
        .send()
        .await
        .expect("gateway did not answer /v1/messages")
        .status();
    assert_eq!(status, 200, "message request should succeed end to end");
}

/// An unprobed upstream must be advertised as not supporting thinking, and the
/// configured output ceiling must reach the model list. This is the pair that let
/// Claude Code turn effort=medium into budget_tokens=8192.
#[tokio::test]
async fn unprobed_upstream_advertises_no_thinking_and_configured_output_ceiling() {
    let (upstream_url, _seen) = stub_upstream().await;
    let (mut server, base) = gateway_with(route(&upstream_url, ThinkingSupport::Unprobed)).await;

    let models = fetch_models(&base).await;
    let model = &models["data"][0];
    assert_eq!(
        model["capabilities"]["thinking"]["supported"],
        json!(false),
        "unprobed 上游的 /models 不得谎报 thinking 支持，否则客户端自开 thinking"
    );
    assert_eq!(
        model["max_tokens"],
        json!(262_144),
        "/models 的 max_tokens 必须反映配置的输出上限，而不是旧版的硬编码 32000"
    );

    server.stop().await;
}

/// A signed upstream keeps advertising thinking so real thinking models still get
/// the feature — the fix narrows the lie, it does not disable the capability.
#[tokio::test]
async fn signed_upstream_still_advertises_thinking() {
    let (upstream_url, _seen) = stub_upstream().await;
    let (mut server, base) = gateway_with(route(&upstream_url, ThinkingSupport::Signed)).await;

    let models = fetch_models(&base).await;
    assert_eq!(
        models["data"][0]["capabilities"]["thinking"]["supported"],
        json!(true),
        "signed 上游应继续如实报告 thinking 支持"
    );

    server.stop().await;
}

/// The gateway must strip a client thinking block before an upstream that cannot
/// honour it sees the request — the same 8192 budget from the bug report, but now
/// observed at the wire.
#[tokio::test]
async fn client_thinking_is_stripped_before_an_unprobed_upstream() {
    let (upstream_url, seen) = stub_upstream().await;
    let (mut server, base) = gateway_with(route(&upstream_url, ThinkingSupport::Unprobed)).await;

    post_message_with_client_thinking(&base).await;

    let bodies = seen.lock().unwrap().message_bodies.clone();
    assert_eq!(bodies.len(), 1, "上游应恰好收到一次转发");
    assert!(
        bodies[0].get("thinking").is_none(),
        "上游不可注入时，客户端带来的 thinking 必须在转发前被剥离，实际收到：{bodies:#?}"
    );
    // Everything else about the request survives the strip.
    assert_eq!(bodies[0]["model"], "glm-5.3-flash");
    assert_eq!(bodies[0]["max_tokens"], 8192);

    server.stop().await;
}

/// On a signed upstream the client's own thinking passes through untouched — the
/// strip only guards against upstreams that would reject unsigned blocks.
#[tokio::test]
async fn client_thinking_passes_through_to_a_signed_upstream() {
    let (upstream_url, seen) = stub_upstream().await;
    let (mut server, base) = gateway_with(route(&upstream_url, ThinkingSupport::Signed)).await;

    post_message_with_client_thinking(&base).await;

    let bodies = seen.lock().unwrap().message_bodies.clone();
    assert_eq!(bodies.len(), 1);
    assert_eq!(
        bodies[0]["thinking"]["budget_tokens"], 8192,
        "signed 上游不应动客户端的 thinking"
    );

    server.stop().await;
}
