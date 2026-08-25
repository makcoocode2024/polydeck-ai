//! Repair for OpenAI Responses SSE streams that stop mid-flight.
//!
//! `/v1/responses` in native passthrough mode forwards upstream bytes verbatim,
//! which is correct right up until the upstream stops sending. Measured against
//! sotamodel with `reasoning.effort=high`, five of five long requests died the
//! same way:
//!
//! ```text
//! status=200 dur=33.6s bytes=50887 IncompleteRead(0 bytes read)
//! status=200 dur=35.4s bytes=57950 IncompleteRead(0 bytes read)
//! status=200 dur=40.4s bytes=55993 IncompleteRead(0 bytes read)
//! ```
//!
//! The chunked body is cut off between two `response.reasoning_summary_text.delta`
//! events, with no terminal event and no zero-length chunk. Codex reports it as
//! `stream disconnected before completion: Transport error: network error: error
//! decoding response body` and loses the whole turn, including the reasoning and
//! text that did arrive.
//!
//! This watches the events go by and, if the stream ends without a terminal one,
//! closes whatever is still open and emits the `response.completed` the upstream
//! owed. The partial output survives.
//!
//! `response.completed` rather than `response.incomplete`, even though the turn
//! genuinely is incomplete: the installed Codex binary contains 32 occurrences of
//! the former and zero of the latter, so `response.incomplete` would be ignored
//! and the turn would fail exactly as before.

use serde_json::{json, Value};

/// Terminal events after which nothing needs synthesising.
const TERMINAL_EVENTS: [&str; 3] = [
    "response.completed",
    "response.failed",
    "response.incomplete",
];

/// One output item seen in the stream, tracked so it can be closed if the stream
/// dies while the item is still open.
#[derive(Debug, Clone)]
struct OpenItem {
    output_index: u64,
    item_id: String,
    /// `item.type` from `response.output_item.added` — decides which closing
    /// events the item needs.
    kind: String,
    /// Text accumulated from delta events, replayed into the closing events so
    /// the client keeps what actually arrived.
    text: String,
    /// Set by `response.reasoning_summary_part.added`; a reasoning item needs its
    /// part closed before the item itself.
    part_open: bool,
    /// Set by `response.content_part.added`, same reason for message items.
    content_part_open: bool,
}

/// Tracks one Responses SSE stream and supplies the terminal events an upstream
/// that dies mid-stream never sent.
#[derive(Debug, Default)]
pub struct ResponsesStreamRepair {
    /// The `response` object from `response.created`, reused as the base of the
    /// synthesised snapshot so id, model and settings match what the client was
    /// already told.
    created_snapshot: Option<Value>,
    /// Highest `sequence_number` seen. Synthesised events continue the numbering
    /// rather than restarting it.
    max_sequence: u64,
    saw_terminal: bool,
    open_items: Vec<OpenItem>,
    /// Items already closed by the upstream, in `output_index` order, kept to
    /// rebuild the `output` array of the final snapshot.
    done_items: Vec<(u64, Value)>,
    synthesised: bool,
}

impl ResponsesStreamRepair {
    pub fn new() -> Self {
        Self::default()
    }

