//! One listener, two profiles, told apart by the caller's token.
//!
//! The property under test is not "requests reach an upstream" but "each request
//! reaches *its own* upstream with *its own* profile settings". A gateway that
//! routed only the base URL while sharing the model rewriter, Responses mode, or
//! thinking settings would pass a naive check and still serve the wrong thing, so
//! the assertions look at what arrived upstream, not just where.

use polydeck_gateway::{
    config::{GatewayConfig, ModelRewriteRule, ResponsesMode, RouteConfig, UpstreamConfig},
    server::GatewayServer,
};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const TOKEN_A: &str = "adk_client_a";
const TOKEN_B: &str = "adk_client_b";

/// What a stub upstream saw, so a test can assert on the request rather than only
/// the reply.
#[derive(Default)]
struct Seen {
    paths: Vec<String>,
    bodies: Vec<serde_json::Value>,
}

type SeenLog = Arc<Mutex<Seen>>;

/// A stub upstream that records every request and answers plausibly for whichever
/// endpoint was hit.
async fn stub_upstream(name: &'static str) -> (String, SeenLog) {
    use axum::{
        extract::State,
        routing::{get, post},
        Json, Router,
    };

    let seen: SeenLog = Arc::new(Mutex::new(Seen::default()));

    async fn record(seen: &SeenLog, path: &str, body: Option<&serde_json::Value>) {
        let mut guard = seen.lock().unwrap();
        guard.paths.push(path.to_string());
        if let Some(body) = body {
            guard.bodies.push(body.clone());
        }
    }

    let app = Router::new()
        .route(
            "/v1/models",
            get(
                |State((seen, name)): State<(SeenLog, &'static str)>| async move {
                    record(&seen, "/v1/models", None).await;
                    Json(serde_json::json!({
                        "object": "list",
                        "data": [{ "id": format!("{name}-model"), "object": "model" }]
                    }))
                },
            ),
        )
        .route(
            "/v1/messages",
            post(
                |State((seen, name)): State<(SeenLog, &'static str)>,
                 Json(body): Json<serde_json::Value>| async move {
                    record(&seen, "/v1/messages", Some(&body)).await;
                    Json(serde_json::json!({
                        "id": "msg_1",
                        "type": "message",
                        "role": "assistant",
                        "model": body.get("model").cloned().unwrap_or_default(),
                        "content": [{ "type": "text", "text": name }],
                        "stop_reason": "end_turn",
                        "usage": { "input_tokens": 1, "output_tokens": 1 }
                    }))
                },
            ),
        )
        .route(
            "/v1/responses",
            post(
                |State((seen, _)): State<(SeenLog, &'static str)>,
                 Json(body): Json<serde_json::Value>| async move {
                    record(&seen, "/v1/responses", Some(&body)).await;
                    Json(serde_json::json!({
                        "id": "resp_1",
                        "object": "response",
                        "status": "completed",
                        "output": []
                    }))
                },
            ),
        )
        .route(
            "/v1/chat/completions",
            post(
                |State((seen, _)): State<(SeenLog, &'static str)>,
                 Json(body): Json<serde_json::Value>| async move {
                    record(&seen, "/v1/chat/completions", Some(&body)).await;
                    Json(serde_json::json!({
                        "id": "chat_1",
                        "object": "chat.completion",
                        "choices": [{
                            "index": 0,
                            "message": { "role": "assistant", "content": "ok" },
                            "finish_reason": "stop"
                        }],
                        "usage": { "prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2 }
                    }))
                },
            ),
        )
        .with_state((Arc::clone(&seen), name));

    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), seen)
}

fn upstream(base_url: &str, token: &str, mode: ResponsesMode) -> UpstreamConfig {
    UpstreamConfig {
        provider_id: Some(format!("prov-{token}")),
        base_url: base_url.to_string(),
        api_key: "stub-key".into(),
        protocol: "openai".into(),
        local_token: token.into(),
        max_price_per_request: None,
        responses_mode: mode,
        rate_limit: Default::default(),
        default_effort_level: None,
        thinking_support: polydeck_core::types::ThinkingSupport::Unprobed,
    }
}

fn route(
    client_id: &str,
    base_url: &str,
    token: &str,
    mode: ResponsesMode,
    rewrites: Vec<ModelRewriteRule>,
) -> RouteConfig {
    RouteConfig {
        client_id: client_id.into(),
        upstream: upstream(base_url, token, mode),
        model_rewrites: rewrites,
    }
}

async fn gateway_with(routes: Vec<RouteConfig>) -> (GatewayServer, String) {
    let mut config = GatewayConfig::single(
        upstream("http://127.0.0.1:1", "unused", ResponsesMode::Auto),
        vec![],
    );
    config.listen_addr = Some(SocketAddr::from(([127, 0, 0, 1], 0)));
    config.timeout = Duration::from_secs(30);
    config.max_retries = 0;
    config.routes = routes;

    let mut server = GatewayServer::new(config);
    let addr = server.start().await.expect("gateway did not start");
    (server, format!("http://{addr}"))
}

async fn post_message(base: &str, token: &str, model: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{base}/v1/messages"))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "model": model,
            "max_tokens": 16,
            "messages": [{ "role": "user", "content": "hi" }]
        }))
        .send()
        .await
        .expect("gateway did not answer")
}

