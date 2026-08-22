//! HTTP client for upstream requests

use reqwest::{Client, Response};
use serde_json::Value;
use std::time::Duration;
use tracing::{debug, warn};

#[derive(Clone)]
pub struct UpstreamClient {
    client: Client,
    base_url: String,
    api_key: String,
    #[allow(dead_code)]
    timeout: Duration,
    max_retries: u32,
}

pub fn is_loopback_url(url: &str) -> bool {
    let host_port = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default();
    let host = if host_port.starts_with('[') {
        host_port
            .strip_prefix('[')
            .and_then(|r| r.split_once(']').map(|(h, _)| h))
            .unwrap_or(host_port)
    } else {
        host_port
            .rsplit_once(':')
            .map(|(h, _)| h)
            .unwrap_or(host_port)
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    ChatCompletions,
    Responses,
    Messages,
    Models,
    CountTokens,
}

impl Endpoint {
    pub fn path(self, base_url: &str) -> &'static str {
        let versioned = base_url.ends_with("/v1");
        match (self, versioned) {
            (Endpoint::ChatCompletions, true) => "/chat/completions",
            (Endpoint::ChatCompletions, false) => "/v1/chat/completions",
            (Endpoint::Responses, true) => "/responses",
            (Endpoint::Responses, false) => "/v1/responses",
            (Endpoint::Messages, true) => "/messages",
            (Endpoint::Messages, false) => "/v1/messages",
            (Endpoint::Models, true) => "/models",
            (Endpoint::Models, false) => "/v1/models",
            (Endpoint::CountTokens, true) => "/messages/count_tokens",
            (Endpoint::CountTokens, false) => "/v1/messages/count_tokens",
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpstreamError {
    pub message: String,
    pub never_sent: bool,
}

impl UpstreamError {
    fn new(message: impl Into<String>, never_sent: bool) -> Self {
        Self {
            message: message.into(),
            never_sent,
        }
    }
}

impl std::fmt::Display for UpstreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for UpstreamError {}

impl From<UpstreamError> for String {
    fn from(error: UpstreamError) -> Self {
        error.message
    }
}

pub(crate) fn build_http_client(base_url: &str, timeout: Duration) -> Result<Client, String> {
    let connect_timeout = (timeout / 2).min(Duration::from_secs(10));
    let mut builder = Client::builder()
        .timeout(timeout)
        .connect_timeout(connect_timeout)
        .danger_accept_invalid_certs(true)
        .use_rustls_tls();
    if is_loopback_url(base_url) {
        builder = builder.no_proxy();
    } else if let Some(proxy_url) = polydeck_core::proxy_manager::get_configured_proxy() {
        if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
            builder = builder.proxy(proxy);
        }
    }
    builder
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))
}

impl UpstreamClient {
    pub fn new(
        base_url: String,
        api_key: String,
        timeout: Duration,
        max_retries: u32,
    ) -> Result<Self, String> {
        let client = build_http_client(&base_url, timeout)?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            timeout,
            max_retries,
        })
    }

    pub async fn chat_completions(&self, body: Value) -> Result<Response, UpstreamError> {
        self.send(Endpoint::ChatCompletions, body).await
    }

    pub async fn responses(&self, body: Value) -> Result<Response, UpstreamError> {
        self.send(Endpoint::Responses, body).await
    }

    pub async fn messages(&self, body: Value) -> Result<Response, UpstreamError> {
        self.send(Endpoint::Messages, body).await
    }

    pub async fn get_models(&self) -> Result<Response, UpstreamError> {
        let url = format!("{}{}", self.base_url, Endpoint::Models.path(&self.base_url));
        self.client
            .get(&url)
            .bearer_auth(&self.api_key)
            .header("x-api-key", &self.api_key)
            .send()
            .await
            .map_err(|e| {
                UpstreamError::new(
                    format!(
                        "Network error: {:?} | source: {:?}",
                        e,
                        std::error::Error::source(&e)
                    ),
                    e.is_connect(),
                )
            })
    }

    pub async fn send(&self, endpoint: Endpoint, body: Value) -> Result<Response, UpstreamError> {
        let url = format!("{}{}", self.base_url, endpoint.path(&self.base_url));
        self.send_with_retry(&url, body).await
    }

    async fn send_with_retry(&self, url: &str, body: Value) -> Result<Response, UpstreamError> {
        let mut last_error = String::new();
        let mut never_sent = true;
        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                let backoff = Duration::from_millis(100 * (1 << (attempt - 1)));
                debug!("Retry attempt {} after {:?}", attempt, backoff);
                tokio::time::sleep(backoff).await;
            }
            match self.send_once(url, body.clone()).await {
                Ok(response) => {
                    if is_retryable_status(&response) && attempt < self.max_retries {
                        last_error = format!("upstream returned status {}", response.status());
                        warn!("Retryable status (attempt {}): {}", attempt + 1, last_error);
                        never_sent = false;
                        continue;
                    }
                    return Ok(response);
                }
                Err(error) => {
                    never_sent &= error.never_sent;
                    last_error = error.message;
                    warn!("Request failed (attempt {}): {}", attempt + 1, last_error);
                }
            }
        }
        Err(UpstreamError::new(
            format!(
                "Request failed after {} attempts: {}",
                self.max_retries + 1,
                last_error
            ),
            never_sent,
        ))
    }

    async fn send_once(&self, url: &str, body: Value) -> Result<Response, UpstreamError> {
        self.client
            .post(url)
            .bearer_auth(&self.api_key)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                UpstreamError::new(
                    format!(
                        "Network error: {:?} | source: {:?}",
                        e,
                        std::error::Error::source(&e)
                    ),
                    e.is_connect(),
                )
            })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

fn is_retryable_status(response: &Response) -> bool {
    let status = response.status();
    // Only retry transient 5xx server errors in single-provider client (502, 503, 504).
    // 429 Too Many Requests should NOT be blind-retried immediately in the proxy,
    // because upstream rate limits need real backoff and downstream clients (Hermes/Codex/Claude)
    // already implement their own rate limit backoff.
    status == reqwest::StatusCode::BAD_GATEWAY
        || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
        || status == reqwest::StatusCode::GATEWAY_TIMEOUT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_loopback_urls() {
        assert!(is_loopback_url("http://127.0.0.1:7890"));
        assert!(is_loopback_url("http://localhost:1234/v1"));
        assert!(is_loopback_url("http://[::1]:8080"));
    }

    #[test]
    fn recognises_remote_urls() {
        assert!(!is_loopback_url("https://api.openai.com"));
        assert!(!is_loopback_url("http://192.168.1.10:7890"));
    }

    #[test]
    fn creates_client() {
        let client = UpstreamClient::new(
            "https://api.example.com".into(),
            "test-key".into(),
            Duration::from_secs(30),
            3,
        );
        assert!(client.is_ok());
        assert_eq!(client.unwrap().base_url(), "https://api.example.com");
    }

    #[test]
    fn trims_trailing_slash() {
        let client = UpstreamClient::new(
            "https://api.example.com/".into(),
            "k".into(),
            Duration::from_secs(30),
            3,
        )
        .unwrap();
        assert_eq!(client.base_url(), "https://api.example.com");
    }
}