    /// Observe one parsed SSE event. Every event is forwarded unchanged by the
    /// caller; this only records state.
    pub fn observe(&mut self, event: &str, data: &Value) {
        if let Some(seq) = data.get("sequence_number").and_then(Value::as_u64) {
            self.max_sequence = self.max_sequence.max(seq);
        }
        if TERMINAL_EVENTS.contains(&event) {
            self.saw_terminal = true;
            return;
        }
        match event {
            "response.created" | "response.in_progress" => {
                if let Some(resp) = data.get("response") {
                    self.created_snapshot = Some(resp.clone());
                }
            }
            "response.output_item.added" => {
                let Some(index) = data.get("output_index").and_then(Value::as_u64) else {
                    return;
                };
                let item = data.get("item");
                self.open_items.push(OpenItem {
                    output_index: index,
                    item_id: item
                        .and_then(|i| i.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    kind: item
                        .and_then(|i| i.get("type"))
                        .and_then(Value::as_str)
                        .unwrap_or("message")
                        .to_string(),
                    text: String::new(),
                    part_open: false,
                    content_part_open: false,
                });
            }
            "response.output_item.done" => {
                let Some(index) = data.get("output_index").and_then(Value::as_u64) else {
                    return;
                };
                self.open_items.retain(|i| i.output_index != index);
                if let Some(item) = data.get("item") {
                    self.done_items.push((index, item.clone()));
                }
            }
            "response.content_part.added" => {
                if let Some(item) = self.item_at(data) {
                    item.content_part_open = true;
                }
            }
            "response.reasoning_summary_part.added" => {
                if let Some(item) = self.item_at(data) {
                    item.part_open = true;
                }
            }
            // Text arriving for an item, whichever flavour of delta carries it.
            "response.output_text.delta"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_text.delta" => {
                let delta = data
                    .get("delta")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if let Some(item) = self.item_at(data) {
                    item.text.push_str(&delta);
                }
            }
            "response.output_text.done"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_text.done" => {
                let text = data.get("text").and_then(Value::as_str).map(str::to_string);
                if let Some(item) = self.item_at(data) {
                    if let Some(t) = text {
                        item.text = t;
                    }
                }
            }
            "response.reasoning_summary_part.done" => {
                if let Some(item) = self.item_at(data) {
                    item.part_open = false;
                }
            }
            "response.content_part.done" => {
                if let Some(item) = self.item_at(data) {
                    item.content_part_open = false;
                }
            }
            _ => {}
        }
    }

    /// The open item an event refers to, matched on `output_index` and falling
    /// back to `item_id` for events that carry only that.
    fn item_at(&mut self, data: &Value) -> Option<&mut OpenItem> {
        if let Some(index) = data.get("output_index").and_then(Value::as_u64) {
            return self.open_items.iter_mut().find(|i| i.output_index == index);
        }
        let item_id = data.get("item_id").and_then(Value::as_str)?;
        self.open_items.iter_mut().find(|i| i.item_id == item_id)
    }

    /// True when the stream already carried a terminal event, so the caller has
    /// nothing to add.
    pub fn saw_terminal(&self) -> bool {
        self.saw_terminal
    }

    /// True once [`Self::finish_truncated`] actually synthesised events.
    pub fn synthesised(&self) -> bool {
        self.synthesised
    }

    /// How many items were still open when the stream died, for logging.
    pub fn open_count(&self) -> usize {
        self.open_items.len()
    }

    /// Close every open item and emit the terminal `response.completed`.
    ///
    /// Returns an empty vec when the stream already ended properly, so calling
    /// this on a healthy stream is a no-op. `reason` lands in
    /// `incomplete_details.reason` — the turn is reported as completed so the
    /// client keeps the partial output, but that field records the truth.
    pub fn finish_truncated(&mut self, reason: &str) -> Vec<String> {
        if self.saw_terminal {
            return Vec::new();
        }
        self.saw_terminal = true;
        self.synthesised = true;
        let mut events = Vec::new();
        let mut open = std::mem::take(&mut self.open_items);
        open.sort_by_key(|i| i.output_index);
        for item in open {
            events.extend(self.close_item(&item));
        }
        self.done_items.sort_by_key(|(index, _)| *index);
        let output: Vec<Value> = self
            .done_items
            .iter()
            .map(|(_, item)| item.clone())
            .collect();
        let mut response = match self.created_snapshot.clone() {
            Some(Value::Object(map)) => Value::Object(map),
            // No `response.created` means the stream died before the first event.
            // A minimal snapshot still gives the client something terminal to
            // parse instead of a severed connection.
            _ => json!({ "object": "response" }),
        };
        if let Some(map) = response.as_object_mut() {
            map.insert("status".into(), Value::String("completed".into()));
            map.insert("output".into(), Value::Array(output));
            map.insert(
                "incomplete_details".into(),
                json!({ "reason": reason.to_string() }),
            );
        }
        events.push(self.event(
            "response.completed",
            json!({ "type": "response.completed", "response": response }),
        ));
        events
    }

    /// The closing events one open item needs, innermost first.
    fn close_item(&mut self, item: &OpenItem) -> Vec<String> {
        let mut events = Vec::new();
        let is_reasoning = item.kind == "reasoning";
        if is_reasoning {
            if item.part_open {
                events.push(self.event(
                    "response.reasoning_summary_text.done",
                    json!({
                        "type": "response.reasoning_summary_text.done",
                        "item_id": item.item_id, "output_index": item.output_index,
                        "summary_index": 0, "text": item.text
                    }),
                ));
                events.push(self.event(
                    "response.reasoning_summary_part.done",
                    json!({
                        "type": "response.reasoning_summary_part.done",
                        "item_id": item.item_id, "output_index": item.output_index,
                        "summary_index": 0,
                        "part": { "type": "summary_text", "text": item.text }
                    }),
                ));
            }
            let done = json!({
                "id": item.item_id, "type": "reasoning", "status": "completed",
                "summary": [{ "type": "summary_text", "text": item.text }]
            });
            events.push(self.event(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": item.output_index, "item": done
                }),
            ));
            self.done_items.push((item.output_index, done));
            return events;
        }
        if item.content_part_open {
            events.push(self.event(
                "response.output_text.done",
                json!({
                    "type": "response.output_text.done", "item_id": item.item_id,
                    "output_index": item.output_index, "content_index": 0,
                    "text": item.text, "logprobs": []
                }),
            ));
            events.push(self.event(
                "response.content_part.done",
                json!({
                    "type": "response.content_part.done", "item_id": item.item_id,
                    "output_index": item.output_index, "content_index": 0,
                    "part": { "type": "output_text", "annotations": [], "text": item.text }
                }),
            ));
        }
        let done = json!({
            "id": item.item_id, "type": "message", "status": "completed", "role": "assistant",
            "content": [{ "type": "output_text", "annotations": [], "logprobs": [], "text": item.text }]
        });
        events.push(self.event(
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": item.output_index, "item": done
            }),
        ));
        self.done_items.push((item.output_index, done));
        events
    }

    /// Format one SSE block, continuing the upstream's `sequence_number`.
    fn event(&mut self, name: &str, mut payload: Value) -> String {
        self.max_sequence += 1;
        if let Some(map) = payload.as_object_mut() {
            map.insert("sequence_number".into(), json!(self.max_sequence));
        }
        format!("event: {name}\ndata: {payload}\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn created(id: &str) -> Value {
        json!({
            "type": "response.created", "sequence_number": 0,
            "response": { "id": id, "object": "response", "model": "claude-opus-5",
                          "status": "in_progress", "output": [] }
        })
    }

    /// The measured sotamodel failure: a reasoning item open, deltas flowing, then
    /// the body is cut off.
    #[test]
    fn truncated_reasoning_stream_gets_a_terminal_event() {
        let mut r = ResponsesStreamRepair::new();
        r.observe("response.created", &created("resp_1"));
        r.observe(
            "response.output_item.added",
            &json!({ "sequence_number": 2, "output_index": 0,
                     "item": { "id": "rs_1", "type": "reasoning" } }),
        );
        r.observe(
            "response.reasoning_summary_part.added",
            &json!({ "sequence_number": 3, "output_index": 0, "item_id": "rs_1" }),
        );
        for (i, chunk) in ["Let me ", "think about ", "primes."].iter().enumerate() {
            r.observe(
                "response.reasoning_summary_text.delta",
                &json!({ "sequence_number": 4 + i, "output_index": 0,
                         "item_id": "rs_1", "delta": chunk }),
            );
        }
        assert!(!r.saw_terminal());
        let out = r.finish_truncated("upstream_disconnected").join("");
        assert!(r.synthesised());
        assert!(
            out.contains("event: response.reasoning_summary_text.done"),
            "open summary part must be closed: {out}"
        );
        assert!(out.contains("event: response.reasoning_summary_part.done"));
        assert!(out.contains("event: response.output_item.done"));
        assert!(out.contains("event: response.completed"));
        // The reasoning that did arrive has to survive into the snapshot.
        assert!(
            out.contains("Let me think about primes."),
            "partial reasoning lost: {out}"
        );
        assert!(out.contains("upstream_disconnected"));
        assert!(out.contains("resp_1"), "response id lost: {out}");
    }

    #[test]
    fn healthy_stream_is_left_alone() {
        let mut r = ResponsesStreamRepair::new();
        r.observe("response.created", &created("resp_2"));
        r.observe(
            "response.completed",
            &json!({ "sequence_number": 9, "response": { "id": "resp_2" } }),
        );
        assert!(r.saw_terminal());
        assert!(r.finish_truncated("upstream_disconnected").is_empty());
        assert!(!r.synthesised());
    }

    #[test]
    fn synthesised_events_continue_the_sequence_numbering() {
        let mut r = ResponsesStreamRepair::new();
        r.observe("response.created", &created("resp_3"));
        r.observe(
            "response.output_item.added",
            &json!({ "sequence_number": 178, "output_index": 0,
                     "item": { "id": "msg_1", "type": "message" } }),
        );
        let out = r.finish_truncated("idle_timeout").join("");
        // Restarting at 0 would make the client discard events as out of order.
        assert!(out.contains("\"sequence_number\":179"), "{out}");
        assert!(!out.contains("\"sequence_number\":0"), "{out}");
    }

    #[test]
    fn message_item_closes_its_content_part() {
        let mut r = ResponsesStreamRepair::new();
        r.observe("response.created", &created("resp_4"));
        r.observe(
            "response.output_item.added",
            &json!({ "sequence_number": 2, "output_index": 0,
                     "item": { "id": "msg_2", "type": "message" } }),
        );
        r.observe(
            "response.content_part.added",
            &json!({ "sequence_number": 3, "output_index": 0, "item_id": "msg_2" }),
        );
        r.observe(
            "response.output_text.delta",
            &json!({ "sequence_number": 4, "output_index": 0,
                     "item_id": "msg_2", "delta": "partial answer" }),
        );
        let out = r.finish_truncated("upstream_disconnected").join("");
        assert!(out.contains("event: response.output_text.done"), "{out}");
        assert!(out.contains("event: response.content_part.done"), "{out}");
        assert!(out.contains("partial answer"), "{out}");
    }

    /// An item the upstream already closed must appear in the final snapshot
    /// exactly once, not be re-closed by the repair.
    #[test]
    fn already_closed_items_are_not_closed_twice() {
        let mut r = ResponsesStreamRepair::new();
        r.observe("response.created", &created("resp_5"));
        r.observe(
            "response.output_item.added",
            &json!({ "sequence_number": 2, "output_index": 0,
                     "item": { "id": "rs_5", "type": "reasoning" } }),
        );
        r.observe(
            "response.output_item.done",
            &json!({ "sequence_number": 3, "output_index": 0,
                     "item": { "id": "rs_5", "type": "reasoning",
                               "summary": [{ "type": "summary_text", "text": "done" }] } }),
        );
        assert_eq!(r.open_count(), 0);
        let out = r.finish_truncated("upstream_disconnected").join("");
        assert_eq!(
            out.matches("event: response.output_item.done").count(),
            0,
            "closed item re-closed: {out}"
        );
        assert!(out.contains("event: response.completed"));
        assert!(
            out.contains("rs_5"),
            "closed item missing from snapshot: {out}"
        );
    }

    /// The stream can die before any event arrives. There is nothing to report,
    /// but the client still needs a terminal event rather than a dropped socket.
    #[test]
    fn stream_dying_before_any_event_still_terminates() {
        let mut r = ResponsesStreamRepair::new();
        let out = r.finish_truncated("upstream_disconnected").join("");
        assert!(out.contains("event: response.completed"), "{out}");
        assert!(out.contains("upstream_disconnected"));
    }

    #[test]
    fn multiple_open_items_close_in_index_order() {
        let mut r = ResponsesStreamRepair::new();
        r.observe("response.created", &created("resp_6"));
        for (index, id, kind) in [(1u64, "msg_6", "message"), (0, "rs_6", "reasoning")] {
            r.observe(
                "response.output_item.added",
                &json!({ "output_index": index, "item": { "id": id, "type": kind } }),
            );
        }
        let out = r.finish_truncated("upstream_disconnected").join("");
        let rs = out.find("rs_6").expect("reasoning item missing");
        let msg = out.find("msg_6").expect("message item missing");
        assert!(rs < msg, "items closed out of order: {out}");
    }
}
