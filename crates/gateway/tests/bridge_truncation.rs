//! Reproduces the sotamodel bridge-path failure without sotamodel.
//!
//! The measured failure needs three things to line up: the request has to reach
//! the bridge rather than native passthrough, the upstream has to die mid-stream,
//! and the client has to be reading SSE. Waiting for the real relay to misbehave
//! gives all three but on its own schedule; a stub gives them on demand.
//!
//! The stub refuses `/v1/responses` with a 404, which is what drives
//! `handle_native_responses` to fall back to the bridge — the same destination
//! Codex reaches via a non-native tool type, without having to reproduce its
//! exact `tools[]` payload.

use polydeck_gateway::{
    config::{GatewayConfig, ResponsesMode, UpstreamConfig},
    server::GatewayServer,
};
use serde_json::json;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// How the stub ends the chat stream once it has sent its events.
#[derive(Clone, Copy)]
enum Ending {
    /// Drop the socket with no terminating chunk — the 2026-08-25 failure shape.
    Sever,
    /// Hold the socket open and send nothing more — the 2026-08-31 shape, caught
    /// by the gateway's own idle timeout.
    GoSilent,
}

/// Upstream that 404s `/v1/responses` and then fails the chat stream mid-body.
///
/// Raw TCP rather than axum: both endings are about what does *not* get written
/// (no terminating chunk, no further data), which a server framework will tidy up
/// on the way out.
async fn stub_upstream(ending: Ending) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                // Read to the end of the headers; the body is irrelevant here but
                // has to be drained or the client may see a reset before it reads
                // the response.
                let mut req = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    match sock.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            req.extend_from_slice(&buf[..n]);
                            if req.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => return,
                    }
                }
                let head = String::from_utf8_lossy(&req);

                if head.starts_with("POST /v1/responses") {
                    let _ = sock
                        .write_all(
                            b"HTTP/1.1 404 Not Found\r\ncontent-type: application/json\r\n\
                              content-length: 32\r\n\r\n{\"error\":{\"message\":\"no such\"}}",
                        )
                        .await;
                    let _ = sock.flush().await;
                    return;
                }

                let _ = sock
                    .write_all(
                        b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\
                          transfer-encoding: chunked\r\n\r\n",
                    )
                    .await;
                for text in ["partial ", "answer"] {
                    let ev = format!(
                        "data: {}\n\n",
                        json!({ "choices": [{ "delta": { "content": text } }] })
                    );
                    let _ = sock
                        .write_all(format!("{:x}\r\n{}\r\n", ev.len(), ev).as_bytes())
                        .await;
                }
                let _ = sock.flush().await;
                match ending {
                    // No terminating `0\r\n\r\n`: the body just stops.
                    Ending::Sever => {}
                    Ending::GoSilent => tokio::time::sleep(Duration::from_secs(120)).await,
                }
            });
        }
    });
    format!("http://{addr}")
}

/// The token this test's single route answers to.
const STUB_TOKEN: &str = "adk_bridge_truncation_stub";

/// A gateway in front of the stub, on an ephemeral port so it cannot collide with
/// the desktop app's 18888.
async fn gateway_in_front_of(upstream: String) -> (GatewayServer, String) {
    let mut config = GatewayConfig::single(
        UpstreamConfig {
            provider_id: Some("stub".into()),
            base_url: upstream,
            api_key: "stub-key".into(),
            protocol: "openai".into(),
            // Requests must now carry a token that selects a route; there is no
            // sentinel that waves authentication through.
            local_token: STUB_TOKEN.into(),
            responses_mode: ResponsesMode::Auto,
            max_price_per_request: None,
            rate_limit: polydeck_core::profile::RateLimitSettings::default(),
            default_effort_level: None,
            thinking_support: polydeck_core::types::ThinkingSupport::Unprobed,
        },
        Vec::new(),
    );
    config.listen_addr = Some(SocketAddr::from(([127, 0, 0, 1], 0)));
    config.timeout = Duration::from_secs(60);
    config.max_retries = 0;
    let mut server = GatewayServer::new(config);
    let addr = server.start().await.expect("gateway did not start");
    (server, format!("http://{addr}"))
}

async fn stream_a_turn(base: &str) -> String {
    let resp = reqwest::Client::new()
        .post(format!("{base}/v1/responses"))
        .bearer_auth(STUB_TOKEN)
        .json(&json!({
            "model": "stub-model",
            "stream": true,
            "input": [{
                "type": "message", "role": "user",
                "content": [{ "type": "input_text", "text": "hi" }]
            }]
        }))
        .send()
        .await
        .expect("gateway did not answer");
    assert_eq!(resp.status(), 200);
    resp.text().await.expect("body did not read")
}

/// The whole gateway, not just the repair type: a bridged stream severed
/// mid-body must still end the turn, because `response.completed` is the only
/// event Codex accepts as an end and it discards the turn without one.
#[tokio::test]
async fn a_severed_bridged_stream_still_ends_the_turn() {
    let upstream = stub_upstream(Ending::Sever).await;
    let (_server, base) = gateway_in_front_of(upstream).await;
    let body = stream_a_turn(&base).await;

    assert_eq!(
        body.matches("event: response.completed").count(),
        1,
        "no terminal event, so the client drops the turn:\n{body}"
    );
    assert!(
        body.contains("partial answer"),
        "text that arrived before the cut was lost:\n{body}"
    );
    // The error frame alone is not enough: measured 2026-09-01, the client kept
    // 3856 characters of a severed turn and stored no trace of the frame sent with
    // them, so the reason has to be in the text or the user never sees it.
    assert!(
        body.contains("上游连接中断"),
        "no reason in the assistant text, so the output just stops:\n{body}"
    );
}

/// The 2026-08-31 shape, and the one behind the 109 logged timeouts: the upstream
/// stops sending without closing. Ignored because it can only be observed by
/// waiting out `SSE_STREAM_IDLE_TIMEOUT`.
///
/// ```text
/// cargo test -p polydeck-gateway --test bridge_truncation -- --ignored --nocapture
/// ```
#[tokio::test]
#[ignore = "waits out the gateway's 25s SSE idle timeout"]
async fn a_silent_bridged_stream_still_ends_the_turn() {
    let upstream = stub_upstream(Ending::GoSilent).await;
    let (_server, base) = gateway_in_front_of(upstream).await;
    let started = std::time::Instant::now();
    let body = stream_a_turn(&base).await;

    assert!(
        started.elapsed() >= Duration::from_secs(20),
        "returned too early to have been the idle timeout: {:?}",
        started.elapsed()
    );
    assert_eq!(
        body.matches("event: response.completed").count(),
        1,
        "no terminal event after the idle timeout:\n{body}"
    );
    assert!(body.contains("partial answer"), "text lost:\n{body}");
    assert!(
        body.contains("timeout_error"),
        "the reason should still reach the client:\n{body}"
    );
    assert!(
        body.contains("网关判定超时"),
        "no reason in the assistant text:\n{body}"
    );
}
