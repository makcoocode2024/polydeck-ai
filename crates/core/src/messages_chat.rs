//! Anthropic Messages ↔ OpenAI Chat Completions conversion.
//!
//! Some OpenAI-compatible relays reject `/v1/messages` outright, even when they
//! serve the same model through `/v1/chat/completions`. Claude Code speaks the
//! Messages dialect, so the gateway translates both directions when the upstream
//! profile is OpenAI-protocol.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum MessagesAdapterError {
    #[error("Messages 请求缺少 model")]
    MissingModel,
    #[error("不支持的 Messages 输入：{0}")]
    UnsupportedInput(String),
    #[error("Chat 响应格式无效：{0}")]
    InvalidChatResponse(String),
}

#[derive(Debug, Clone)]
pub struct ConvertedMessagesRequest {
    pub body: Value,
    pub stream: bool,
    pub warnings: Vec<String>,
}

fn system_text(system: &Value) -> String {
    match system {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => String::new(),
    }
}

fn image_content_part(block: &Value, warnings: &mut Vec<String>) -> Option<Value> {
    let source = block.get("source")?;
    let source_type = source
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let url = match source_type {
        "base64" => {
            let media_type = source
                .get("media_type")
                .and_then(Value::as_str)
                .unwrap_or("image/png");
            let data = source
                .get("data")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if data.is_empty() {
                warnings.push("Messages 图片块缺少 base64 data，已忽略".into());
                return None;
            }
            format!("data:{media_type};base64,{data}")
        }
        "url" => source
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        other => {
            warnings.push(format!(
                "Messages 图片 source.type={other} 无法映射到 Chat Completions，已忽略"
            ));
            return None;
        }
    };
    if url.is_empty() {
        warnings.push("Messages 图片块缺少 URL，已忽略".into());
        return None;
    }
    Some(json!({
        "type": "image_url",
        "image_url": { "url": url }
    }))
}

fn tool_result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .map(|part| match part.get("type").and_then(Value::as_str) {
                Some("text") => part
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                Some("image") => "[图片结果已省略：Chat Completions 的 tool 消息不支持图片]".into(),
                _ => part.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn push_user_content(
    messages: &mut Vec<Value>,
    content: &Value,
    warnings: &mut Vec<String>,
) -> Result<(), MessagesAdapterError> {
    if let Some(text) = content.as_str() {
        messages.push(json!({ "role": "user", "content": text }));
        return Ok(());
    }
    let Some(blocks) = content.as_array() else {
        return Err(MessagesAdapterError::UnsupportedInput(
            "user message.content 不是字符串或数组".into(),
        ));
    };

    // Chat Completions requires tool messages to immediately follow the
    // assistant's tool_calls. Anthropic permits a tool_result and user text in
    // the same content array, so emit all tool results first.
    let mut chat_parts = Vec::new();
    for block in blocks {
        match block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "tool_result" => {
                let tool_use_id = block
                    .get("tool_use_id")
                    .and_then(Value::as_str)
                    .unwrap_or("call_unknown");
                let mut text = tool_result_text(block.get("content").unwrap_or(&Value::Null));
                if block
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    text = format!("Error: {text}");
                }
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": tool_use_id,
                    "content": text
                }));
            }
            "text" => {
                let text = block
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                chat_parts.push(json!({ "type": "text", "text": text }));
            }
            "image" => {
                if let Some(image) = image_content_part(block, warnings) {
                    chat_parts.push(image);
                }
            }
            "document" | "file" => {
                warnings.push("Messages 文档块无法映射到 Chat Completions，已忽略".into())
            }
            other => warnings.push(format!(
                "user content 类型 {other} 无法映射到 Chat Completions，已忽略"
            )),
        }
    }

    if chat_parts.is_empty() {
        return Ok(());
    }
    let all_text = chat_parts
        .iter()
        .all(|part| part.get("type").and_then(Value::as_str) == Some("text"));
    let content = if all_text {
        Value::String(
            chat_parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
    } else {
        Value::Array(chat_parts)
    };
    messages.push(json!({ "role": "user", "content": content }));
    Ok(())
}

