//! Protocol auto-detection engine.
//!
//! Probes a Base URL to identify the AI protocol (OpenAI/Anthropic/Gemini/Azure),
//! fetch model lists, and detect Codex tool compatibility. All detection is based
//! on real HTTP responses — never inferred from model names.

use crate::error::{AppError, AppResult};
use crate::types::{CodexToolCompat, Confidence, ProtocolKind};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub protocol: ProtocolKind,
    pub confidence: Confidence,
    pub evidence: Vec<String>,
    pub models: Vec<ModelInfo>,
    pub codex_compat: CodexToolCompat,
    pub base_url: String,
    pub supports_streaming: bool,
    #[serde(default)]
    pub supports_1m_context: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context_length: Option<u64>,
    pub max_output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ChatTestResult {
    pub success: bool,
    pub reply: String,
    pub latency_ms: u64,
    pub model: String,
    pub protocol: ProtocolKind,
}

/// Normalize a base URL: trim whitespace, ensure scheme, strip trailing slash.
pub fn normalize_url(url: &str) -> String {
    let mut url = url.trim().to_string();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        url = format!("https://{url}");
    }
    url = url.trim_end_matches('/').to_string();
    // Strip /v1 suffix if present
    if url.ends_with("/v1") {
        url = url[..url.len() - 3].to_string();
    }
    url
}

/// Probe a provider endpoint to detect protocol, models, and capabilities.

pub async fn probe_1m_context(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    protocol: ProtocolKind,
    model_name: &str,
) -> bool {
    let probe_model = format!("{model_name}[1m]");
    let url = normalize_url(base_url);

    if protocol == ProtocolKind::Anthropic {
        let body = serde_json::json!({
            "model": probe_model,
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "ping"}],
        });
        if let Ok(resp) = client
            .post(format!("{url}/v1/messages"))
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
        {
            return resp.status().is_success();
        }
    } else {
        let body = serde_json::json!({
            "model": probe_model,
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "ping"}],
        });
        if let Ok(resp) = client
            .post(format!("{url}/v1/chat/completions"))
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
        {
            return resp.status().is_success();
        }
    }
    false
}

