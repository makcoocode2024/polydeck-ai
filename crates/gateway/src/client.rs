//! HTTP client for upstream requests

use polydeck_core::types::RelayChatCompat;
use reqwest::{Client, Response};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, warn};

/// The one origin that is the real OpenAI service rather than a relay.
const OFFICIAL_OPENAI_ORIGIN: &str = "https://api.openai.com";

/// Cline API host — auto-inject product headers when detected.
const CLINE_API_HOST: &str = "cline.bot";

/// Build the Cline product-identification headers required by api.cline.bot.
pub fn cline_product_headers() -> HashMap<String, String> {
    let mut h = HashMap::new();
    h.insert("HTTP-Referer".into(), "https://cline.bot".into());
    h.insert("X-Title".into(), "Cline".into());
    h.insert("X-IS-MULTIROOT".into(), "false".into());
    h.insert("X-CLIENT-TYPE".into(), "Cline".into());
    h.insert("User-Agent".into(), "Cline/0.0.83".into());
    h.insert("X-CLIENT-VERSION".into(), "0.0.83".into());
    h.insert("X-PLATFORM".into(), "cli".into());
    h.insert("X-PLATFORM-VERSION".into(), "0.0.83".into());
    h.insert("X-CORE-VERSION".into(), "0.0.83".into());
    h
}

/// True when the base URL points at the Cline API.
pub fn is_cline_api(base_url: &str) -> bool {
    base_url.contains(CLINE_API_HOST)
}

/// Ensure the API key has the `workos:` prefix required by Cline.
pub fn ensure_workos_prefix(key: &str) -> String {
    if key.to_lowercase().starts_with("workos:") {
        key.to_string()
    } else {
        format!("workos:{key}")
    }
}

#[derive(Clone)]
pub struct UpstreamClient {
    client: Client,
    base_url: String,
    api_key: String,
    max_retries: u32,
    /// Whether non-streaming Chat Completions requests are sent as streams and
    /// buffered back into a single JSON body. Comes from the provider's probed
    /// `relay_chat_compat`, so it is per-provider rather than process-wide.
    relay_chat_compat: RelayChatCompat,
    /// Extra headers injected into every outbound request.
    extra_headers: HashMap<String, String>,
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

/// True when `base_url` points at something other than the official OpenAI
/// service — i.e. an OpenAI-compatible relay.
///
/// An unparseable URL counts as custom: it is certainly not `api.openai.com`,
/// and the compat path is harmless where the request would fail anyway.
pub fn uses_custom_openai_base_url(base_url: &str) -> bool {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return false;
    }
    match url::Url::parse(trimmed) {
        Ok(parsed) => {
            let origin_matches = parsed.origin().ascii_serialization() == OFFICIAL_OPENAI_ORIGIN;
            let path_matches = parsed.path().trim_end_matches('/') == "/v1";
            !(origin_matches && path_matches)
        }
        Err(_) => true,
    }
}