fn push_assistant_content(
    messages: &mut Vec<Value>,
    content: &Value,
    warnings: &mut Vec<String>,
) -> Result<(), MessagesAdapterError> {
    if let Some(text) = content.as_str() {
        messages.push(json!({ "role": "assistant", "content": text }));
        return Ok(());
    }
    let Some(blocks) = content.as_array() else {
        return Err(MessagesAdapterError::UnsupportedInput(
            "assistant message.content 不是字符串或数组".into(),
        ));
    };

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    for block in blocks {
        match block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "text" => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    text_parts.push(text);
                }
            }
            "tool_use" => {
                let id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("call_ad_{}", Uuid::new_v4().simple()));
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unnamed_tool");
                let arguments = block
                    .get("input")
                    .cloned()
                    .unwrap_or_else(|| json!({}))
                    .to_string();
                tool_calls.push(json!({
                    "id": id,
                    "type": "function",
                    "function": { "name": name, "arguments": arguments }
                }));
            }
            // Signed Anthropic thinking cannot be replayed through Chat. The
            // bridge response never emits thinking blocks, so dropping these is
            // limited to history captured before the bridge was enabled.
            "thinking" | "redacted_thinking" => {}
            "image" => warnings.push("assistant 图片块无法映射到 Chat Completions，已忽略".into()),
            other => warnings.push(format!(
                "assistant content 类型 {other} 无法映射到 Chat Completions，已忽略"
            )),
        }
    }

    let text = text_parts.join("\n\n");
    if text.is_empty() && tool_calls.is_empty() {
        return Ok(());
    }
    let mut message = Map::new();
    message.insert("role".into(), Value::String("assistant".into()));
    message.insert(
        "content".into(),
        if text.is_empty() {
            Value::Null
        } else {
            Value::String(text)
        },
    );
    if !tool_calls.is_empty() {
        message.insert("tool_calls".into(), Value::Array(tool_calls));
    }
    messages.push(Value::Object(message));
    Ok(())
}

fn convert_tools(request: &Value, warnings: &mut Vec<String>) -> Vec<Value> {
    request
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let name = tool.get("name").and_then(Value::as_str).unwrap_or_default();
            if name.is_empty() {
                warnings.push("Messages 工具缺少 name，已忽略".into());
                return None;
            }
            let mut function = Map::new();
            function.insert("name".into(), Value::String(name.to_string()));
            if let Some(description) = tool.get("description").and_then(Value::as_str) {
                function.insert("description".into(), Value::String(description.to_string()));
            }
            function.insert(
                "parameters".into(),
                tool.get("input_schema")
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
            );
            if let Some(strict) = tool.get("strict").filter(|value| !value.is_null()) {
                function.insert("strict".into(), strict.clone());
            }
            Some(json!({ "type": "function", "function": function }))
        })
        .collect()
}

fn convert_tool_choice(choice: &Value) -> Value {
    if let Some(value) = choice.as_str() {
        return Value::String(value.to_string());
    }
    match choice
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
        "auto" => Value::String("auto".into()),
        "any" => Value::String("required".into()),
        "none" => Value::String("none".into()),
        "tool" => choice
            .get("name")
            .and_then(Value::as_str)
            .map(|name| json!({ "type": "function", "function": { "name": name } }))
            .unwrap_or_else(|| Value::String("auto".into())),
        _ => Value::String("auto".into()),
    }
}

fn budget_to_effort(budget: u64) -> Option<String> {
    match budget {
        0 => None,
        1..=5_999 => Some("low".into()),
        6_000..=11_999 => Some("medium".into()),
        12_000..=23_999 => Some("high".into()),
        24_000..=47_999 => Some("xhigh".into()),
        _ => Some("max".into()),
    }
}

fn normalize_message_effort(raw: &str, _model: &str, warnings: &mut Vec<String>) -> Option<String> {
    let clean = raw.trim().to_ascii_lowercase();
    let effort = match clean.as_str() {
        "minimal" => "low".to_string(),
        "none" | "off" | "0" | "false" => return None,
        // Every level travels verbatim; the upstream decides whether it takes it.
        "low" | "medium" | "high" | "xhigh" | "max" => clean,
        numeric => match numeric.parse::<u64>().ok().and_then(budget_to_effort) {
            Some(effort) => effort,
            None => {
                warnings.push(format!("未知 Messages reasoning effort={raw}，已忽略"));
                return None;
            }
        },
    };
    Some(effort)
}