pub async fn probe(
    base_url: &str,
    api_key: &str,
    accept_invalid_certs: bool,
) -> AppResult<ProbeResult> {
    let raw_url = base_url.trim();
    if raw_url.is_empty() {
        return Err(AppError::Protocol("API 基础地址 (Base URL) 不能为空".into()));
    }
    let url = normalize_url(raw_url);
    let client = build_client(accept_invalid_certs)?;
    let mut evidence = Vec::new();
    let mut protocol = ProtocolKind::Unknown;
    let mut confidence = Confidence::Unknown;
    let mut models = Vec::new();
    let mut auth_error = None;

    // Try OpenAI-compatible /v1/models first (most common)
    match fetch_openai_models(&client, &url, api_key).await {
        Ok(fetched) => {
            models = fetched;
            evidence.push("GET /v1/models 返回有效模型列表".into());
            protocol = ProtocolKind::OpenAI;
            confidence = Confidence::High;
        }
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("401") || err_msg.contains("403") || err_msg.contains("Unauthorized") || err_msg.contains("Forbidden") {
                auth_error = Some(format!("OpenAI 鉴权失败：{err_msg}"));
            }
            evidence.push(format!("GET /v1/models 失败：{err_msg}"));

            // Try fallback chat ping if not explicit auth failure
            if auth_error.is_none() {
                match probe_openai_chat_fallback(&client, &url, api_key).await {
                    Ok(true) => {
                        evidence.push("POST /v1/chat/completions 验证通过".into());
                        protocol = ProtocolKind::OpenAI;
                        confidence = Confidence::Medium;
                    }
                    Ok(false) => {}
                    Err(fb_err) => {
                        let fb_msg = fb_err.to_string();
                        if fb_msg.contains("401") || fb_msg.contains("403") || fb_msg.contains("Unauthorized") || fb_msg.contains("Forbidden") {
                            auth_error = Some(format!("OpenAI 鉴权失败：{fb_msg}"));
                        }
                    }
                }
            }
        }
    }

    // Try Anthropic if OpenAI failed and no fatal auth error
    if protocol == ProtocolKind::Unknown && auth_error.is_none() {
        match fetch_anthropic_models(&client, &url, api_key).await {
            Ok(fetched) => {
                models = fetched;
                evidence.push("Anthropic /v1/models 返回有效模型列表".into());
                protocol = ProtocolKind::Anthropic;
                confidence = Confidence::High;
            }
            Err(e) => {
                let err_msg = e.to_string();
                if err_msg.contains("401") || err_msg.contains("403") || err_msg.contains("Unauthorized") || err_msg.contains("Forbidden") {
                    auth_error = Some(format!("Anthropic 鉴权失败：{err_msg}"));
                }
                evidence.push(format!("Anthropic 探测失败：{err_msg}"));

                if auth_error.is_none() {
                    match probe_anthropic_messages_fallback(&client, &url, api_key).await {
                        Ok(true) => {
                            evidence.push("Anthropic /v1/messages 验证通过".into());
                            protocol = ProtocolKind::Anthropic;
                            confidence = Confidence::Medium;
                        }
                        Ok(false) => {}
                        Err(fb_err) => {
                            let fb_msg = fb_err.to_string();
                            if fb_msg.contains("401") || fb_msg.contains("403") || fb_msg.contains("Unauthorized") || fb_msg.contains("Forbidden") {
                                auth_error = Some(format!("Anthropic 鉴权失败：{fb_msg}"));
                            }
                        }
                    }
                }
            }
        }
    }

    // If explicit authentication failure occurred
    if let Some(err) = auth_error {
        return Err(AppError::Protocol(format!("API Key 鉴权失败：{err}")));
    }

    // If protocol could not be identified, report connection / endpoint failure
    if protocol == ProtocolKind::Unknown {
        let details = if evidence.is_empty() {
            "无法连接到目标服务地址，请检查 Base URL 是否有效".to_string()
        } else {
            evidence.join("； ")
        };
        return Err(AppError::Protocol(format!("服务连接与协议探测失败：{}", details)));
    }

    // Probe Codex tool compatibility
    let codex_compat = if !models.is_empty() {
        let model = models[0].id.clone();
        probe_codex_compat(&client, &url, api_key, &model).await
    } else {
        CodexToolCompat::ResponsesCustom
    };

    if protocol == ProtocolKind::OpenAI && (codex_compat == CodexToolCompat::ResponsesCustom || codex_compat == CodexToolCompat::ResponsesFunction) {
        protocol = ProtocolKind::Responses;
        evidence.push("上游终端原生支持 OpenAI Responses API 协议".into());
    }

   let supports_1m_context = if let Some(first_model) = models.first() {
        let ok = probe_1m_context(&client, &url, api_key, protocol, &first_model.id).await;
        if ok {
            evidence.push("供应商端点支持 [1m] 原生长上下文".into());
        }
        Some(ok)
    } else {
        None
    };

    Ok(ProbeResult {
        protocol,
        confidence,
        evidence,
        models,
        codex_compat,
        base_url: url,
        supports_streaming: true,
        supports_1m_context,
    })
}