/// Whether to route this provider's non-streaming Chat Completions through a
/// buffered stream.
///
/// The verdict comes from the provider's profile, where `protocol::probe` wrote
/// what it measured. The official OpenAI service is excluded outright: its
/// non-streaming body is correct, so even a profile that somehow asks for the
/// workaround does not get it.
pub fn should_buffer_chat(base_url: &str, relay_chat_compat: RelayChatCompat) -> bool {
    relay_chat_compat.should_buffer() && uses_custom_openai_base_url(base_url)
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

/// Ask for a stream, and for usage to be reported in it.
///
/// `include_usage` matters because the buffered result has to carry `usage`; a
/// stream without it would silently drop the token counts the caller reads.
fn streamify_chat_body(body: &Value) -> Value {
    let mut rewritten = body.clone();
    let Some(object) = rewritten.as_object_mut() else {
        return rewritten;
    };
    object.insert("stream".into(), Value::Bool(true));
    let mut stream_options = object
        .get("stream_options")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    stream_options.insert("include_usage".into(), Value::Bool(true));
    object.insert("stream_options".into(), Value::Object(stream_options));
    rewritten
}

/// What a buffered Chat Completions stream turned out to be.
pub(crate) enum BufferedChat {
    /// A `chat.completion` body assembled from the stream's deltas.
    Completion(Value),
    /// The stream carried an error frame; report it with this status.
    UpstreamError { status: u16, body: Value },
    /// The body held no `data:` frames at all, so it was not a stream. Returned
    /// verbatim rather than guessed at.
    NotSse(String),
}

/// Reassemble a Chat Completions SSE body into a single `chat.completion` JSON.
///
/// `requested_model` is the fallback for a stream whose chunks omit `model`.
/// Unparseable frames are skipped: one bad frame is not a reason to discard a
/// turn that is otherwise intact.
pub(crate) fn chat_completion_from_sse(raw: &str, requested_model: &str) -> BufferedChat {
    let data_lines: Vec<&str> = raw
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim)
        .collect();
    if data_lines.is_empty() {
        return BufferedChat::NotSse(raw.to_string());
    }

    let mut id = String::new();
    let mut created = 0u64;
    let mut model = requested_model.to_string();
    let mut content = String::new();
    let mut finish_reason = Value::Null;
    let mut usage: Option<Value> = None;
    // Keyed by the stream's `index`, so fragments of the same call concatenate
    // in arrival order and separate calls stay separate.
    let mut tool_calls: std::collections::BTreeMap<u64, ToolCallAccumulator> = Default::default();

    for data in data_lines {
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(chunk) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if let Some(error) = chunk.get("error").filter(|e| e.is_object()) {
            return BufferedChat::UpstreamError {
                status: error_status(error),
                body: chunk,
            };
        }
        if let Some(value) = chunk.get("id").and_then(Value::as_str) {
            id = value.to_string();
        }
        if let Some(value) = chunk.get("created").and_then(Value::as_u64) {
            created = value;
        }
        if let Some(value) = chunk.get("model").and_then(Value::as_str) {
            model = value.to_string();
        }
        if let Some(value) = chunk.get("usage").filter(|u| !u.is_null()) {
            usage = Some(value.clone());
        }
        for choice in chunk
            .get("choices")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            // Only the first choice is assembled; `n > 1` is not something the
            // clients this gateway serves ask for.
            if choice.get("index").and_then(Value::as_u64).unwrap_or(0) != 0 {
                continue;
            }
            if let Some(reason) = choice.get("finish_reason").filter(|r| !r.is_null()) {
                finish_reason = reason.clone();
            }
            let Some(delta) = choice.get("delta") else {
                continue;
            };
            content.push_str(&delta_text(delta.get("content")));
            for call in delta
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let index = call.get("index").and_then(Value::as_u64).unwrap_or(0);
                let entry = tool_calls.entry(index).or_default();
                if let Some(value) = call.get("id").and_then(Value::as_str) {
                    entry.id = value.to_string();
                }
                if let Some(value) = call.get("type").and_then(Value::as_str) {
                    entry.kind = value.to_string();
                }
                if let Some(function) = call.get("function") {
                    if let Some(name) = function.get("name").and_then(Value::as_str) {
                        if !name.is_empty() {
                            entry.name = name.to_string();
                        }
                    }
                    if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                        entry.arguments.push_str(arguments);
                    }
                }
            }
        }
    }

    let mut message = Map::new();
    message.insert("role".into(), Value::String("assistant".into()));
    message.insert("content".into(), Value::String(content));
    if !tool_calls.is_empty() {
        message.insert(
            "tool_calls".into(),
            Value::Array(
                tool_calls
                    .into_values()
                    .map(ToolCallAccumulator::into_json)
                    .collect(),
            ),
        );
    }
    let mut completion = json!({
        "id": if id.is_empty() { format!("chatcmpl_ad_{}", uuid::Uuid::new_v4().simple()) } else { id },
        "object": "chat.completion",
        "created": if created == 0 { chrono::Utc::now().timestamp() as u64 } else { created },
        "model": model,
        "choices": [{ "index": 0, "message": Value::Object(message), "finish_reason": finish_reason }],
    });
    if let Some(usage) = usage {
        completion["usage"] = usage;
    }
    BufferedChat::Completion(completion)
}

