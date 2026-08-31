use std::collections::HashMap;

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("Responses 请求缺少 model")]
    MissingModel,
    #[error("不支持的 Responses 输入：{0}")]
    UnsupportedInput(String),
    #[error("Chat 响应格式无效：{0}")]
    InvalidChatResponse(String),
}

#[derive(Debug, Clone)]
enum ToolKind {
    Function,
    Custom,
}

#[derive(Debug, Clone)]
struct ToolMapping {
    original_name: String,
    kind: ToolKind,
}

#[derive(Debug, Clone, Default)]
pub struct ToolMap {
    by_chat_name: HashMap<String, ToolMapping>,
    by_original_name: HashMap<String, String>,
}

impl ToolMap {
    fn insert(&mut self, chat_name: String, original_name: String, kind: ToolKind) {
        self.by_original_name
            .insert(original_name.clone(), chat_name.clone());
        self.by_chat_name.insert(
            chat_name,
            ToolMapping {
                original_name,
                kind,
            },
        );
    }

    fn chat_name(&self, original_name: &str) -> String {
        self.by_original_name
            .get(original_name)
            .cloned()
            .unwrap_or_else(|| safe_tool_name(original_name, "fn"))
    }

    fn mapping(&self, chat_name: &str) -> ToolMapping {
        self.by_chat_name
            .get(chat_name)
            .cloned()
            .or_else(|| {
                self.by_original_name
                    .get(chat_name)
                    .and_then(|mapped| self.by_chat_name.get(mapped))
                    .cloned()
            })
            .unwrap_or_else(|| ToolMapping {
                original_name: chat_name.to_string(),
                kind: ToolKind::Function,
            })
    }
}

#[derive(Debug, Clone)]
pub struct ConvertedRequest {
    pub body: Value,
    pub tools: ToolMap,
    pub warnings: Vec<String>,
    pub stream: bool,
}

fn safe_tool_name(name: &str, prefix: &str) -> String {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'));
    if valid {
        return name.to_string();
    }
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    let digest = hex::encode(hasher.finalize());
    let cleaned: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    format!("ad_{prefix}_{}_{}", &digest[..8], cleaned)
        .chars()
        .take(64)
        .collect()
}

fn custom_chat_name(name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    let digest = hex::encode(hasher.finalize());
    let cleaned: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .take(32)
        .collect();
    format!("ad_custom_{}_{}", &digest[..10], cleaned)
        .chars()
        .take(64)
        .collect()
}

fn convert_tools(request: &Value, warnings: &mut Vec<String>) -> (Vec<Value>, ToolMap) {
    let mut converted = Vec::new();
    let mut mappings = ToolMap::default();
    for tool in request
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let tool_type = tool.get("type").and_then(Value::as_str).unwrap_or_default();
        let name = tool
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unnamed_tool");
        match tool_type {
            "function" => {
                let chat_name = safe_tool_name(name, "fn");
                let mut function = Map::new();
                function.insert("name".into(), Value::String(chat_name.clone()));
                function.insert(
                    "parameters".into(),
                    tool.get("parameters")
                        .cloned()
                        .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
                );
                if let Some(description) = tool.get("description").filter(|value| !value.is_null())
                {
                    function.insert("description".into(), description.clone());
                }
                if let Some(strict) = tool.get("strict").filter(|value| !value.is_null()) {
                    function.insert("strict".into(), strict.clone());
                }
                converted.push(json!({ "type": "function", "function": function }));
                mappings.insert(chat_name, name.to_string(), ToolKind::Function);
            }
            "custom" => {
                let chat_name = custom_chat_name(name);
                let description = tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("Freeform Codex tool");
                converted.push(json!({
                    "type": "function",
                    "function": {
                        "name": chat_name,
                        "description": format!("{description} Pass the exact freeform tool input in the input string."),
                        "parameters": {
                            "type": "object",
                            "properties": { "input": { "type": "string", "description": "Exact freeform input for the original tool" } },
                            "required": ["input"],
                            "additionalProperties": false
                        }
                    }
                }));
                mappings.insert(chat_name, name.to_string(), ToolKind::Custom);
                warnings.push(format!("custom 工具 {name} 已包装为标准 function"));
            }
            "namespace" => warnings.push(format!(
                "namespace 工具 {name} 无法可靠映射到 Chat Completions，已过滤"
            )),
            other => warnings.push(format!(
                "Responses 工具类型 {other} 无法由 Chat 后端执行，已过滤"
            )),
        }
    }
    (converted, mappings)
}

fn convert_content(content: &Value) -> Result<Value, AdapterError> {
    if let Some(text) = content.as_str() {
        return Ok(Value::String(text.to_string()));
    }
    let Some(parts) = content.as_array() else {
        return Err(AdapterError::UnsupportedInput(
            "message.content 不是字符串或数组".into(),
        ));
    };
    let mut converted = Vec::new();
    for part in parts {
        match part.get("type").and_then(Value::as_str).unwrap_or_default() {
            "input_text" | "output_text" | "text" => converted.push(json!({
                "type": "text",
                "text": part.get("text").and_then(Value::as_str).unwrap_or_default()
            })),
            "input_image" => {
                let Some(url) = part.get("image_url").and_then(Value::as_str) else {
                    return Err(AdapterError::UnsupportedInput(
                        "input_image 缺少 image_url".into(),
                    ));
                };
                converted.push(json!({
                    "type": "image_url",
                    "image_url": {
                        "url": url,
                        "detail": part.get("detail").and_then(Value::as_str).unwrap_or("auto")
                    }
                }));
            }
            "input_file" => {
                return Err(AdapterError::UnsupportedInput(
                    "Chat Completions 无法无损承载 Responses input_file".into(),
                ))
            }
            other => {
                return Err(AdapterError::UnsupportedInput(format!(
                    "未知 content 类型 {other}"
                )))
            }
        }
    }
    Ok(Value::Array(converted))
}