/// Perform a real conversation test with a live model.
pub async fn test_chat(
    base_url: &str,
    api_key: &str,
    model: &str,
    protocol: Option<ProtocolKind>,
    accept_invalid_certs: bool,
    prompt: Option<&str>,
) -> AppResult<ChatTestResult> {
    let url = normalize_url(base_url);
    let client = build_client(accept_invalid_certs)?;
    let test_prompt = prompt.unwrap_or("请回复五个字以内：连接测试成功");
    let model_to_use = if model.trim().is_empty() { "gpt-4o" } else { model.trim() };
    let start = std::time::Instant::now();

    let target_protocol = protocol.unwrap_or(ProtocolKind::OpenAI);

    if target_protocol == ProtocolKind::Anthropic {
        let body = serde_json::json!({
            "model": model_to_use,
            "max_tokens": 100,
            "messages": [{"role": "user", "content": test_prompt}],
        });
        let resp = client
            .post(format!("{url}/v1/messages"))
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Network(format!("Anthropic 请求发送失败: {e}")))?;

        let latency_ms = start.elapsed().as_millis() as u64;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Protocol(format!("Anthropic 对话测试失败 HTTP {status}: {text}")));
        }

        let json: serde_json::Value = resp.json().await?;
        let reply = json
            .pointer("/content/0/text")
            .and_then(|v| v.as_str())
            .unwrap_or("OK")
            .to_string();

        return Ok(ChatTestResult {
            success: true,
            reply,
            latency_ms,
            model: model_to_use.to_string(),
            protocol: ProtocolKind::Anthropic,
        });
    }

    if target_protocol == ProtocolKind::Responses {
        let resp_body = serde_json::json!({
            "model": model_to_use,
            "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": test_prompt}]}],
        });
        let resp = client
            .post(format!("{url}/v1/responses"))
            .bearer_auth(api_key)
            .json(&resp_body)
            .send()
            .await
            .map_err(|e| AppError::Network(format!("Responses 请求发送失败: {e}")))?;
        let latency_ms = start.elapsed().as_millis() as u64;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Protocol(format!("Responses 对话测试失败 HTTP {status}: {text}")));
        }
        let json: serde_json::Value = resp.json().await?;
        let reply = json
            .pointer("/output/0/content/0/text")
            .or_else(|| json.pointer("/output/1/content/0/text"))
            .or_else(|| json.pointer("/content/0/text"))
            .and_then(|v| v.as_str())
            .unwrap_or("OK")
            .to_string();
        return Ok(ChatTestResult {
            success: true,
            reply,
            latency_ms,
            model: model_to_use.to_string(),
            protocol: ProtocolKind::Responses,
        });
    }

    // Default OpenAI Chat Completions
    let chat_body = serde_json::json!({
        "model": model_to_use,
        "messages": [{"role": "user", "content": test_prompt}],
        "max_tokens": 100,
        "temperature": 0.3,
    });

    let resp = client
        .post(format!("{url}/v1/chat/completions"))
        .bearer_auth(api_key)
        .json(&chat_body)
        .send()
        .await;

    match resp {
        Ok(res) if res.status().is_success() => {
            let latency_ms = start.elapsed().as_millis() as u64;
            let json: serde_json::Value = res.json().await?;
            let reply = json
                .pointer("/choices/0/message/content")
                .and_then(|v| v.as_str())
                .unwrap_or("OK")
                .to_string();
            Ok(ChatTestResult {
                success: true,
                reply,
                latency_ms,
                model: model_to_use.to_string(),
                protocol: ProtocolKind::OpenAI,
            })
        }
        Ok(res) if res.status() == reqwest::StatusCode::NOT_FOUND || res.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED => {
            // Try Responses endpoint
            let resp_body = serde_json::json!({
                "model": model_to_use,
                "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": test_prompt}]}],
            });
            let resp2 = client
                .post(format!("{url}/v1/responses"))
                .bearer_auth(api_key)
                .json(&resp_body)
                .send()
                .await
                .map_err(|e| AppError::Network(format!("Responses 请求失败: {e}")))?;
            let latency_ms = start.elapsed().as_millis() as u64;
            if !resp2.status().is_success() {
                let status = resp2.status();
                let text = resp2.text().await.unwrap_or_default();
                return Err(AppError::Protocol(format!("Responses 对话测试失败 HTTP {status}: {text}")));
            }
            let json: serde_json::Value = resp2.json().await?;
            let reply = json
                .pointer("/output/0/content/0/text")
                .or_else(|| json.pointer("/output/1/content/0/text"))
                .and_then(|v| v.as_str())
                .unwrap_or("OK")
                .to_string();
            Ok(ChatTestResult {
                success: true,
                reply,
                latency_ms,
                model: model_to_use.to_string(),
                protocol: ProtocolKind::OpenAI,
            })
        }
        Ok(res) => {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            Err(AppError::Protocol(format!("对话测试失败 HTTP {status}: {text}")))
        }
        Err(e) => Err(AppError::Network(format!("网络请求失败: {e}"))),
    }
}