#[derive(Default)]
struct ToolCallAccumulator {
    id: String,
    kind: String,
    name: String,
    arguments: String,
}

impl ToolCallAccumulator {
    fn into_json(self) -> Value {
        json!({
            "id": self.id,
            "type": if self.kind.is_empty() { "function".to_string() } else { self.kind },
            "function": { "name": self.name, "arguments": self.arguments }
        })
    }
}

/// Text out of a `delta.content`, which relays send either as a string or as
/// OpenAI's content-part array.
fn delta_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect(),
        _ => String::new(),
    }
}

/// The HTTP status an in-stream error frame should be reported as.
fn error_status(error: &Value) -> u16 {
    let code = match error.get("code") {
        Some(Value::Number(number)) => number.as_u64(),
        Some(Value::String(text)) => text.parse::<u64>().ok(),
        _ => None,
    };
    match code {
        Some(status) if (400..=599).contains(&status) => status as u16,
        _ => 500,
    }
}

pub(crate) fn build_http_client(
    base_url: &str,
    timeout: Duration,
    accept_invalid_certs: bool,
) -> Result<Client, String> {
    let connect_timeout = (timeout / 2).min(Duration::from_secs(10));
    let mut builder = Client::builder()
        // No total `.timeout()`: reqwest's read timeout applies per read, so a
        // streaming body may run arbitrarily long as long as chunks keep
        // arriving. A total timeout would cut every stream off at `timeout`
        // seconds regardless of how healthy it is.
        .read_timeout(timeout)
        .connect_timeout(connect_timeout)
        // Relays in front of real upstreams routinely drop silent connections:
        // TCP keepalive marks the flow live at L4, HTTP/2 pings keep half-open
        // proxies from RSTing a long idle-ish stream, and a bounded idle pool
        // avoids racing a stale pooled connection against the next request.
        .tcp_keepalive(Duration::from_secs(30))
        .http2_keep_alive_interval(Duration::from_secs(30))
        .http2_keep_alive_timeout(Duration::from_secs(10))
        .pool_idle_timeout(Duration::from_secs(90))
        .danger_accept_invalid_certs(accept_invalid_certs)
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
    /// `accept_invalid_certs` is a constructor argument rather than a `with_`
    /// builder like [`Self::with_relay_chat_compat`]: reqwest bakes TLS policy
    /// into the `Client` at build time, so it cannot be flipped afterwards. It
    /// is also required rather than defaulted, so adding an upstream forces a
    /// deliberate answer instead of inheriting a bypass by omission.
    pub fn new(
        base_url: String,
        api_key: String,
        timeout: Duration,
        max_retries: u32,
        accept_invalid_certs: bool,
    ) -> Result<Self, String> {
        let client = build_http_client(&base_url, timeout, accept_invalid_certs)?;
        // Auto-detect Cline API and inject required headers + key prefix
        let (effective_key, auto_headers) = if is_cline_api(&base_url) {
            (ensure_workos_prefix(&api_key), cline_product_headers())
        } else {
            (api_key, HashMap::new())
        };
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: effective_key,
            max_retries,
            relay_chat_compat: RelayChatCompat::default(),
            extra_headers: auto_headers,
        })
    }

    /// Apply the provider's probed non-streaming Chat Completions verdict.
    ///
    /// Left at the default by [`Self::new`] so a caller with no profile in hand
    /// — a test, or a probe — behaves as it did before this existed.
    pub fn with_relay_chat_compat(mut self, relay_chat_compat: RelayChatCompat) -> Self {
        self.relay_chat_compat = relay_chat_compat;
        if should_buffer_chat(&self.base_url, relay_chat_compat) {
            debug!(
                "Relay workaround active for {}: a non-streaming /chat/completions \
                 will be sent as a stream and buffered back into JSON",
                self.base_url
            );
        }
        self
    }

    /// Apply provider-configured extra HTTP headers to every outbound request.
    /// Merges with any auto-detected headers (e.g. Cline); explicit values win.
    pub fn with_extra_headers(mut self, extra_headers: HashMap<String, String>) -> Self {
        for (k, v) in extra_headers {
            self.extra_headers.insert(k, v);
        }
        self
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
        let mut request = self
            .client
            .get(&url)
            .bearer_auth(&self.api_key)
            .header("x-api-key", &self.api_key);
        for (key, value) in &self.extra_headers {
            request = request.header(key.as_str(), value.as_str());
        }
        request.send().await.map_err(|e| {
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
        // The relay workaround only applies to a non-streaming Chat Completions
        // call. A request that already asked for a stream is forwarded as-is:
        // the streaming path is the one relays get right.
        let buffer_stream = should_buffer_chat(&self.base_url, self.relay_chat_compat)
            && url.ends_with("/chat/completions")
            && !body.get("stream").and_then(Value::as_bool).unwrap_or(false);
        let requested_model = body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let outbound = if buffer_stream {
            streamify_chat_body(&body)
        } else {
            body
        };

        let mut request = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json");
        for (key, value) in &self.extra_headers {
            request = request.header(key.as_str(), value.as_str());
        }
        let response = request.json(&outbound).send().await.map_err(|e| {
            UpstreamError::new(
                format!(
                    "Network error: {:?} | source: {:?}",
                    e,
                    std::error::Error::source(&e)
                ),
                e.is_connect(),
            )
        })?;

        if !buffer_stream || !response.status().is_success() {
            return Ok(response);
        }
        Ok(buffer_streamed_chat(response, &requested_model).await)
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

/// Read a streamed Chat Completions response to the end and hand back a
/// synthetic non-streaming one, so callers downstream see the JSON body they
/// asked for and never learn a stream was involved.
///
/// A read failure is reported as a 502 rather than swallowed: the caller's
/// retry and error handling both key off the status.
async fn buffer_streamed_chat(response: Response, requested_model: &str) -> Response {
    let status = response.status();
    let raw = match response.text().await {
        Ok(text) => text,
        Err(e) => {
            warn!("Failed to read buffered chat completions stream: {e}");
            return json_response(
                502,
                &json!({
                    "error": {
                        "message": format!("Failed to read upstream stream: {e}"),
                        "type": "gateway_error",
                        "code": 502
                    }
                }),
            );
        }
    };
    match chat_completion_from_sse(&raw, requested_model) {
        BufferedChat::Completion(completion) => {
            debug!("Buffered a relay's chat completions stream into a JSON body");
            json_response(status.as_u16(), &completion)
        }
        BufferedChat::UpstreamError { status, body } => {
            warn!("Relay stream carried an error frame; reporting it as {status}");
            json_response(status, &body)
        }
        // Not a stream after all. Whatever it is, it is the upstream's answer,
        // so pass it through untouched instead of inventing a shape for it.
        BufferedChat::NotSse(raw) => {
            let inner = axum::http::Response::builder()
                .status(status.as_u16())
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(raw)
                .expect("valid synthetic response");
            Response::from(inner)
        }
    }
}

fn json_response(status: u16, body: &Value) -> Response {
    let inner = axum::http::Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(body.to_string())
        .expect("valid synthetic response");
    Response::from(inner)
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
            false,
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
            false,
        )
        .unwrap();
        assert_eq!(client.base_url(), "https://api.example.com");
    }

    /// The official service is never treated as a relay, so the workaround
    /// cannot engage for it however the switch is set.
    #[test]
    fn official_openai_base_url_is_not_custom() {
        for official in [
            "https://api.openai.com/v1",
            "https://api.openai.com/v1/",
            "  https://api.openai.com/v1  ",
        ] {
            assert!(
                !uses_custom_openai_base_url(official),
                "{official} must count as the official service"
            );
        }
    }

    #[test]
    fn relay_base_urls_are_custom() {
        for relay in [
            "https://relay.example.com/v1",
            "https://api.openai.com/v2",
            "http://127.0.0.1:8080/v1",
            "not a url",
        ] {
            assert!(
                uses_custom_openai_base_url(relay),
                "{relay} must count as a relay"
            );
        }
    }

    /// A stream of deltas becomes one `chat.completion`, with tool call
    /// fragments concatenated and usage carried over.
    #[test]
    fn buffers_sse_into_a_chat_completion() {
        let raw = concat!(
            "data: {\"id\":\"chatcmpl-9\",\"created\":1712,\"model\":\"relay-model\",",
            "\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hel\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lo\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",",
            "\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\"\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,",
            "\"function\":{\"arguments\":\":\\\"a.txt\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":4,\"total_tokens\":14}}\n\n",
            "data: [DONE]\n\n"
        );

        let BufferedChat::Completion(completion) = chat_completion_from_sse(raw, "requested-model")
        else {
            panic!("stream should have assembled into a completion");
        };

        assert_eq!(completion["object"], "chat.completion");
        assert_eq!(completion["id"], "chatcmpl-9");
        assert_eq!(completion["created"], 1712);
        assert_eq!(completion["model"], "relay-model");
        let choice = &completion["choices"][0];
        assert_eq!(choice["message"]["content"], "Hello");
        assert_eq!(choice["finish_reason"], "tool_calls");
        let call = &choice["message"]["tool_calls"][0];
        assert_eq!(call["id"], "call_1");
        assert_eq!(call["function"]["name"], "read_file");
        assert_eq!(call["function"]["arguments"], "{\"path\":\"a.txt\"}");
        assert_eq!(completion["usage"]["total_tokens"], 14);
    }

    /// A chunk the relay sent as a content-part array still yields text, and a
    /// frame that is not JSON is skipped rather than failing the turn.
    #[test]
    fn buffers_content_parts_and_skips_junk_frames() {
        let raw = concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":[{\"type\":\"text\",\"text\":\"part\"}]}}]}\n\n",
            "data: {not json}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"-two\"}}]}\n\n"
        );

        let BufferedChat::Completion(completion) = chat_completion_from_sse(raw, "m") else {
            panic!("expected a completion");
        };
        assert_eq!(completion["choices"][0]["message"]["content"], "part-two");
        // No `model` in any chunk, so the requested one stands in.
        assert_eq!(completion["model"], "m");
    }

    #[test]
    fn in_stream_error_frame_becomes_an_error_status() {
        let raw = "data: {\"error\":{\"message\":\"quota exceeded\",\"code\":429}}\n\n";
        let BufferedChat::UpstreamError { status, body } = chat_completion_from_sse(raw, "m")
        else {
            panic!("expected an upstream error");
        };
        assert_eq!(status, 429);
        assert_eq!(body["error"]["message"], "quota exceeded");
    }

    /// A body with no `data:` frames is not a stream; it is handed back as-is
    /// rather than turned into an empty completion.
    #[test]
    fn body_without_sse_frames_is_passed_through() {
        let raw = "{\"id\":\"chatcmpl-plain\",\"object\":\"chat.completion\"}";
        let BufferedChat::NotSse(passed) = chat_completion_from_sse(raw, "m") else {
            panic!("expected passthrough");
        };
        assert_eq!(passed, raw);
    }

    #[test]
    fn streamify_asks_for_a_stream_and_usage() {
        let rewritten = streamify_chat_body(&json!({ "model": "m", "messages": [] }));
        assert_eq!(rewritten["stream"], true);
        assert_eq!(rewritten["stream_options"]["include_usage"], true);
        // The rest of the body is untouched.
        assert_eq!(rewritten["model"], "m");
    }

    /// An existing `stream_options` is extended, not replaced.
    #[test]
    fn streamify_preserves_existing_stream_options() {
        let rewritten = streamify_chat_body(&json!({
            "model": "m",
            "stream_options": { "something_else": 1 }
        }));
        assert_eq!(rewritten["stream_options"]["something_else"], 1);
        assert_eq!(rewritten["stream_options"]["include_usage"], true);
    }
}