fn effort_from_request(
    request: &Value,
    model: &str,
    fallback: Option<&str>,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let thinking = request.get("thinking");
    if thinking
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
        == Some("disabled")
    {
        return None;
    }

    if let Some(raw) = request
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .or_else(|| {
            thinking
                .and_then(|value| value.get("effort"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            request
                .pointer("/output_config/effort")
                .and_then(Value::as_str)
        })
    {
        return normalize_message_effort(raw, model, warnings);
    }

    if let Some(budget) = thinking
        .and_then(|value| value.get("budget_tokens"))
        .and_then(Value::as_u64)
    {
        return budget_to_effort(budget);
    }

    fallback.and_then(|effort| normalize_message_effort(effort, model, warnings))
}

pub fn messages_to_chat(
    request: &Value,
    fallback_effort: Option<&str>,
) -> Result<ConvertedMessagesRequest, MessagesAdapterError> {
    let model = request
        .get("model")
        .and_then(Value::as_str)
        .ok_or(MessagesAdapterError::MissingModel)?;
    let stream = request
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut warnings = Vec::new();
    let mut messages = Vec::new();

    if let Some(system) = request.get("system") {
        let text = system_text(system);
        if !text.is_empty() {
            messages.push(json!({ "role": "system", "content": text }));
        }
    }

    for message in request
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        let content = message
            .get("content")
            .cloned()
            .unwrap_or_else(|| Value::String(String::new()));
        match role {
            "assistant" => push_assistant_content(&mut messages, &content, &mut warnings)?,
            "system" => {
                let text = system_text(&content);
                if !text.is_empty() {
                    messages.push(json!({ "role": "system", "content": text }));
                }
            }
            _ => push_user_content(&mut messages, &content, &mut warnings)?,
        }
    }

    let mut body = Map::new();
    body.insert("model".into(), Value::String(model.to_string()));
    body.insert("messages".into(), Value::Array(messages));
    body.insert("stream".into(), Value::Bool(stream));
    if stream {
        body.insert("stream_options".into(), json!({ "include_usage": true }));
    }
    if let Some(value) = request.get("max_tokens") {
        body.insert("max_tokens".into(), value.clone());
    }
    for key in ["temperature", "top_p"] {
        if let Some(value) = request.get(key) {
            body.insert(key.into(), value.clone());
        }
    }
    if let Some(stop) = request
        .get("stop_sequences")
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty())
    {
        body.insert("stop".into(), Value::Array(stop.clone()));
    }

    let tools = convert_tools(request, &mut warnings);
    if !tools.is_empty() {
        body.insert("tools".into(), Value::Array(tools));
        if let Some(choice) = request.get("tool_choice") {
            body.insert("tool_choice".into(), convert_tool_choice(choice));
            if choice
                .get("disable_parallel_tool_use")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                body.insert("parallel_tool_calls".into(), Value::Bool(false));
            }
        }
    }
    if let Some(effort) = effort_from_request(request, model, fallback_effort, &mut warnings) {
        body.insert("reasoning_effort".into(), Value::String(effort));
    }

    Ok(ConvertedMessagesRequest {
        body: Value::Object(body),
        stream,
        warnings,
    })
}

fn chat_message_text(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn anthropic_usage(usage: &Value) -> Value {
    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "cache_creation_input_tokens": 0,
        "cache_read_input_tokens": usage
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0)
    })
}

fn tool_input(arguments: &str) -> Value {
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(value)) => Value::Object(value),
        Ok(Value::Null) | Err(_) => json!({}),
        Ok(value) => json!({ "value": value }),
    }
}

fn stop_reason(finish_reason: &str, has_tool_calls: bool) -> &'static str {
    match finish_reason {
        "length" => "max_tokens",
        "tool_calls" => "tool_use",
        "content_filter" => "end_turn",
        "stop" if has_tool_calls => "tool_use",
        _ if has_tool_calls => "tool_use",
        _ => "end_turn",
    }
}