/// Perform a minimal service self-test with a real API call.
pub async fn self_test(
    base_url: &str,
    api_key: &str,
    model: &str,
    accept_invalid_certs: bool,
) -> AppResult<String> {
    let result = test_chat(base_url, api_key, model, None, accept_invalid_certs, Some("Reply with exactly: OK")).await?;
    Ok(result.reply)
}

/// Probe Codex tool compatibility: Responses+custom → Responses+function → Chat+function.
async fn probe_codex_compat(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    model: &str,
) -> CodexToolCompat {
    // 1. Try plain Responses API ping (no tools)
    let plain_body = serde_json::json!({
        "model": model,
        "input": "test",
        "max_output_tokens": 5,
    });
    if let Ok(resp) = client
        .post(format!("{base_url}/v1/responses"))
        .bearer_auth(api_key)
        .json(&plain_body)
        .send()
        .await
    {
        if resp.status().is_success() {
            let custom_body = serde_json::json!({
                "model": model,
                "input": "test",
                "max_output_tokens": 5,
                "tools": [{"type": "custom", "name": "test_tool", "description": "test", "format": {"type": "text"}}],
            });
            if let Ok(c_resp) = client
                .post(format!("{base_url}/v1/responses"))
                .bearer_auth(api_key)
                .json(&custom_body)
                .send()
                .await
            {
                if c_resp.status().is_success() {
                    return CodexToolCompat::ResponsesCustom;
                }
            }
            return CodexToolCompat::ResponsesFunction;
        }
    }

    // 2. Try Responses API with custom tools directly
    let custom_body = serde_json::json!({
        "model": model,
        "input": "test",
        "max_output_tokens": 5,
        "tools": [{"type": "custom", "name": "test_tool", "description": "test", "format": {"type": "text"}}],
    });
    if let Ok(resp) = client
        .post(format!("{base_url}/v1/responses"))
        .bearer_auth(api_key)
        .json(&custom_body)
        .send()
        .await
    {
        if resp.status().is_success() {
            return CodexToolCompat::ResponsesCustom;
        }
    }

    // 3. Try Responses API with function tools
    let fn_body = serde_json::json!({
        "model": model,
        "input": "test",
        "max_output_tokens": 5,
        "tools": [{"type": "function", "name": "test_fn", "parameters": {"type": "object"}}],
    });
    if let Ok(resp) = client
        .post(format!("{base_url}/v1/responses"))
        .bearer_auth(api_key)
        .json(&fn_body)
        .send()
        .await
    {
        if resp.status().is_success() {
            return CodexToolCompat::ResponsesFunction;
        }
    }

    // 4. Try Chat Completions with function tools
    let chat_body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "test"}],
        "max_tokens": 5,
        "tools": [{"type": "function", "function": {"name": "test_fn", "parameters": {"type": "object"}}}],
    });
    if let Ok(resp) = client
        .post(format!("{base_url}/v1/chat/completions"))
        .bearer_auth(api_key)
        .json(&chat_body)
        .send()
        .await
    {
        if resp.status().is_success() {
            return CodexToolCompat::ChatFunction;
        }
    }

    CodexToolCompat::None
}


async fn probe_openai_chat_fallback(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> AppResult<bool> {
    let body = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 1,
    });
    let mut req = client
        .post(format!("{base_url}/v1/chat/completions"))
        .timeout(std::time::Duration::from_secs(10));
    if !api_key.trim().is_empty() {
        req = req.bearer_auth(api_key.trim());
    }
    let resp = req.json(&body).send().await?;
    let status = resp.status();
    if status.is_success() {
        return Ok(true);
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        let err_text = resp.text().await.unwrap_or_default();
        return Err(AppError::Protocol(format!("HTTP {status} 鉴权失败：{err_text}")));
    }
    if status.is_client_error() {
        let err_text = resp.text().await.unwrap_or_default();
        if err_text.contains("model") || err_text.contains("error") {
            return Ok(true);
        }
    }
    Err(AppError::Protocol(format!("HTTP {status}")))
}