/// Two clients, two upstreams, one port. Each request must land on its own stub and
/// nowhere else.
#[tokio::test]
async fn each_token_reaches_its_own_upstream() {
    let (url_a, seen_a) = stub_upstream("alpha").await;
    let (url_b, seen_b) = stub_upstream("beta").await;
    let (mut server, base) = gateway_with(vec![
        route("codex-cli", &url_a, TOKEN_A, ResponsesMode::Auto, vec![]),
        route("claude-code", &url_b, TOKEN_B, ResponsesMode::Auto, vec![]),
    ])
    .await;

    assert_eq!(post_message(&base, TOKEN_A, "m").await.status(), 200);
    assert_eq!(post_message(&base, TOKEN_B, "m").await.status(), 200);

    assert_eq!(
        seen_a.lock().unwrap().paths,
        vec!["/v1/messages".to_string()],
        "A 的上游只应收到 A 的那一个请求"
    );
    assert_eq!(
        seen_b.lock().unwrap().paths,
        vec!["/v1/messages".to_string()],
        "B 的上游只应收到 B 的那一个请求"
    );

    server.stop().await;
}

/// Model rewriting is per-profile, so it has to follow the token too. A shared
/// rewriter would apply A's mapping to B's request.
#[tokio::test]
async fn model_rewrites_follow_the_token() {
    let (url_a, seen_a) = stub_upstream("alpha").await;
    let (url_b, seen_b) = stub_upstream("beta").await;
    let (mut server, base) = gateway_with(vec![
        route(
            "codex-cli",
            &url_a,
            TOKEN_A,
            ResponsesMode::Auto,
            vec![ModelRewriteRule::exact("claude-opus-5", "alpha-real")],
        ),
        route("claude-code", &url_b, TOKEN_B, ResponsesMode::Auto, vec![]),
    ])
    .await;

    post_message(&base, TOKEN_A, "claude-opus-5").await;
    post_message(&base, TOKEN_B, "claude-opus-5").await;

    assert_eq!(
        seen_a.lock().unwrap().bodies[0]["model"].as_str(),
        Some("alpha-real"),
        "A 的重写规则应生效"
    );
    assert_eq!(
        seen_b.lock().unwrap().bodies[0]["model"].as_str(),
        Some("claude-opus-5"),
        "B 没有这条规则，不该被 A 的规则改写"
    );

    server.stop().await;
}

/// `GET /models` does not go through `send_upstream`, so it is the handler most
/// likely to regress to a single shared upstream.
#[tokio::test]
async fn models_listing_is_routed_too() {
    let (url_a, seen_a) = stub_upstream("alpha").await;
    let (url_b, seen_b) = stub_upstream("beta").await;
    let (mut server, base) = gateway_with(vec![
        route("codex-cli", &url_a, TOKEN_A, ResponsesMode::Auto, vec![]),
        route("claude-code", &url_b, TOKEN_B, ResponsesMode::Auto, vec![]),
    ])
    .await;

    let body = reqwest::Client::new()
        .get(format!("{base}/v1/models"))
        .bearer_auth(TOKEN_B)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(
        seen_b
            .lock()
            .unwrap()
            .paths
            .contains(&"/v1/models".to_string()),
        "B 的 token 应把 /models 打到 B 的上游"
    );
    assert!(
        seen_a.lock().unwrap().paths.is_empty(),
        "A 的上游完全不应被碰到"
    );
    assert!(
        body.contains("beta-model"),
        "返回的目录应来自 B，实际：{body}"
    );

    server.stop().await;
}

/// The Responses mode is per-profile, and it decides which upstream *endpoint* is
/// used. Routing only the base URL would send both clients down the same path.
#[tokio::test]
async fn responses_mode_follows_the_token() {
    let (url_a, seen_a) = stub_upstream("alpha").await;
    let (url_b, seen_b) = stub_upstream("beta").await;
    let (mut server, base) = gateway_with(vec![
        route(
            "codex-native",
            &url_a,
            TOKEN_A,
            ResponsesMode::Native,
            vec![],
        ),
        route(
            "codex-bridge",
            &url_b,
            TOKEN_B,
            ResponsesMode::Bridge,
            vec![],
        ),
    ])
    .await;

    let payload = serde_json::json!({
        "model": "m",
        "input": [{
            "type": "message", "role": "user",
            "content": [{ "type": "input_text", "text": "hi" }]
        }]
    });
    for token in [TOKEN_A, TOKEN_B] {
        reqwest::Client::new()
            .post(format!("{base}/v1/responses"))
            .bearer_auth(token)
            .json(&payload)
            .send()
            .await
            .unwrap();
    }

    assert_eq!(
        seen_a.lock().unwrap().paths,
        vec!["/v1/responses".to_string()],
        "Native 档应直接走 /v1/responses"
    );
    assert_eq!(
        seen_b.lock().unwrap().paths,
        vec!["/v1/chat/completions".to_string()],
        "Bridge 档应桥接到 /v1/chat/completions"
    );

    server.stop().await;
}