fn push_input_item(
    messages: &mut Vec<Value>,
    item: &Value,
    tools: &ToolMap,
) -> Result<(), AdapterError> {
    if let Some(text) = item.as_str() {
        messages.push(json!({ "role": "user", "content": text }));
        return Ok(());
    }
    let item_type = item
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("message");
    match item_type {
        "message" => {
            let role = match item.get("role").and_then(Value::as_str).unwrap_or("user") {
                "developer" | "system" => "system",
                "assistant" => "assistant",
                _ => "user",
            };
            let content = item
                .get("content")
                .cloned()
                .unwrap_or_else(|| Value::String(String::new()));
            messages.push(json!({ "role": role, "content": convert_content(&content)? }));
        }
        "function_call" => {
            let original_name = item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unnamed_tool");
            let call_id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("call_unknown");
            messages.push(json!({
                "role": "assistant",
                "content": Value::Null,
                "tool_calls": [{
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": tools.chat_name(original_name),
                        "arguments": item.get("arguments").and_then(Value::as_str).unwrap_or("{}")
                    }
                }]
            }));
        }
        "custom_tool_call" => {
            let original_name = item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unnamed_tool");
            let call_id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("call_unknown");
            let input = item
                .get("input")
                .and_then(Value::as_str)
                .unwrap_or_default();
            messages.push(json!({
                "role": "assistant",
                "content": Value::Null,
                "tool_calls": [{
                    "id": call_id,
                    "type": "function",
                    "function": { "name": tools.chat_name(original_name), "arguments": json!({ "input": input }).to_string() }
                }]
            }));
        }
        "function_call_output" | "custom_tool_call_output" => {
            let call_id = item
                .get("call_id")
                .and_then(Value::as_str)
                .unwrap_or("call_unknown");
            let output = item
                .get("output")
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string())
                })
                .unwrap_or_default();
            messages.push(json!({ "role": "tool", "tool_call_id": call_id, "content": output }));
        }
        "reasoning" => {}
        other => {
            return Err(AdapterError::UnsupportedInput(format!(
                "未知 input item 类型 {other}"
            )))
        }
    }
    Ok(())
}

fn convert_tool_choice(choice: &Value, tools: &ToolMap) -> Value {
    if let Some(value) = choice.as_str() {
        return Value::String(value.to_string());
    }
    let choice_type = choice
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let original_name = choice
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if matches!(choice_type, "function" | "custom") && !original_name.is_empty() {
        return json!({ "type": "function", "function": { "name": tools.chat_name(original_name) } });
    }
    Value::String("auto".into())
}

pub fn responses_to_chat(
    request: &Value,
    previous_messages: Option<&[Value]>,
) -> Result<ConvertedRequest, AdapterError> {
    let model = request
        .get("model")
        .and_then(Value::as_str)
        .ok_or(AdapterError::MissingModel)?;
    let stream = request
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut warnings = Vec::new();
    let (converted_tools, tools) = convert_tools(request, &mut warnings);
    let mut messages = previous_messages.map(ToOwned::to_owned).unwrap_or_default();
    if let Some(instructions) = request.get("instructions").and_then(Value::as_str) {
        messages.push(json!({ "role": "system", "content": instructions }));
    }
    match request.get("input") {
        Some(Value::String(text)) => messages.push(json!({ "role": "user", "content": text })),
        Some(Value::Array(items)) => {
            for item in items {
                push_input_item(&mut messages, item, &tools)?;
            }
        }
        Some(item @ Value::Object(_)) => push_input_item(&mut messages, item, &tools)?,
        Some(_) => {
            return Err(AdapterError::UnsupportedInput(
                "input 不是字符串、对象或数组".into(),
            ))
        }
        None => messages.push(json!({ "role": "user", "content": "" })),
    }

    let mut body = Map::new();
    body.insert("model".into(), Value::String(model.into()));
    body.insert("messages".into(), Value::Array(messages));
    body.insert("stream".into(), Value::Bool(stream));
    if !converted_tools.is_empty() {
        body.insert("tools".into(), Value::Array(converted_tools));
    }
    if let Some(choice) = request.get("tool_choice") {
        body.insert("tool_choice".into(), convert_tool_choice(choice, &tools));
    }
    if let Some(value) = request.get("max_output_tokens") {
        body.insert("max_tokens".into(), value.clone());
    }
    for key in [
        "temperature",
        "top_p",
        "parallel_tool_calls",
        "seed",
        "stop",
    ] {
        if let Some(value) = request.get(key) {
            body.insert(key.into(), value.clone());
        }
    }
    if let Some(effort) = request.pointer("/reasoning/effort").and_then(Value::as_str) {
        let model_lower = model.to_ascii_lowercase();
        let effort_clean = effort.trim().to_ascii_lowercase();
        // Auto-thinking DeepSeek reasoner variants and QwQ drop the field; the
        // plain DeepSeek chat models accept low/medium/high/xhigh/max.
        if model_lower.contains("reasoner")
            || model_lower.contains("r1")
            || model_lower.contains("qwq")
        {
            warnings.push(format!(
                "模型 {model} 自动执行思考推理，已安全忽略 reasoning_effort 参数以避免上游报错"
            ));
        } else if model_lower.contains("gemini") {
            if effort_clean == "minimal" {
                body.insert("reasoning_effort".into(), Value::String("low".into()));
                warnings.push("Gemini 不支持 minimal 推理档位，已自动平滑提升为 low".into());
            } else if matches!(effort_clean.as_str(), "xhigh" | "max") {
                body.insert("reasoning_effort".into(), Value::String("high".into()));
                warnings.push(format!(
                    "Gemini 不支持 {effort} 推理档位，已自动降级为 high"
                ));
            } else if matches!(effort_clean.as_str(), "low" | "medium" | "high") {
                body.insert("reasoning_effort".into(), Value::String(effort_clean));
            } else {
                warnings.push(format!("Gemini 忽略未知/关闭推理档位 effort={effort}"));
            }
        } else {
            // DeepSeek non-reasoner models accept the full low..max range, so
            // pass every whitelisted level through instead of clamping to high.
            let is_deepseek = model_lower.contains("deepseek");
            if effort_clean == "minimal" {
                body.insert("reasoning_effort".into(), Value::String("low".into()));
                warnings.push("Chat 后端不支持 minimal 推理档位，已自动平滑映射为 low".into());
            // Levels this upstream takes verbatim: low..high everywhere, plus
            // xhigh/max on DeepSeek, which accepts the extended range. Kept as
            // one arm because the action is the same; the `xhigh`/`max` arm
            // below then only sees upstreams that need the downgrade.
            } else if matches!(effort_clean.as_str(), "low" | "medium" | "high")
                || (is_deepseek && matches!(effort_clean.as_str(), "xhigh" | "max"))
            {
                body.insert("reasoning_effort".into(), Value::String(effort_clean));
            } else if matches!(effort_clean.as_str(), "xhigh" | "max") {
                body.insert("reasoning_effort".into(), Value::String("high".into()));
                warnings.push(format!(
                    "Chat 后端不支持 Responses reasoning effort={effort}，已降级为 high"
                ));
            } else if effort_clean == "none" {
                // none: do not inject
            } else {
                warnings.push(format!(
                    "Chat 后端不支持 Responses reasoning effort={effort}，已省略该字段"
                ));
            }
        }
    }
    if let Some(format) = request.pointer("/text/format") {
        let response_format = if format.get("type").and_then(Value::as_str) == Some("json_schema") {
            json!({ "type": "json_schema", "json_schema": {
                "name": format.get("name").and_then(Value::as_str).unwrap_or("response"),
                "schema": format.get("schema").cloned().unwrap_or_else(|| json!({})),
                "strict": format.get("strict").cloned().unwrap_or(Value::Bool(false))
            }})
        } else {
            format.clone()
        };
        body.insert("response_format".into(), response_format);
    }
    Ok(ConvertedRequest {
        body: Value::Object(body),
        tools,
        warnings,
        stream,
    })
}