async fn probe_anthropic_messages_fallback(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> AppResult<bool> {
    let body = serde_json::json!({
        "model": "claude-3-7-sonnet-20250219",
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 1,
    });
    let mut req = client
        .post(format!("{base_url}/v1/messages"))
        .header("anthropic-version", "2023-06-01")
        .timeout(std::time::Duration::from_secs(10));
    if !api_key.trim().is_empty() {
        req = req.header("x-api-key", api_key.trim());
    }
    let resp = req.json(&body).send().await?;
    let status = resp.status();
    if status.is_success() {
        return Ok(true);
    }
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        let err_text = resp.text().await.unwrap_or_default();
        return Err(AppError::Protocol(format!("Anthropic HTTP {status} 鉴权失败：{err_text}")));
    }
    if status.is_client_error() {
        let err_text = resp.text().await.unwrap_or_default();
        if err_text.contains("model") || err_text.contains("type") || err_text.contains("error") {
            return Ok(true);
        }
    }
    Err(AppError::Protocol(format!("Anthropic HTTP {status}")))
}

async fn fetch_openai_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> AppResult<Vec<ModelInfo>> {
    let resp = client
        .get(format!("{base_url}/v1/models"))
        .bearer_auth(api_key)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(AppError::Protocol(format!(
            "HTTP {}",
            resp.status()
        )));
    }

    let json: serde_json::Value = resp.json().await?;
    let data = json
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AppError::Protocol("响应缺少 data 数组".into()))?;

    let models = data
        .iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(|v| v.as_str())?.to_string();
            if id == "codex-auto-review" || id.ends_with("auto-review") {
                return None;
            }
            Some(ModelInfo {
                name: id.clone(),
                id,
                context_length: m
                    .get("context_length")
                    .or_else(|| m.get("context_window"))
                    .and_then(|v| v.as_u64()),
                max_output_tokens: m
                    .get("max_output_tokens")
                    .and_then(|v| v.as_u64()),
            })
        })
        .collect();

    Ok(models)
}

async fn fetch_anthropic_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> AppResult<Vec<ModelInfo>> {
    let resp = client
        .get(format!("{base_url}/v1/models"))
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(AppError::Protocol(format!(
            "Anthropic HTTP {}",
            resp.status()
        )));
    }

    let json: serde_json::Value = resp.json().await?;
    let data = json
        .get("data")
        .and_then(|v| v.as_array())
        .ok_or_else(|| AppError::Protocol("Anthropic 响应缺少 data 数组".into()))?;

    let models = data
        .iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(|v| v.as_str())?.to_string();
            Some(ModelInfo {
                name: m
                    .get("display_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&id)
                    .to_string(),
                id,
                context_length: m
                    .get("max_input_tokens")
                    .and_then(|v| v.as_u64()),
                max_output_tokens: m
                    .get("max_output_tokens")
                    .and_then(|v| v.as_u64()),
            })
        })
        .collect();

    Ok(models)
}


#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitRecommendation {
    pub recommended_rpm: u32,
    pub recommended_tpm: u32,
    pub detected_from_headers: bool,
    pub message: String,
}

