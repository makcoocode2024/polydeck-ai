//! Streaming protocol bridge: Chat Completions SSE -> OpenAI Responses SSE format

use serde_json::{json, Value};

pub struct StreamAdapter {
    model: String,
    response_id: String,
    accumulated_content: String,
    accumulated_thinking: String,
    tool_calls: Vec<ToolCallState>,
    text_started: bool,
}

#[derive(Debug, Clone)]
struct ToolCallState {
    id: String,
    name: String,
    arguments: String,
}

impl StreamAdapter {
    pub fn new(model: String) -> Self {
        Self {
            model,
            response_id: String::new(),
            accumulated_content: String::new(),
            accumulated_thinking: String::new(),
            tool_calls: Vec::new(),
            text_started: false,
        }
    }

    pub fn start(&mut self) -> Vec<String> {
        if self.response_id.is_empty() {
            self.response_id = "temp-id".to_string();
        }
        let response = json!({
            "id": self.response_id, "object": "response", "created_at": 0,
            "status": "in_progress", "model": self.model,
            "output": [], "parallel_tool_calls": true
        });
        vec![self.event(
            "response.created",
            json!({"type": "response.created", "response": response}),
        )]
    }

    pub fn push_chat_chunk(&mut self, chunk: &Value) -> Vec<String> {
        let mut events = Vec::new();
        if let Some(id) = chunk.get("id").and_then(Value::as_str) {
            if self.response_id.is_empty() {
                self.response_id = id.to_string();
            }
        }
        let choices = match chunk.get("choices").and_then(Value::as_array) {
            Some(c) if !c.is_empty() => c,
            _ => return events,
        };
        let delta = match choices[0].get("delta") {
            Some(d) => d,
            None => return events,
        };
        if let Some(thinking) = delta.get("thinking").and_then(Value::as_str) {
            self.accumulated_thinking.push_str(thinking);
            events.push(self.event("response.reasoning_summary_text.delta", json!({
                "type": "response.reasoning_summary_text.delta",
                "item_id": self.item_id(), "output_index": 0, "summary_index": 0, "delta": thinking
            })));
        }
        if let Some(content) = delta.get("content").and_then(Value::as_str) {
            if !self.text_started {
                self.text_started = true;
                events.push(self.event("response.output_item.added", json!({
                    "type": "response.output_item.added", "output_index": 0,
                    "item": {"id": self.item_id(), "type": "message", "role": "assistant", "content": []}
                })));
                events.push(self.event(
                    "response.content_part.added",
                    json!({
                        "type": "response.content_part.added", "item_id": self.item_id(),
                        "output_index": 0, "content_index": 0,
                        "part": {"type": "output_text", "text": "", "annotations": []}
                    }),
                ));
            }
            self.accumulated_content.push_str(content);
            events.push(self.event(
                "response.output_text.delta",
                json!({
                    "type": "response.output_text.delta", "item_id": self.item_id(),
                    "output_index": 0, "content_index": 0, "delta": content
                }),
            ));
        }
        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for tool_call in tool_calls {
                let index = tool_call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                while self.tool_calls.len() <= index {
                    self.tool_calls.push(ToolCallState {
                        id: String::new(),
                        name: String::new(),
                        arguments: String::new(),
                    });
                }
                let state = &mut self.tool_calls[index];
                if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
                    state.id = id.to_string();
                }
                if let Some(function) = tool_call.get("function") {
                    if let Some(name) = function.get("name").and_then(Value::as_str) {
                        state.name = name.to_string();
                    }
                    if let Some(args) = function.get("arguments").and_then(Value::as_str) {
                        state.arguments.push_str(args);
                    }
                }
            }
        }
        events
    }

    pub fn finish(&self) -> Vec<String> {
        let id = self.item_id();
        let mut output = Vec::new();
        if !self.accumulated_content.is_empty() {
            output.push(json!({
                "id": id, "type": "message", "role": "assistant",
                "content": [{"type": "output_text", "text": self.accumulated_content, "annotations": []}],
                "status": "completed"
            }));
        }
        for tool_call in &self.tool_calls {
            if !tool_call.name.is_empty() {
                output.push(json!({
                    "id": if tool_call.id.is_empty() { id.clone() } else { tool_call.id.clone() },
                    "type": "function_call", "call_id": tool_call.id,
                    "name": tool_call.name, "arguments": tool_call.arguments, "status": "completed"
                }));
            }
        }
        let response = json!({
            "id": if self.response_id.is_empty() { "completed" } else { &self.response_id },
            "object": "response", "status": "completed", "model": self.model,
            "output": output, "parallel_tool_calls": true
        });
        vec![self.event(
            "response.completed",
            json!({"type": "response.completed", "response": response}),
        )]
    }

    fn item_id(&self) -> String {
        if self.response_id.is_empty() {
            "msg_0".to_string()
        } else {
            format!("{}_msg", self.response_id)
        }
    }

    fn event(&self, event_type: &str, payload: Value) -> String {
        format!("event: {}\ndata: {}\n\n", event_type, payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_responses_completion_event() {
        let mut adapter = StreamAdapter::new("test-model".to_string());
        adapter
            .push_chat_chunk(&json!({"id":"chatcmpl-123","choices":[{"delta":{"content":"hi"}}]}));
        let events = adapter.finish();
        assert_eq!(events.len(), 1);
        assert!(events[0].contains("response.completed"));
    }

    #[test]
    fn emits_output_text_delta_for_content() {
        let mut adapter = StreamAdapter::new("test-model".to_string());
        let events = adapter
            .push_chat_chunk(&json!({"id":"chatcmpl-123","choices":[{"delta":{"content":"hi"}}]}));
        assert!(events
            .iter()
            .any(|e| e.contains("response.output_text.delta")));
    }
}