fn extract_custom_input(arguments: &str) -> String {
    serde_json::from_str::<Value>(arguments)
        .ok()
        .and_then(|value| {
            value
                .get("input")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| arguments.to_string())
}

fn usage_from_chat(chat: &Value) -> Value {
    let usage = chat.get("usage").cloned().unwrap_or_else(|| json!({}));
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
        "input_tokens_details": { "cached_tokens": usage.pointer("/prompt_tokens_details/cached_tokens").and_then(Value::as_u64).unwrap_or(0) },
        "output_tokens": output_tokens,
        "output_tokens_details": { "reasoning_tokens": usage.pointer("/completion_tokens_details/reasoning_tokens").and_then(Value::as_u64).unwrap_or(0) },
        "total_tokens": usage.get("total_tokens").and_then(Value::as_u64).unwrap_or(input_tokens + output_tokens)
    })
}

pub fn chat_to_response(chat: &Value, tools: &ToolMap) -> Result<Value, AdapterError> {
    let choice = chat
        .pointer("/choices/0")
        .ok_or_else(|| AdapterError::InvalidChatResponse("缺少 choices[0]".into()))?;
    let message = choice
        .get("message")
        .ok_or_else(|| AdapterError::InvalidChatResponse("缺少 choices[0].message".into()))?;
    let response_id = format!("resp_ad_{}", Uuid::new_v4().simple());
    let created_at = chat
        .get("created")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| chrono::Utc::now().timestamp() as u64);
    let mut output = Vec::new();
    // Reasoning models on a Chat backend return their thinking in the
    // non-standard `reasoning_content` field (DeepSeek's convention, also used by
    // Agnes and QwQ).
    //
    // Folded into the message text rather than emitted as a `reasoning` item, for
    // the same reason as the streaming path: Codex sends
    // include:["reasoning.encrypted_content"] and discards reasoning items that
    // lack that blob, so an item here is dropped and the thinking never reaches
    // the replayed history. Keeping both paths on the same shape also means a
    // client cannot see the thinking appear or vanish depending on `stream`.
    let reasoning = message
        .get("reasoning_content")
        .and_then(Value::as_str)
        .filter(|reasoning| !reasoning.is_empty())
        .map(|reasoning| {
            format!(
                "{REASONING_OPEN}\n{}\n{REASONING_CLOSE}\n\n",
                reasoning.trim()
            )
        });
    let content = message
        .get("content")
        .and_then(Value::as_str)
        .filter(|content| !content.is_empty());
    if reasoning.is_some() || content.is_some() {
        let text = format!(
            "{}{}",
            reasoning.unwrap_or_default(),
            content.unwrap_or_default()
        );
        output.push(json!({
            "id": format!("msg_ad_{}", Uuid::new_v4().simple()),
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{ "type": "output_text", "annotations": [], "logprobs": [], "text": text }]
        }));
    }
    for call in message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let call_id = call
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("call_ad_{}", Uuid::new_v4().simple()));
        let chat_name = call
            .pointer("/function/name")
            .and_then(Value::as_str)
            .unwrap_or("unnamed_tool");
        let arguments = call
            .pointer("/function/arguments")
            .and_then(Value::as_str)
            .unwrap_or("{}");
        let mapping = tools.mapping(chat_name);
        let item = match mapping.kind {
            ToolKind::Function => json!({
                "id": format!("fc_ad_{}", Uuid::new_v4().simple()), "type": "function_call", "status": "completed",
                "call_id": call_id, "name": mapping.original_name, "arguments": arguments
            }),
            ToolKind::Custom => json!({
                "id": format!("ctc_ad_{}", Uuid::new_v4().simple()), "type": "custom_tool_call", "status": "completed",
                "call_id": call_id, "name": mapping.original_name, "input": extract_custom_input(arguments)
            }),
        };
        output.push(item);
    }
    let finish_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("stop");
    let completed = !matches!(finish_reason, "length" | "content_filter");
    Ok(json!({
        "id": response_id,
        "object": "response",
        "created_at": created_at,
        "status": if completed { "completed" } else { "incomplete" },
        "completed_at": if completed { Value::from(created_at) } else { Value::Null },
        "error": Value::Null,
        "incomplete_details": if completed { Value::Null } else { json!({ "reason": if finish_reason == "length" { "max_output_tokens" } else { finish_reason } }) },
        "model": chat.get("model").cloned().unwrap_or(Value::Null),
        "output": output,
        "parallel_tool_calls": true,
        "tool_choice": "auto",
        "tools": [],
        "usage": usage_from_chat(chat)
    }))
}

