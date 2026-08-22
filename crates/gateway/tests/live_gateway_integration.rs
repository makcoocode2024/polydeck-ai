use polydeck_gateway::{
    config::{GatewayConfig, ModelRewriteRule, ResponsesMode, UpstreamConfig},
    server::GatewayServer,
};
use reqwest::Client;
use serde_json::json;
use std::net::SocketAddr;
use std::time::Duration;

#[tokio::test]
#[ignore = "Live network test against subtoken.vip"]
async fn test_full_gateway_and_clients_flow() {
    let base_url = "https://subtoken.vip";
    let api_key = "sk-d40a498260c10e1c6a1017aa0c027bd296b8c4189fb6d3ad3b89ecad6e68bf9d";

    let mut rules = Vec::new();
    rules.push(ModelRewriteRule::exact(
        "claude-3-7-sonnet-20250219",
        "subtoken-sonnet-4-6",
    ));
    rules.push(ModelRewriteRule::exact(
        "claude-3-5-sonnet-20241022",
        "subtoken-sonnet-4-6",
    ));
    rules.push(ModelRewriteRule::exact("gpt-4o", "gemini-3.7-flash-high"));

    let config = GatewayConfig {
        listen_addr: Some(SocketAddr::from(([127, 0, 0, 1], 18889))),
        timeout: Duration::from_secs(60),
        max_retries: 2,
        upstream: UpstreamConfig {
            provider_id: Some("subtoken-test".into()),
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            protocol: "openai".to_string(),
            local_token: "ai-deck-local".into(),
            responses_mode: ResponsesMode::Auto,
            max_price_per_request: Some(5.0),
            rate_limit: polydeck_core::profile::RateLimitSettings::default(),
            default_effort_level: None,
        },
        model_rewrites: rules,
    };

    let mut server = GatewayServer::new(config);
    let addr = server
        .start()
        .await
        .expect("Failed to start gateway server");
    println!("Gateway server running on {}", addr);

    let client = Client::new();
    let gw_base = format!("http://{}", addr);

    // 1. Test GET /v1/models with upstream bearer token
    println!("=== 1. Test GET /v1/models (Auth: Bearer upstream_key) ===");
    let resp = client
        .get(format!("{}/v1/models", gw_base))
        .bearer_auth(api_key)
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let models_body = resp.text().await.unwrap();
    println!("Status: {}, Body: {}", status, models_body);
    assert_eq!(status, 200);
    assert!(models_body.contains("gemini-3.7-flash-high") || models_body.contains("gpt-4o"));
    assert!(
        models_body.contains("subtoken-sonnet-4-6")
            || models_body.contains("claude-3-5-sonnet-20241022")
    );

    // 2. Test GET /models with local token
    println!("=== 2. Test GET /models (Auth: Bearer ai-deck-local) ===");
    let resp2 = client
        .get(format!("{}/models", gw_base))
        .bearer_auth("ai-deck-local")
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), 200);

    // 3. Test POST /v1/chat/completions (OpenAI client)
    println!("=== 3. Test POST /v1/chat/completions (OpenAI protocol) ===");
    let chat_req = json!({
        "model": "gemini-3.7-flash-high",
        "messages": [
            {"role": "user", "content": "Say hello in one word"}
        ],
        "temperature": 0.3
    });
    let chat_resp = client
        .post(format!("{}/v1/chat/completions", gw_base))
        .bearer_auth("ai-deck-local")
        .json(&chat_req)
        .send()
        .await
        .unwrap();
    assert_eq!(chat_resp.status(), 200);
    let chat_body: serde_json::Value = chat_resp.json().await.unwrap();
    println!("Chat completion response: {}", chat_body);
    assert!(chat_body.get("choices").is_some());

    // 4. Test POST /v1/messages (Claude Code protocol)
    println!("=== 4. Test POST /v1/messages (Claude protocol) ===");
    let claude_req = json!({
        "model": "claude-3-5-sonnet-20241022",
        "max_tokens": 100,
        "messages": [
            {"role": "user", "content": "Reply with 'Claude Test OK'"}
        ]
    });
    let claude_resp = client
        .post(format!("{}/v1/messages", gw_base))
        .header("x-api-key", "ai-deck-local")
        .header("anthropic-version", "2023-06-01")
        .json(&claude_req)
        .send()
        .await
        .unwrap();
    assert_eq!(claude_resp.status(), 200);
    let claude_body: serde_json::Value = claude_resp.json().await.unwrap();
    println!("Claude message response: {}", claude_body);
    assert!(claude_body.get("content").is_some());

    // 5. Test POST /v1/responses (Codex protocol via Auto bridge)
    println!("=== 5. Test POST /v1/responses (Codex protocol) ===");
    let codex_req = json!({
        "model": "gemini-3.7-flash-high",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "Reply with 'Codex Test OK'"}
                ]
            }
        ]
    });
    let codex_resp = client
        .post(format!("{}/v1/responses", gw_base))
        .bearer_auth("ai-deck-local")
        .json(&codex_req)
        .send()
        .await
        .unwrap();
    assert_eq!(codex_resp.status(), 200);
    let codex_body: serde_json::Value = codex_resp.json().await.unwrap();
    println!("Codex responses result: {}", codex_body);
    assert!(codex_body.get("output").is_some() || codex_body.get("status").is_some());

    println!("All live gateway tests passed successfully!");
}