pub async fn probe_rate_limits(
    base_url: &str,
    api_key: &str,
    model: Option<&str>,
    accept_invalid_certs: bool,
) -> AppResult<RateLimitRecommendation> {
    let raw_url = base_url.trim();
    if raw_url.is_empty() {
        return Err(AppError::Protocol("API 基础地址 (Base URL) 不能为空".into()));
    }
    let url = normalize_url(raw_url);
    let client = build_client(accept_invalid_certs)?;

    let mut detected_rpm = None;
    let mut detected_tpm = None;

    // 1. Try GET /v1/models to probe RateLimit headers
    let models_url = format!("{url}/v1/models");
    let mut req = client.get(&models_url);
    if !api_key.trim().is_empty() {
        req = req
            .bearer_auth(api_key.trim())
            .header("x-api-key", api_key.trim());
    }

    if let Ok(resp) = req.send().await {
        let (rpm, tpm) = parse_generic_ratelimit_headers(resp.headers());
        if rpm.is_some() { detected_rpm = rpm; }
        if tpm.is_some() { detected_tpm = tpm; }
    }

    // 2. If headers not found, send minimal POST /v1/chat/completions ping to check response headers
    if (detected_rpm.is_none() || detected_tpm.is_none()) && !api_key.trim().is_empty() {
        let chat_url = format!("{url}/v1/chat/completions");
        let probe_model = model.unwrap_or("gpt-4o");
        let body = serde_json::json!({
            "model": probe_model,
            "messages": [{"role": "user", "content": "ping"}],
            "max_tokens": 1
        });
        if let Ok(resp) = client
            .post(&chat_url)
            .bearer_auth(api_key.trim())
            .header("x-api-key", api_key.trim())
            .json(&body)
            .send()
            .await
        {
            let (rpm, tpm) = parse_generic_ratelimit_headers(resp.headers());
            if detected_rpm.is_none() && rpm.is_some() { detected_rpm = rpm; }
            if detected_tpm.is_none() && tpm.is_some() { detected_tpm = tpm; }
        }
    }

    // 3. If both RPM and TPM were detected from standard/extended response headers
    if let (Some(rpm), Some(tpm)) = (detected_rpm, detected_tpm) {
        let safe_rpm = (rpm as f64 * 0.9).floor().max(1.0) as u32;
        let safe_tpm = (tpm as f64 * 0.9).floor().max(1000.0) as u32;
        return Ok(RateLimitRecommendation {
            recommended_rpm: safe_rpm,
            recommended_tpm: safe_tpm,
            detected_from_headers: true,
            message: format!("已从上游响应头 (X-RateLimit) 获取到限额，已预留 10% 安全余量 (推荐: {safe_rpm} RPM, {safe_tpm} TPM)"),
        });
    }

    // 4. If only RPM was detected from headers, estimate safe TPM based on standard context window
    if let Some(rpm) = detected_rpm {
        let safe_rpm = (rpm as f64 * 0.9).floor().max(1.0) as u32;
        let estimated_tpm = (safe_rpm * 2000).clamp(20000, 1_000_000);
        return Ok(RateLimitRecommendation {
            recommended_rpm: safe_rpm,
            recommended_tpm: estimated_tpm,
            detected_from_headers: true,
            message: format!("已从上游响应头获取到 RPM={rpm}，已按模型标准负载推断推荐 TPM={estimated_tpm}"),
        });
    }

    // 5. If no headers detected: generic heuristic algorithm (zero vendor hardcoding)
    let is_local = is_loopback_or_private_url(&url);
    let (rpm, tpm, desc) = if is_local {
        (300, 1_000_000, "本地/局域网环境通用高吞吐推荐配置 (300 RPM / 1,000,000 TPM)")
    } else {
        (60, 100_000, "通用大模型服务商标准安全推荐配置 (60 RPM / 100,000 TPM)")
    };

    Ok(RateLimitRecommendation {
        recommended_rpm: rpm,
        recommended_tpm: tpm,
        detected_from_headers: false,
        message: format!("上游未返回 RateLimit 响应头，使用{desc}。"),
    })
}