fn sse_event(event_type: &str, payload: Value) -> String {
    format!("event: {event_type}\ndata: {}\n\n", payload)
}

#[derive(Debug, Clone)]
struct StreamingTool {
    output_index: usize,
    item_id: String,
    call_id: String,
    chat_name: String,
    arguments: String,
    started: bool,
}

/// Delimiters wrapping folded-in reasoning inside assistant text.
///
/// Chosen to be recognisable to both a reader and the model on replay, and
/// unlikely to collide with ordinary output.
const REASONING_OPEN: &str = "<!-- thinking -->";
const REASONING_CLOSE: &str = "<!-- /thinking -->";

pub struct StreamAdapter {
    response_id: String,
    model: String,
    tools: ToolMap,
    text_item_id: Option<String>,
    text_output_index: Option<usize>,
    text: String,
    /// Accumulated `delta.reasoning_content`, awaiting a flush into the message.
    ///
    /// Reasoning models stream their thinking in this non-standard field
    /// (DeepSeek's convention, also Agnes and QwQ). It is buffered rather than
    /// forwarded as a `reasoning` item — see `flush_reasoning_into_text`.
    reasoning: String,
    next_output_index: usize,
    tool_calls: HashMap<usize, StreamingTool>,
    usage: Value,
    /// Why the upstream stream ended early, if it did. Appended to the assistant
    /// text by `finish`, because an `event: error` frame does not reach the user:
    /// measured 2026-09-01, a severed bridge stream delivered 3856 characters and
    /// the client stored no trace of the error frame sent alongside them, so the
    /// output simply stopped mid-sentence with nothing saying why.
    truncated: Option<String>,
}

impl StreamAdapter {
    pub fn new(model: String, tools: ToolMap) -> Self {
        Self {
            response_id: format!("resp_ad_{}", Uuid::new_v4().simple()),
            model,
            tools,
            text_item_id: None,
            text_output_index: None,
            text: String::new(),
            reasoning: String::new(),
            next_output_index: 0,
            tool_calls: HashMap::new(),
            usage: json!({ "input_tokens": 0, "output_tokens": 0, "total_tokens": 0 }),
            truncated: None,
        }
    }

    /// Record that the upstream stream ended early, so `finish` can say so in the
    /// text. `reason` is short and user-facing; it lands in the transcript.
    pub fn mark_truncated(&mut self, reason: impl Into<String>) {
        self.truncated = Some(reason.into());
    }

    pub fn start(&self) -> Vec<String> {
        let response = json!({ "id": self.response_id, "object": "response", "status": "in_progress", "model": self.model, "output": [] });
        vec![sse_event(
            "response.created",
            json!({ "type": "response.created", "sequence_number": 0, "response": response }),
        )]
    }

    /// Open the assistant message item if it does not exist yet, then stream
    /// `text` into it.
    fn push_text(&mut self, text: &str) -> Vec<String> {
        let mut events = Vec::new();
        self.text.push_str(text);
        if self.text_item_id.is_none() {
            let item_id = format!("msg_ad_{}", Uuid::new_v4().simple());
            let output_index = self.next_output_index;
            self.next_output_index += 1;
            self.text_item_id = Some(item_id.clone());
            self.text_output_index = Some(output_index);
            events.push(sse_event("response.output_item.added", json!({
                "type": "response.output_item.added", "output_index": output_index,
                "item": { "id": item_id, "type": "message", "status": "in_progress", "role": "assistant", "content": [] }
            })));
            events.push(sse_event("response.content_part.added", json!({
                "type": "response.content_part.added", "output_index": output_index, "content_index": 0,
                "item_id": self.text_item_id, "part": { "type": "output_text", "annotations": [], "text": "" }
            })));
        }
        events.push(sse_event("response.output_text.delta", json!({
            "type": "response.output_text.delta", "output_index": self.text_output_index, "content_index": 0,
            "item_id": self.text_item_id, "delta": text, "logprobs": []
        })));
        events
    }

    /// Move buffered `reasoning_content` into the assistant message text.
    ///
    /// Emitting it as a proper `reasoning` output item was tried first and does
    /// not survive: Codex sends `include: ["reasoning.encrypted_content"]` and
    /// discards reasoning items that arrive without `encrypted_content`, which is
    /// a blob signed with an OpenAI server key that a relay cannot produce.
    /// Measured over a 12-turn session — the gateway emitted 39 reasoning deltas
    /// and Codex stored `reasoning: 0`.
    ///
    /// Dropping it is not free either. The client then persists an assistant
    /// message holding only the `"\n\n"` that preceded the thinking and replays
    /// that as history, so the model is handed its own turns with the reasoning
    /// removed and re-derives it every time. By turn 12 that consumed the entire
    /// output budget — 131 of 133 tokens — leaving nothing for an answer or a tool
    /// call, which is the stall this fixes.
    ///
    /// Folding it into the message text keeps it in history, at the cost of the
    /// thinking being visible in the transcript. Marked so the model can tell its
    /// own prior reasoning from its answers when the turn is replayed.
    fn flush_reasoning_into_text(&mut self) -> Vec<String> {
        if self.reasoning.is_empty() {
            return Vec::new();
        }
        let reasoning = std::mem::take(&mut self.reasoning);
        let block = format!(
            "{REASONING_OPEN}\n{}\n{REASONING_CLOSE}\n\n",
            reasoning.trim()
        );
        self.push_text(&block)
    }