pub fn chat_to_messages(chat: &Value, model: &str) -> Result<Value, MessagesAdapterError> {
    let choice = chat
        .pointer("/choices/0")
        .ok_or_else(|| MessagesAdapterError::InvalidChatResponse("缺少 choices[0]".into()))?;
    let message = choice.get("message").ok_or_else(|| {
        MessagesAdapterError::InvalidChatResponse("缺少 choices[0].message".into())
    })?;

    let mut content = Vec::new();
    let mut text = chat_message_text(message);
    if text.is_empty() {
        if let Some(reasoning) = message
            .get("reasoning_content")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            text = reasoning.to_string();
        }
    }
    if !text.is_empty() {
        content.push(json!({ "type": "text", "text": text }));
    }

    let mut has_tool_calls = false;
    for call in message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        has_tool_calls = true;
        let id = call
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("toolu_ad_{}", Uuid::new_v4().simple()));
        let name = call
            .pointer("/function/name")
            .and_then(Value::as_str)
            .unwrap_or("unnamed_tool");
        let arguments = call
            .pointer("/function/arguments")
            .and_then(Value::as_str)
            .unwrap_or("{}");
        content.push(json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": tool_input(arguments)
        }));
    }
    if content.is_empty() {
        content.push(json!({ "type": "text", "text": "" }));
    }

    let finish_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("stop");
    let message_id = chat
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| format!("msg_ad_{}", Uuid::new_v4().simple()));
    Ok(json!({
        "id": message_id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content,
        "stop_reason": stop_reason(finish_reason, has_tool_calls),
        "stop_sequence": Value::Null,
        "usage": anthropic_usage(chat.get("usage").unwrap_or(&Value::Null))
    }))
}

fn sse_event(event: &str, payload: Value) -> String {
    format!("event: {event}\ndata: {payload}\n\n")
}

#[derive(Debug, Default)]
struct StreamingTool {
    id: String,
    name: String,
    arguments: String,
    block_index: Option<u64>,
    emitted_arguments: usize,
    closed: bool,
}

pub struct MessagesStreamAdapter {
    message_id: String,
    model: String,
    next_index: u64,
    text_index: Option<u64>,
    text_open: bool,
    tools: BTreeMap<u64, StreamingTool>,
    usage: Value,
    finish_reason: Option<String>,
    saw_tool_calls: bool,
    finished: bool,
}