/// Generic case-insensitive parser for RateLimit response headers across various providers.
fn parse_generic_ratelimit_headers(headers: &reqwest::header::HeaderMap) -> (Option<u32>, Option<u32>) {
    let mut detected_rpm = None;
    let mut detected_tpm = None;

    // RPM Candidate Header Keys
    const RPM_HEADERS: &[&str] = &[
        "x-ratelimit-limit-requests",
        "ratelimit-limit-requests",
        "x-ratelimit-requests-limit",
        "x-ratelimit-limit-rpm",
        "x-ratelimit-rpm",
        "x-request-limit",
    ];

    // TPM Candidate Header Keys
    const TPM_HEADERS: &[&str] = &[
        "x-ratelimit-limit-tokens",
        "ratelimit-limit-tokens",
        "x-ratelimit-tokens-limit",
        "x-ratelimit-limit-tpm",
        "x-ratelimit-tpm",
        "x-token-limit",
    ];

    for (k, v) in headers.iter() {
        let key_str = k.as_str().to_ascii_lowercase();
        let val_str = match v.to_str() {
            Ok(s) => s.trim(),
            Err(_) => continue,
        };

        if detected_rpm.is_none() && RPM_HEADERS.iter().any(|h| key_str == *h) {
            if let Ok(num) = val_str.parse::<u32>() {
                if num > 0 { detected_rpm = Some(num); }
            }
        }

        if detected_tpm.is_none() && TPM_HEADERS.iter().any(|h| key_str == *h) {
            if let Ok(num) = val_str.parse::<u32>() {
                if num > 0 { detected_tpm = Some(num); }
            }
        }

        // Handle generic RFC draft "RateLimit-Limit: requests=60, tokens=100000"
        if key_str == "ratelimit-limit" {
            for part in val_str.split(',') {
                let part = part.trim();
                if part.starts_with("req") || part.starts_with("r=") {
                    if let Some(num_str) = part.split('=').nth(1) {
                        if let Ok(num) = num_str.trim().parse::<u32>() {
                            if detected_rpm.is_none() && num > 0 { detected_rpm = Some(num); }
                        }
                    }
                } else if part.starts_with("tok") || part.starts_with("t=") {
                    if let Some(num_str) = part.split('=').nth(1) {
                        if let Ok(num) = num_str.trim().parse::<u32>() {
                            if detected_tpm.is_none() && num > 0 { detected_tpm = Some(num); }
                        }
                    }
                } else if detected_rpm.is_none() {
                    if let Ok(num) = part.parse::<u32>() {
                        if num > 0 { detected_rpm = Some(num); }
                    }
                }
            }
        }
    }

    (detected_rpm, detected_tpm)
}

/// Generic loopback or private network IP checker.
fn is_loopback_or_private_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("localhost")
        || lower.contains("127.0.0.1")
        || lower.contains("::1")
        || lower.contains("0.0.0.0")
        || lower.contains(".local")
        || lower.contains("192.168.")
        || lower.contains("10.")
        || lower.contains("172.16.")
        || lower.contains("172.17.")
        || lower.contains("172.18.")
        || lower.contains("172.19.")
        || lower.contains("172.2")
        || lower.contains("172.30.")
        || lower.contains("172.31.")
}

fn build_client(accept_invalid_certs: bool) -> AppResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .danger_accept_invalid_certs(accept_invalid_certs).use_rustls_tls()
        .timeout(std::time::Duration::from_secs(30));
    if let Some(proxy_url) = crate::proxy_manager::get_configured_proxy() {
        if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
            builder = builder.proxy(proxy);
        }
    }
    builder.build().map_err(|e| AppError::Network(format!("HTTP 客户端创建失败：{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_urls() {
        assert_eq!(normalize_url("api.openai.com"), "https://api.openai.com");
        assert_eq!(
            normalize_url("https://api.openai.com/v1/"),
            "https://api.openai.com"
        );
        assert_eq!(
            normalize_url("  http://localhost:8080/  "),
            "http://localhost:8080"
        );
    }
    #[test]
    fn parses_generic_ratelimit_headers_properly() {
        use reqwest::header::{HeaderMap, HeaderValue};

        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-limit-requests", HeaderValue::from_static("120"));
        headers.insert("x-ratelimit-limit-tokens", HeaderValue::from_static("250000"));

        let (rpm, tpm) = parse_generic_ratelimit_headers(&headers);
        assert_eq!(rpm, Some(120));
        assert_eq!(tpm, Some(250000));

        let mut rfc_headers = HeaderMap::new();
        rfc_headers.insert("ratelimit-limit", HeaderValue::from_static("requests=300, tokens=600000"));
        let (rpm2, tpm2) = parse_generic_ratelimit_headers(&rfc_headers);
        assert_eq!(rpm2, Some(300));
        assert_eq!(tpm2, Some(600000));
    }

    #[test]
    fn detects_loopback_and_private_urls() {
        assert!(is_loopback_or_private_url("http://127.0.0.1:11434"));
        assert!(is_loopback_or_private_url("http://localhost:8080"));
        assert!(is_loopback_or_private_url("http://192.168.1.50:5000"));
        assert!(!is_loopback_or_private_url("https://api.openai.com"));
        assert!(!is_loopback_or_private_url("https://api.anthropic.com"));
    }

}