    pub fn push_chat_chunk(&mut self, chunk: &Value) -> Vec<String> {
        let mut events = Vec::new();
        if let Some(usage) = chunk.get("usage").filter(|value| !value.is_null()) {
            self.usage = usage_from_chat(&json!({ "usage": usage }));
        }
        let Some(choice) = chunk.pointer("/choices/0") else {
            return events;
        };
        let delta = choice.get("delta").cloned().unwrap_or_else(|| json!({}));
        // Reasoning is buffered here and folded into the assistant message by
        // `flush_reasoning_into_text`, rather than emitted as a `reasoning` item.
        if let Some(reasoning) = delta
            .get("reasoning_content")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            self.reasoning.push_str(reasoning);
        }
        if let Some(content) = delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            // Reasoning arrives before content, so flush it first and the thinking
            // reads ahead of the answer it produced.
            events.extend(self.flush_reasoning_into_text());
            events.extend(self.push_text(content));
        }
        for call in delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let chat_name = call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let arguments_delta = call
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let tool = self.tool_calls.entry(index).or_insert_with(|| {
                let output_index = self.next_output_index;
                self.next_output_index += 1;
                StreamingTool {
                    output_index,
                    item_id: format!("tool_ad_{}", Uuid::new_v4().simple()),
                    call_id: call
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("call_ad_{}", Uuid::new_v4().simple())),
                    chat_name: chat_name.to_string(),
                    arguments: String::new(),
                    started: false,
                }
            });
            if !chat_name.is_empty() {
                tool.chat_name = chat_name.to_string();
            }
            tool.arguments.push_str(arguments_delta);
            let mapping = self.tools.mapping(&tool.chat_name);
            if !tool.started {
                tool.started = true;
                let item = match mapping.kind {
                    ToolKind::Function => {
                        json!({ "id": tool.item_id, "type": "function_call", "status": "in_progress", "call_id": tool.call_id, "name": mapping.original_name, "arguments": "" })
                    }
                    ToolKind::Custom => {
                        json!({ "id": tool.item_id, "type": "custom_tool_call", "status": "in_progress", "call_id": tool.call_id, "name": mapping.original_name, "input": "" })
                    }
                };
                events.push(sse_event("response.output_item.added", json!({ "type": "response.output_item.added", "output_index": tool.output_index, "item": item })));
            }
            if matches!(mapping.kind, ToolKind::Function) && !arguments_delta.is_empty() {
                events.push(sse_event("response.function_call_arguments.delta", json!({
                    "type": "response.function_call_arguments.delta", "output_index": tool.output_index,
                    "item_id": tool.item_id, "delta": arguments_delta
                })));
            }
        }
        events
    }

    pub fn conversation_snapshot(&self) -> (String, Value) {
        let tool_calls = self
            .tool_calls
            .values()
            .map(|call| {
                json!({
                    "id": call.call_id,
                    "type": "function",
                    "function": { "name": call.chat_name, "arguments": call.arguments }
                })
            })
            .collect::<Vec<_>>();
        let mut message = Map::new();
        message.insert("role".into(), Value::String("assistant".into()));
        message.insert(
            "content".into(),
            if self.text.is_empty() {
                Value::Null
            } else {
                Value::String(self.text.clone())
            },
        );
        if !tool_calls.is_empty() {
            message.insert("tool_calls".into(), Value::Array(tool_calls));
        }
        (self.response_id.clone(), Value::Object(message))
    }

    pub fn finish(mut self) -> Vec<String> {
        let mut events = Vec::new();
        let mut indexed_output = Vec::new();
        // A turn can end with reasoning and no content at all — that is exactly the
        // stalling case, where the whole output budget went to thinking. Flushing
        // here means the client still receives a message instead of nothing.
        events.extend(self.flush_reasoning_into_text());
        // After the flush, so the notice reads as the last thing in the turn rather
        // than sitting in front of the thinking it interrupted.
        if let Some(reason) = self.truncated.take() {
            let notice = format!("\n\n⚠ 上游在此处断流，本轮输出不完整（{reason}）。");
            events.extend(self.push_text(&notice));
        }
        if let (Some(item_id), Some(output_index)) =
            (self.text_item_id.take(), self.text_output_index)
        {
            let text = self.text.clone();
            events.push(sse_event("response.output_text.done", json!({
                "type": "response.output_text.done", "output_index": output_index, "content_index": 0,
                "item_id": item_id, "text": text, "logprobs": []
            })));
            events.push(sse_event("response.content_part.done", json!({
                "type": "response.content_part.done", "output_index": output_index, "content_index": 0,
                "item_id": item_id, "part": { "type": "output_text", "annotations": [], "text": text }
            })));
            let item = json!({
                "id": item_id, "type": "message", "status": "completed", "role": "assistant",
                "content": [{ "type": "output_text", "annotations": [], "logprobs": [], "text": text }]
            });
            events.push(sse_event("response.output_item.done", json!({ "type": "response.output_item.done", "output_index": output_index, "item": item })));
            indexed_output.push((output_index, item));
        }
        let mut calls: Vec<_> = self.tool_calls.into_values().collect();
        calls.sort_by_key(|call| call.output_index);
        for call in calls {
            let mapping = self.tools.mapping(&call.chat_name);
            let item = match mapping.kind {
                ToolKind::Function => {
                    events.push(sse_event("response.function_call_arguments.done", json!({
                        "type": "response.function_call_arguments.done", "output_index": call.output_index,
                        "item_id": call.item_id, "arguments": call.arguments
                    })));
                    json!({ "id": call.item_id, "type": "function_call", "status": "completed", "call_id": call.call_id, "name": mapping.original_name, "arguments": call.arguments })
                }
                ToolKind::Custom => {
                    let input = extract_custom_input(&call.arguments);
                    events.push(sse_event("response.custom_tool_call_input.delta", json!({
                        "type": "response.custom_tool_call_input.delta", "output_index": call.output_index,
                        "item_id": call.item_id, "delta": input
                    })));
                    events.push(sse_event("response.custom_tool_call_input.done", json!({
                        "type": "response.custom_tool_call_input.done", "output_index": call.output_index,
                        "item_id": call.item_id, "input": input
                    })));
                    json!({ "id": call.item_id, "type": "custom_tool_call", "status": "completed", "call_id": call.call_id, "name": mapping.original_name, "input": input })
                }
            };
            events.push(sse_event("response.output_item.done", json!({ "type": "response.output_item.done", "output_index": call.output_index, "item": item })));
            indexed_output.push((call.output_index, item));
        }
        indexed_output.sort_by_key(|(index, _)| *index);
        let output = indexed_output
            .into_iter()
            .map(|(_, item)| item)
            .collect::<Vec<_>>();
        let response = json!({
            "id": self.response_id, "object": "response", "status": "completed", "model": self.model,
            "output": output, "usage": self.usage, "error": Value::Null, "incomplete_details": Value::Null
        });
        events.push(sse_event(
            "response.completed",
            json!({ "type": "response.completed", "response": response }),
        ));
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_custom_tools_to_functions_and_back() {
        let request = json!({
            "model": "third-party-model",
            "input": "update the file",
            "tools": [{ "type": "custom", "name": "apply_patch", "description": "Apply patch", "format": { "type": "text" } }]
        });
        let converted = responses_to_chat(&request, None).unwrap();
        assert_eq!(converted.body["model"], "third-party-model");
        assert_eq!(converted.body["tools"][0]["type"], "function");
        let chat_name = converted.body["tools"][0]["function"]["name"]
            .as_str()
            .unwrap();
        let chat = json!({
            "id": "chatcmpl-test", "created": 1, "model": "third-party-model",
            "choices": [{ "finish_reason": "tool_calls", "message": { "role": "assistant", "content": null, "tool_calls": [{
                "id": "call_1", "type": "function", "function": { "name": chat_name, "arguments": "{\"input\":\"*** Begin Patch\"}" }
            }] } }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
        });
        let response = chat_to_response(&chat, &converted.tools).unwrap();
        assert_eq!(response["output"][0]["type"], "custom_tool_call");
        assert_eq!(response["output"][0]["name"], "apply_patch");
        assert_eq!(response["output"][0]["input"], "*** Begin Patch");
    }

    #[test]
    fn maps_messages_function_outputs_and_dynamic_model() {
        let request = json!({
            "model": "agnes-dynamic",
            "instructions": "be concise",
            "input": [
                { "role": "user", "content": [{ "type": "input_text", "text": "hello" }] },
                { "type": "function_call", "call_id": "call_2", "name": "read_file", "arguments": "{\"path\":\"a.txt\"}" },
                { "type": "function_call_output", "call_id": "call_2", "output": "data" }
            ],
            "tools": [{ "type": "function", "name": "read_file", "parameters": { "type": "object" } }]
        });
        let converted = responses_to_chat(&request, None).unwrap();
        assert_eq!(converted.body["model"], "agnes-dynamic");
        assert_eq!(converted.body["messages"][0]["role"], "system");
        assert_eq!(converted.body["messages"][3]["role"], "tool");
    }

    #[test]
    fn filters_namespace_with_explicit_warning() {
        let request = json!({
            "model": "m", "input": "x",
            "tools": [{ "type": "namespace", "name": "code_mode", "tools": [] }]
        });
        let converted = responses_to_chat(&request, None).unwrap();
        assert!(converted.body.get("tools").is_none());
        assert!(converted
            .warnings
            .iter()
            .any(|warning| warning.contains("namespace")));
    }

    #[test]
    fn downgrades_extended_reasoning_effort_for_chat_backends() {
        let request = json!({
            "model": "dynamic-model", "input": "x", "reasoning": { "effort": "xhigh" }
        });
        let converted = responses_to_chat(&request, None).unwrap();
        assert_eq!(converted.body["reasoning_effort"], "high");
        assert!(converted
            .warnings
            .iter()
            .any(|warning| warning.contains("xhigh")));
    }

    #[test]
    fn folds_reasoning_into_the_message_on_the_nonstream_path() {
        // Same shape as the streaming path: a `reasoning` item would be discarded
        // by a client sending include:["reasoning.encrypted_content"], and having
        // the two paths differ would make the thinking appear or vanish depending
        // on whether `stream` was set.
        let chat = json!({
            "model": "agnes-2.5-pro",
            "choices": [{
                "finish_reason": "stop",
                "message": {
                    "role": "assistant",
                    "content": "9 remain.",
                    "reasoning_content": "All but 9 ran away, so 9 stayed."
                }
            }]
        });
        let converted = chat_to_response(&chat, &ToolMap::default()).unwrap();
        let output = converted["output"].as_array().unwrap();
        assert_eq!(
            output.len(),
            1,
            "one message, not a separate reasoning item"
        );
        assert_eq!(output[0]["type"], "message");
        let text = output[0]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("All but 9 ran away, so 9 stayed."));
        assert!(text.contains("9 remain."));
        assert!(text.contains(REASONING_OPEN) && text.contains(REASONING_CLOSE));
        // Thinking before answer.
        assert!(text.find("All but 9").unwrap() < text.find("9 remain.").unwrap());
    }

    #[test]
    fn reasoning_only_reply_still_yields_output() {
        // Agnes reasoning models bill thinking against `max_tokens`, so a tight
        // budget returns reasoning with empty content. That used to produce
        // `output: []` — a valid but empty response the client could not show.
        let chat = json!({
            "model": "agnes-2.5-pro",
            "choices": [{
                "finish_reason": "length",
                "message": {
                    "role": "assistant",
                    "content": "",
                    "reasoning_content": "Let me work through this"
                }
            }]
        });
        let converted = chat_to_response(&chat, &ToolMap::default()).unwrap();
        let output = converted["output"].as_array().unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["type"], "message");
        assert!(output[0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Let me work through this"));
        assert_eq!(converted["status"], "incomplete");
        assert_eq!(
            converted["incomplete_details"]["reason"],
            "max_output_tokens"
        );
    }

    #[test]
    fn nonstream_reply_without_reasoning_gains_no_delimiters() {
        let chat = json!({
            "model": "gpt-4o",
            "choices": [{ "finish_reason": "stop",
                "message": { "role": "assistant", "content": "plain answer" } }]
        });
        let converted = chat_to_response(&chat, &ToolMap::default()).unwrap();
        let text = converted["output"][0]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert_eq!(text, "plain answer");
        assert!(!text.contains(REASONING_OPEN));
    }

    #[test]
    fn omits_reasoning_item_when_upstream_sends_none() {
        let chat = json!({
            "model": "agnes-2.5-flash",
            "choices": [{
                "finish_reason": "stop",
                "message": { "role": "assistant", "content": "9 remain." }
            }]
        });
        let converted = chat_to_response(&chat, &ToolMap::default()).unwrap();
        let output = converted["output"].as_array().unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["type"], "message");
    }

    #[test]
    fn streams_text_and_completion_events() {
        let request = json!({ "model": "dynamic-model", "input": "hello" });
        let converted = responses_to_chat(&request, None).unwrap();
        let mut adapter = StreamAdapter::new("dynamic-model".into(), converted.tools);
        let mut events = adapter.start();
        events.extend(adapter.push_chat_chunk(&json!({
            "choices": [{ "delta": { "role": "assistant", "content": "hi" } }]
        })));
        events.extend(adapter.finish());
        let stream = events.join("");
        assert!(stream.contains("response.output_text.delta"));
        assert!(stream.contains("response.output_text.done"));
        assert!(stream.contains("response.completed"));
    }

    /// Chunk shape Agnes streams for a tool call: the first carries `id` and
    /// `function.name` with empty arguments, later ones carry only an arguments
    /// fragment and are tied to the first by `index`.
    fn agnes_tool_call_chunks(chat_name: &str) -> Vec<Value> {
        vec![
            json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"id":"call_abc","type":"function",
                 "function":{"name":chat_name,"arguments":""}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"type":"function","function":{"arguments":"{"}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"type":"function","function":{"arguments":"\"cmd\": \"ls\""}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"type":"function","function":{"arguments":"}"}}]}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
        ]
    }

    #[test]
    fn streams_incremental_events_for_a_function_call() {
        // Codex reads tool calls from the incremental events and ignores the
        // closing `response.completed` snapshot. An adapter that only filled in
        // that snapshot left every tool call invisible: the model asked to run
        // `exec_command`, the client saw an empty text message, nothing ran.
        let request = json!({
            "model": "m", "input": "x",
            "tools": [{ "type": "function", "name": "exec_command",
                        "parameters": { "type": "object" } }]
        });
        let converted = responses_to_chat(&request, None).unwrap();
        let mut adapter = StreamAdapter::new("m".into(), converted.tools);
        let mut events = adapter.start();
        for chunk in agnes_tool_call_chunks("exec_command") {
            events.extend(adapter.push_chat_chunk(&chunk));
        }
        // Assert before `finish()`, so the snapshot cannot satisfy these.
        let incremental = events.join("");
        assert!(
            incremental.contains("response.output_item.added"),
            "no output_item.added for the tool call; client cannot see it"
        );
        assert!(
            incremental.contains("response.function_call_arguments.delta"),
            "no argument deltas emitted"
        );
        assert!(incremental.contains("exec_command"));
        assert!(incremental.contains("call_abc"));

        events.extend(adapter.finish());
        let full = events.join("");
        assert!(full.contains("response.output_item.done"));
        assert!(full.contains("response.completed"));
    }

    #[test]
    fn folds_reasoning_into_the_assistant_message() {
        // Measured against Agnes with reasoning_effort=medium: 17 reasoning_content
        // deltas, one content delta holding just "\n\n", then the tool call.
        //
        // Emitting a `reasoning` output item was tried and does not survive the
        // client: Codex sends include:["reasoning.encrypted_content"] and discards
        // reasoning items lacking that blob, which a relay cannot forge. Over a
        // 12-turn session the gateway emitted 39 reasoning deltas and Codex stored
        // zero. Folding the text into the message is what keeps it in history.
        let request = json!({ "model": "m", "input": "x" });
        let converted = responses_to_chat(&request, None).unwrap();
        let mut adapter = StreamAdapter::new("m".into(), converted.tools);
        let mut events = adapter.start();
        for piece in ["Let me", " check", " the path"] {
            events.extend(adapter.push_chat_chunk(&json!({
                "choices": [{ "delta": { "reasoning_content": piece } }]
            })));
        }
        events.extend(adapter.push_chat_chunk(&json!({
            "choices": [{ "delta": { "content": "found it" } }]
        })));
        events.extend(adapter.finish());
        let full = events.join("");

        // No `reasoning` item: the client would throw it away.
        assert!(
            !full.contains("\"type\":\"reasoning\""),
            "reasoning item would be discarded by the client: {full}"
        );
        assert!(!full.contains("response.reasoning_text.delta"));
        // The thinking has to be in the message text, delimited and intact.
        assert!(
            full.contains("Let me check the path"),
            "reasoning text lost: {full}"
        );
        assert!(full.contains(REASONING_OPEN) && full.contains(REASONING_CLOSE));
        assert!(full.contains("found it"), "answer lost: {full}");
        // Thinking must precede the answer it produced.
        let think_at = full.find("Let me check the path").unwrap();
        let answer_at = full.find("found it").unwrap();
        assert!(
            think_at < answer_at,
            "reasoning must come before the answer"
        );
        // One message item, not one per flush: the reasoning and the answer share
        // it. Counting events rather than JSON substrings, since field order is
        // not guaranteed.
        assert_eq!(
            full.matches("event: response.output_item.added").count(),
            1,
            "reasoning and answer must share one message item: {full}"
        );
    }

    /// A truncation notice has to be the last thing in the text. Appending it
    /// before the reasoning flush would put it in front of the thinking it
    /// interrupted, which reads as though the turn ended and then kept going.
    #[test]
    fn a_truncation_notice_lands_after_the_reasoning_it_interrupted() {
        let mut adapter = StreamAdapter::new("m".into(), ToolMap::default());
        let mut events = adapter.start();
        events.extend(adapter.push_chat_chunk(&json!({
            "choices": [{ "delta": { "reasoning_content": "half a thought" } }]
        })));
        adapter.mark_truncated("上游连接中断");
        events.extend(adapter.finish());
        let full = events.join("");

        let think_at = full.find("half a thought").expect("reasoning lost");
        let notice_at = full.find("本轮输出不完整").expect("no notice emitted");
        assert!(
            think_at < notice_at,
            "notice must follow the thinking it cut off:\n{full}"
        );
        assert!(full.contains("上游连接中断"), "reason lost:\n{full}");
        assert!(full.contains("event: response.completed"));
    }

    /// Nothing is appended to a turn that ended normally.
    #[test]
    fn an_untruncated_turn_gets_no_notice() {
        let mut adapter = StreamAdapter::new("m".into(), ToolMap::default());
        let mut events = adapter.start();
        events.extend(adapter.push_chat_chunk(&json!({
            "choices": [{ "delta": { "content": "all done" } }]
        })));
        events.extend(adapter.finish());
        let full = events.join("");
        assert!(!full.contains("本轮输出不完整"), "spurious notice:\n{full}");
        assert!(full.contains("all done"));
    }

    #[test]
    fn reasoning_only_stream_still_produces_a_message() {
        // The stalling case: the whole output budget went to thinking, so there is
        // no content delta at all. Previously the completed response carried an
        // empty output array and the client showed nothing, then replayed a blank
        // assistant turn as history.
        let request = json!({ "model": "m", "input": "x" });
        let converted = responses_to_chat(&request, None).unwrap();
        let mut adapter = StreamAdapter::new("m".into(), converted.tools);
        let mut events = adapter.start();
        events.extend(adapter.push_chat_chunk(&json!({
            "choices": [{ "delta": { "reasoning_content": "thinking hard" } }]
        })));
        events.extend(adapter.finish());
        let full = events.join("");
        assert!(full.contains("response.completed"));
        assert!(
            full.contains("thinking hard"),
            "reasoning-only turn produced no visible output: {full}"
        );
        assert!(
            full.contains("\"type\":\"message\""),
            "no message item to carry it: {full}"
        );
    }

    #[test]
    fn a_turn_without_reasoning_gains_no_delimiters() {
        // Non-reasoning models must be unaffected — no stray markers in their text.
        let request = json!({ "model": "m", "input": "x" });
        let converted = responses_to_chat(&request, None).unwrap();
        let mut adapter = StreamAdapter::new("m".into(), converted.tools);
        let mut events = adapter.start();
        events.extend(adapter.push_chat_chunk(&json!({
            "choices": [{ "delta": { "content": "plain answer" } }]
        })));
        events.extend(adapter.finish());
        let full = events.join("");
        assert!(full.contains("plain answer"));
        assert!(
            !full.contains(REASONING_OPEN),
            "delimiters leaked into a non-reasoning turn: {full}"
        );
    }

    #[test]
    fn streams_a_bridged_custom_tool_under_its_original_name() {
        // `apply_patch` is renamed for the Chat call; the stream has to hand the
        // original name back or the client gets a tool it never registered.
        let request = json!({
            "model": "m", "input": "x",
            "tools": [{ "type": "custom", "name": "apply_patch",
                        "format": { "type": "text" } }]
        });
        let converted = responses_to_chat(&request, None).unwrap();
        let chat_name = converted.tools.chat_name("apply_patch").to_string();
        let mut adapter = StreamAdapter::new("m".into(), converted.tools);
        let mut events = adapter.start();
        for chunk in agnes_tool_call_chunks(&chat_name) {
            events.extend(adapter.push_chat_chunk(&chunk));
        }
        let incremental = events.join("");
        assert!(incremental.contains("custom_tool_call"));
        assert!(
            incremental.contains("apply_patch"),
            "custom tool must stream under its original name, got: {incremental}"
        );
    }

    #[test]
    fn restores_custom_tools_when_chat_backend_echoes_original_name() {
        let request = json!({
            "model": "dynamic-model", "input": "x",
            "tools": [{ "type": "custom", "name": "apply_patch", "format": { "type": "text" } }]
        });
        let converted = responses_to_chat(&request, None).unwrap();
        let chat = json!({
            "choices": [{ "finish_reason": "tool_calls", "message": {
                "role": "assistant", "content": null,
                "tool_calls": [{ "id": "call_1", "type": "function", "function": {
                    "name": "apply_patch", "arguments": "{\"input\":\"patch\"}"
                }}]
            }}]
        });
        let response = chat_to_response(&chat, &converted.tools).unwrap();
        assert_eq!(response["output"][0]["type"], "custom_tool_call");
    }

    #[test]
    fn maps_reasoning_effort_dynamically_per_model() {
        // Gemini minimal -> low
        let req_gemini = json!({
            "model": "gemini-2.5-pro", "input": "x", "reasoning": { "effort": "minimal" }
        });
        let c_gemini = responses_to_chat(&req_gemini, None).unwrap();
        assert_eq!(c_gemini.body["reasoning_effort"], "low");

        // DeepSeek R1 -> omits reasoning_effort
        let req_ds = json!({
            "model": "deepseek-reasoner", "input": "x", "reasoning": { "effort": "high" }
        });
        let c_ds = responses_to_chat(&req_ds, None).unwrap();
        assert!(c_ds.body.get("reasoning_effort").is_none());

        // Generic chat backend xhigh -> high
        let req_gen = json!({
            "model": "gpt-5.6-sol", "input": "x", "reasoning": { "effort": "xhigh" }
        });
        let c_gen = responses_to_chat(&req_gen, None).unwrap();
        assert_eq!(c_gen.body["reasoning_effort"], "high");

        // Plain DeepSeek chat model keeps xhigh (upstream accepts the full range)
        let req_dsv4 = json!({
            "model": "deepseek-v4-pro-0813", "input": "x", "reasoning": { "effort": "xhigh" }
        });
        let c_dsv4 = responses_to_chat(&req_dsv4, None).unwrap();
        assert_eq!(c_dsv4.body["reasoning_effort"], "xhigh");
    }
}