/// Rejections must not fall through to some default profile.
#[tokio::test]
async fn unknown_and_missing_tokens_are_refused_and_reach_nothing() {
    let (url_a, seen_a) = stub_upstream("alpha").await;
    let (mut server, base) = gateway_with(vec![route(
        "codex-cli",
        &url_a,
        TOKEN_A,
        ResponsesMode::Auto,
        vec![],
    )])
    .await;

    let client = reqwest::Client::new();
    // The pre-binding sentinel is included deliberately: it used to mean "no token
    // required", and a client still configured with it must be refused rather than
    // silently served.
    for bad in ["adk_unknown", "ai-deck-local", ""] {
        let resp = client
            .post(format!("{base}/v1/messages"))
            .bearer_auth(bad)
            .json(&serde_json::json!({ "model": "m", "messages": [] }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "token {bad:?} 应被拒绝");
    }

    let no_token = client
        .post(format!("{base}/v1/messages"))
        .json(&serde_json::json!({ "model": "m", "messages": [] }))
        .send()
        .await
        .unwrap();
    assert_eq!(no_token.status(), 401, "不带 token 也应被拒绝");

    assert!(
        seen_a.lock().unwrap().paths.is_empty(),
        "被拒的请求一个都不该到上游"
    );

    server.stop().await;
}

/// `/health` is for diagnostics and reveals nothing profile-specific, so it stays
/// reachable without a token.
#[tokio::test]
async fn health_needs_no_token() {
    let (url_a, _seen) = stub_upstream("alpha").await;
    let (mut server, base) = gateway_with(vec![route(
        "codex-cli",
        &url_a,
        TOKEN_A,
        ResponsesMode::Auto,
        vec![],
    )])
    .await;

    let resp = reqwest::Client::new()
        .get(format!("{base}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    server.stop().await;
}

/// Binding a client to another profile must not restart the listener or disturb the
/// clients that did not move.
#[tokio::test]
async fn applying_routes_swaps_without_a_restart() {
    let (url_a, seen_a) = stub_upstream("alpha").await;
    let (url_b, seen_b) = stub_upstream("beta").await;
    let (mut server, base) = gateway_with(vec![
        route("codex-cli", &url_a, TOKEN_A, ResponsesMode::Auto, vec![]),
        route("claude-code", &url_b, TOKEN_B, ResponsesMode::Auto, vec![]),
    ])
    .await;
    let addr_before = server.addr();

    // Codex moves to B's profile and gets a new token; Claude Code is untouched.
    const TOKEN_A2: &str = "adk_client_a_rotated";
    server
        .apply_routes(vec![
            route("codex-cli", &url_b, TOKEN_A2, ResponsesMode::Auto, vec![]),
            route("claude-code", &url_b, TOKEN_B, ResponsesMode::Auto, vec![]),
        ])
        .await
        .expect("route swap failed");

    assert_eq!(server.addr(), addr_before, "监听地址不应变化");
    assert_eq!(server.route_count().await, 2);

    assert_eq!(
        post_message(&base, TOKEN_A, "m").await.status(),
        401,
        "旧 token 应立即失效"
    );
    assert_eq!(post_message(&base, TOKEN_A2, "m").await.status(), 200);
    assert_eq!(
        post_message(&base, TOKEN_B, "m").await.status(),
        200,
        "没被改动的客户端不应受影响"
    );

    assert!(
        seen_a.lock().unwrap().paths.is_empty(),
        "换路由后 A 的上游不该再收到请求"
    );
    assert_eq!(
        seen_b.lock().unwrap().paths.len(),
        2,
        "两个客户端现在都指向 B 的上游"
    );

    server.stop().await;
}

/// A bad route must leave the running table alone rather than half-applying.
#[tokio::test]
async fn a_rejected_swap_leaves_the_old_routes_serving() {
    let (url_a, _seen_a) = stub_upstream("alpha").await;
    let (mut server, base) = gateway_with(vec![route(
        "codex-cli",
        &url_a,
        TOKEN_A,
        ResponsesMode::Auto,
        vec![],
    )])
    .await;

    // A tokenless route cannot be authenticated as, so compilation refuses it.
    let err = server
        .apply_routes(vec![route(
            "codex-cli",
            &url_a,
            "",
            ResponsesMode::Auto,
            vec![],
        )])
        .await
        .expect_err("empty token should have been refused");
    assert!(err.contains("local token"), "实际报错：{err}");

    assert_eq!(
        post_message(&base, TOKEN_A, "m").await.status(),
        200,
        "换路由失败后原来的路由应继续可用"
    );

    server.stop().await;
}