impl MessagesStreamAdapter {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            message_id: format!("msg_ad_{}", Uuid::new_v4().simple()),
            model: model.into(),
            next_index: 0,
            text_index: None,
            text_open: false,
            tools: BTreeMap::new(),
            usage: anthropic_usage(&Value::Null),
            finish_reason: None,
            saw_tool_calls: false,
            finished: false,
        }
    }

    pub fn start(&self) -> Vec<String> {
        vec![sse_event(
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": self.message_id,
                    "type": "message",
                    "role": "assistant",
                    "model": self.model,
                    "content": [],
                    "stop_reason": Value::Null,
                    "stop_sequence": Value::Null,
                    "usage": self.usage
                }
            }),
        )]
    }

    fn close_text_block(&mut self, events: &mut Vec<String>) {
        if !self.text_open {
            return;
        }
        if let Some(index) = self.text_index {
            events.push(sse_event(
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": index }),
            ));
        }
        self.text_open = false;
        self.text_index = None;
    }

    fn ensure_text_open(&mut self, events: &mut Vec<String>) -> u64 {
        if self.text_open {
            return self.text_index.unwrap_or(0);
        }
        let index = self.next_index;
        self.next_index += 1;
        self.text_index = Some(index);
        self.text_open = true;
        events.push(sse_event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": index,
                "content_block": { "type": "text", "text": "" }
            }),
        ));
        index
    }

    fn open_tool_block(&mut self, key: u64, events: &mut Vec<String>) {
        if self
            .tools
            .get(&key)
            .and_then(|tool| tool.block_index)
            .is_some()
        {
            return;
        }
        self.close_text_block(events);
        let index = self.next_index;
        self.next_index += 1;
        let Some(tool) = self.tools.get_mut(&key) else {
            return;
        };
        let id = if tool.id.is_empty() {
            format!("toolu_ad_{}", Uuid::new_v4().simple())
        } else {
            tool.id.clone()
        };
        let name = if tool.name.is_empty() {
            "unnamed_tool".to_string()
        } else {
            tool.name.clone()
        };
        tool.block_index = Some(index);
        tool.id = id.clone();
        events.push(sse_event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": {}
                }
            }),
        ));
        if !tool.arguments.is_empty() {
            let arguments = tool.arguments.clone();
            tool.emitted_arguments = arguments.len();
            events.push(sse_event(
                "content_block_delta",
                json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": { "type": "input_json_delta", "partial_json": arguments }
                }),
            ));
        }
    }

    fn push_tool_delta(&mut self, call: &Value, events: &mut Vec<String>) {
        let key = call.get("index").and_then(Value::as_u64).unwrap_or(0);
        let tool = self.tools.entry(key).or_default();
        if let Some(id) = call.get("id").and_then(Value::as_str) {
            if !id.is_empty() {
                tool.id = id.to_string();
            }
        }
        if let Some(name) = call
            .pointer("/function/name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
        {
            tool.name = name.to_string();
        }
        if let Some(arguments) = call.pointer("/function/arguments").and_then(Value::as_str) {
            tool.arguments.push_str(arguments);
        }
        let should_open =
            tool.block_index.is_none() && (!tool.name.is_empty() || !tool.arguments.is_empty());
        if should_open {
            self.open_tool_block(key, events);
        }
        if let Some(tool) = self.tools.get_mut(&key) {
            if let Some(index) = tool.block_index {
                if tool.emitted_arguments < tool.arguments.len() {
                    let fragment = tool.arguments[tool.emitted_arguments..].to_string();
                    tool.emitted_arguments = tool.arguments.len();
                    events.push(sse_event(
                        "content_block_delta",
                        json!({
                            "type": "content_block_delta",
                            "index": index,
                            "delta": { "type": "input_json_delta", "partial_json": fragment }
                        }),
                    ));
                }
            }
        }
    }

    pub fn push_chat_chunk(&mut self, chunk: &Value) -> Vec<String> {
        let mut events = Vec::new();
        if self.finished {
            return events;
        }
        if let Some(usage) = chunk.get("usage").filter(|value| !value.is_null()) {
            self.usage = anthropic_usage(usage);
        }
        if let Some(error) = chunk.get("error") {
            events.push(sse_event(
                "error",
                json!({ "type": "error", "error": error }),
            ));
            self.finish_reason = Some("end_turn".into());
            return events;
        }
        let Some(choice) = chunk.pointer("/choices/0") else {
            return events;
        };
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish_reason = Some(reason.to_string());
        }
        let Some(delta) = choice.get("delta") else {
            return events;
        };
        if let Some(text) = delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            let index = self.ensure_text_open(&mut events);
            events.push(sse_event(
                "content_block_delta",
                json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": { "type": "text_delta", "text": text }
                }),
            ));
        }
        for call in delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            self.saw_tool_calls = true;
            self.push_tool_delta(call, &mut events);
        }
        events
    }

    pub fn finish_with_reason(&mut self, override_reason: Option<&str>) -> Vec<String> {
        if self.finished {
            return Vec::new();
        }
        let mut events = Vec::new();

        // A tool call whose name never arrived still needs a block, or the client
        // receives argument JSON it cannot attach to any tool.
        let pending: Vec<u64> = self
            .tools
            .iter()
            .filter(|(_, tool)| tool.block_index.is_none())
            .map(|(key, _)| *key)
            .collect();
        for key in pending {
            self.open_tool_block(key, &mut events);
        }

        if self.text_index.is_none() && self.tools.is_empty() {
            self.ensure_text_open(&mut events);
        }
        self.close_text_block(&mut events);

        let mut open_blocks: Vec<(u64, u64)> = self
            .tools
            .iter()
            .filter_map(|(key, tool)| {
                tool.block_index
                    .filter(|_| !tool.closed)
                    .map(|index| (*key, index))
            })
            .collect();
        open_blocks.sort_by_key(|(_, index)| *index);
        for (key, index) in open_blocks {
            events.push(sse_event(
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": index }),
            ));
            if let Some(tool) = self.tools.get_mut(&key) {
                tool.closed = true;
            }
        }

        let stop_reason = override_reason.unwrap_or_else(|| {
            stop_reason(
                self.finish_reason.as_deref().unwrap_or("stop"),
                self.saw_tool_calls,
            )
        });
        events.push(sse_event(
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": { "stop_reason": stop_reason, "stop_sequence": Value::Null },
                "usage": self.usage
            }),
        ));
        events.push(sse_event("message_stop", json!({ "type": "message_stop" })));
        self.finished = true;
        events
    }

    pub fn finish(&mut self) -> Vec<String> {
        self.finish_with_reason(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_tools_history_and_extended_effort() {
        let request = json!({
            "model": "deepseek-v4.1-flash",
            "max_tokens": 8192,
            "system": [{ "type": "text", "text": "be useful" }],
            "thinking": { "type": "enabled", "budget_tokens": 63999 },
            "messages": [
                { "role": "user", "content": "run it" },
                { "role": "assistant", "content": [
                    { "type": "text", "text": "calling" },
                    { "type": "tool_use", "id": "toolu_1", "name": "exec", "input": { "cmd": "pwd" } }
                ]},
                { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "toolu_1", "content": "/tmp" },
                    { "type": "text", "text": "continue" }
                ]}
            ],
            "tools": [{ "name": "exec", "description": "run", "input_schema": { "type": "object" } }],
            "tool_choice": { "type": "any" },
            "stop_sequences": ["END"]
        });

        let converted = messages_to_chat(&request, None).unwrap();
        let body = &converted.body;
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][2]["tool_calls"][0]["id"], "toolu_1");
        assert_eq!(body["messages"][3]["role"], "tool");
        assert_eq!(body["messages"][3]["tool_call_id"], "toolu_1");
        assert_eq!(body["messages"][4]["content"], "continue");
        assert_eq!(body["tools"][0]["function"]["name"], "exec");
        assert_eq!(body["tool_choice"], "required");
        assert_eq!(body["stop"][0], "END");
        assert_eq!(body["reasoning_effort"], "max");
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn output_config_effort_reaches_chat() {
        let request = json!({"model":"glm-5.3", "messages":[], "output_config":{"effort":"max"}});
        let converted = messages_to_chat(&request, None).unwrap();
        assert_eq!(converted.body["reasoning_effort"], "max");
        assert!(converted.body.get("output_config").is_none());
    }

    #[test]
    fn passes_extended_effort_through_without_clamping() {
        let request = json!({
            "model": "plain-model",
            "max_tokens": 1024,
            "reasoning_effort": "max",
            "messages": [{ "role": "user", "content": "hi" }]
        });
        let converted = messages_to_chat(&request, None).unwrap();
        assert_eq!(
            converted.body["reasoning_effort"], "max",
            "深档必须原样透传，由上游决定接不接受"
        );
        assert!(
            !converted.warnings.iter().any(|w| w.contains("降级")),
            "透传路径不该产生降级警告"
        );
    }

    #[test]
    fn converts_chat_tool_calls_to_anthropic_blocks() {
        let chat = json!({
            "id": "chat_1",
            "model": "deepseek",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "need a tool",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": "exec", "arguments": "{\"cmd\":\"pwd\"}" }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
        });
        let response = chat_to_messages(&chat, "deepseek-v4.1-flash").unwrap();
        assert_eq!(response["model"], "deepseek-v4.1-flash");
        assert_eq!(response["stop_reason"], "tool_use");
        assert_eq!(response["content"][1]["type"], "tool_use");
        assert_eq!(response["content"][1]["input"]["cmd"], "pwd");
        assert_eq!(response["usage"]["input_tokens"], 10);
    }

    #[test]
    fn stream_adapter_emits_text_and_tool_blocks() {
        let mut adapter = MessagesStreamAdapter::new("deepseek-v4.1-flash");
        assert!(adapter.start()[0].contains("message_start"));
        let first = adapter.push_chat_chunk(&json!({
            "choices": [{
                "delta": {
                    "content": "hello",
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "function": { "name": "exec", "arguments": "{\"cmd\":" }
                    }]
                }
            }]
        }));
        assert!(first.iter().any(|event| event.contains("text_delta")));
        assert!(first.iter().any(|event| event.contains("tool_use")));
        assert!(first.iter().any(|event| event.contains("input_json_delta")));

        let second = adapter.push_chat_chunk(&json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "arguments": "\"pwd\"}" }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 7, "completion_tokens": 3 }
        }));
        assert!(second
            .iter()
            .any(|event| event.contains("input_json_delta")));
        let finish = adapter.finish();
        assert!(finish
            .iter()
            .any(|event| event.contains("content_block_stop")));
        assert!(finish
            .iter()
            .any(|event| event.contains("\"stop_reason\":\"tool_use\"")));
        assert!(finish.iter().any(|event| event.contains("message_stop")));
    }
}
