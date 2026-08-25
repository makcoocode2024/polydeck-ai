//! Repair for Anthropic Messages SSE streams from incomplete upstreams.
//!
//! The Messages protocol requires `message_delta` to carry `stop_reason` before
//! `message_stop` closes the turn. Agnes omits it entirely — measured event
//! sequence for a turn that called a tool:
//!
//! ```text
//! message_start
//! content_block_start  index=0  tool_use
//! content_block_delta  index=0  input_json_delta
//! content_block_stop   index=0
//! content_block_start  index=1  text
//! content_block_delta  index=1  text_delta
//! content_block_stop   index=1
//! message_stop
//! ```
//!
//! Claude Code's parser needs a `stop_reason` to finalise the turn, and without
//! one it fails with `API Error: Content block not found`, then retries the same
//! request — observed as bursts of ~600ms requests in the gateway log ending in a
//! 400.
//!
//! `/v1/messages` is otherwise a byte-for-byte passthrough, so there was no layer
//! in which the gap could be covered. This adds the narrowest possible one: watch
//! the events go by, and if `message_stop` arrives with no `message_delta` before
//! it, synthesise one from what was actually seen. A stream that already carries
//! `message_delta` passes through untouched.

use serde_json::{json, Value};

/// Tracks one Messages SSE stream and supplies a `message_delta` when the
/// upstream fails to send one.
#[derive(Debug, Default)]
pub struct MessagesStreamRepair {
    saw_message_delta: bool,
    saw_tool_use: bool,
    /// Usage from `message_start`, echoed back so the synthesised delta reports
    /// the same input token count rather than zero.
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    /// Set once `message_stop` has been handled, so a repeated or trailing
    /// `message_stop` cannot emit a second delta.
    finished: bool,
    /// Indices opened by `content_block_start` in this stream.
    open_indices: Vec<u64>,
    /// Indices that received a `content_block_delta`/`_stop` without ever being
    /// opened. That is precisely the client's `Content block not found`
    /// condition, so recording it turns a client-side symptom into a
    /// gateway-side observation.
    orphan_indices: Vec<u64>,
    /// `(index, content_block.type)` per opened block, so a broken stream can be
    /// read back as a sequence rather than a set.
    block_types: Vec<(u64, String)>,
    /// Set when the turn was closed by `finish_truncated` rather than by the
    /// upstream's own `message_stop`.
    truncated: bool,
}

/// What the caller should write for one inbound SSE event.
#[derive(Debug, PartialEq)]
pub enum Emit {
    /// Forward the original bytes unchanged.
    Passthrough,
    /// Write these SSE blocks *before* forwarding the original.
    InsertBefore(Vec<String>),
}

impl MessagesStreamRepair {
    pub fn new() -> Self {
        Self::default()
    }

    /// Observe one parsed SSE event and decide what to write.
    ///
    /// `event` is the SSE event name, `data` its parsed JSON payload. Events the
    /// repair does not care about return `Passthrough`, so an unrecognised or
    /// future event type is never dropped.
    pub fn observe(&mut self, event: &str, data: &Value) -> Emit {
        match event {
            "message_start" => {
                // Usage lives at /message/usage on this event.
                if let Some(usage) = data.pointer("/message/usage") {
                    self.input_tokens = usage.get("input_tokens").and_then(Value::as_u64);
                    self.output_tokens = usage.get("output_tokens").and_then(Value::as_u64);
                }
                Emit::Passthrough
            }
            "content_block_start" => {
                if data.pointer("/content_block/type").and_then(Value::as_str) == Some("tool_use") {
                    self.saw_tool_use = true;
                }
                if let Some(i) = data.get("index").and_then(Value::as_u64) {
                    if !self.open_indices.contains(&i) {
                        self.open_indices.push(i);
                    }
                    self.block_types.push((
                        i,
                        data.pointer("/content_block/type")
                            .and_then(Value::as_str)
                            .unwrap_or("?")
                            .to_string(),
                    ));
                }
                Emit::Passthrough
            }
            "content_block_delta" | "content_block_stop" => {
                if let Some(i) = data.get("index").and_then(Value::as_u64) {
                    if !self.open_indices.contains(&i) {
                        // The upstream sent a delta or stop for an index it never
                        // opened, which is exactly what the client reports as
                        // `Content block not found` before failing the whole turn.
                        // Opening it here lets the rest of the stream through.
                        self.orphan_indices.push(i);
                        // Must be marked open, or every later delta on this index
                        // synthesises another start and the client gets a stream of
                        // duplicate `content_block_start` events instead.
                        self.open_indices.push(i);
                        let kind = infer_block_type(data);
                        self.block_types.push((i, format!("{kind}(synthetic)")));
                        return Emit::InsertBefore(vec![synthetic_start(i, kind)]);
                    }
                }
                Emit::Passthrough
            }
            "message_delta" => {
                self.saw_message_delta = true;
                // Keep the upstream's own usage if it bothered to send one.
                if let Some(out) = data.pointer("/usage/output_tokens").and_then(Value::as_u64) {
                    self.output_tokens = Some(out);
                }
                Emit::Passthrough
            }
            "message_stop" if !self.saw_message_delta && !self.finished => {
                self.finished = true;
                Emit::InsertBefore(vec![self.synthesised_delta()])
            }
            "message_stop" => {
                self.finished = true;
                Emit::Passthrough
            }
            _ => Emit::Passthrough,
        }
    }

    /// Build the `message_delta` the upstream should have sent.
    ///
    /// `stop_reason` is inferred from the blocks observed: a turn that opened a
    /// `tool_use` block stopped to call a tool, and anything else ended its turn.
    /// Guessing wrong here is still better than sending none — the client cannot
    /// finalise the turn at all without one.
    fn synthesised_delta(&self) -> String {
        let stop_reason = if self.saw_tool_use {
            "tool_use"
        } else {
            "end_turn"
        };
        let payload = json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop_reason, "stop_sequence": null },
            "usage": {
                "input_tokens": self.input_tokens.unwrap_or(0),
                "output_tokens": self.output_tokens.unwrap_or(0)
            }
        });
        format!("event: message_delta\ndata: {payload}\n\n")
    }

    /// True when a `message_delta` was synthesised, for logging.
    pub fn repaired(&self) -> bool {
        self.finished && !self.saw_message_delta
    }

    /// Close a turn whose stream died before `message_stop` arrived.
    ///
    /// Measured against sotamodel, `/v1/messages` truncates the same way
    /// `/v1/responses` does: the chunked body stops mid-`content_block_delta` with
    /// no terminal event. Claude Code needs `message_delta` for `stop_reason` and
    /// `message_stop` to finalise, so without these the turn is lost even though
    /// the text that arrived is perfectly usable.
    ///
    /// Every still-open block is closed first, or the client is left waiting on a
    /// `content_block_stop` that never comes.
    ///
    /// `stop_reason` is `max_tokens`, which is not literally what happened but is
    /// the only value in the protocol that means "output was cut short". `tool_use`
    /// would be actively harmful for a truncated `tool_use` block: its argument
    /// JSON is incomplete, and naming it would have the client invoke a tool with
    /// malformed input rather than report a short turn.
    pub fn finish_truncated(&mut self) -> Vec<String> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;
        self.truncated = true;
        let mut events = Vec::new();
        // Close in the order the blocks were opened, innermost state first.
        let open = std::mem::take(&mut self.open_indices);
        for index in &open {
            let payload = json!({ "type": "content_block_stop", "index": index });
            events.push(format!("event: content_block_stop\ndata: {payload}\n\n"));
        }
        self.open_indices = open;
        let delta = json!({
            "type": "message_delta",
            "delta": { "stop_reason": "max_tokens", "stop_sequence": null },
            "usage": {
                "input_tokens": self.input_tokens.unwrap_or(0),
                "output_tokens": self.output_tokens.unwrap_or(0)
            }
        });
        events.push(format!("event: message_delta\ndata: {delta}\n\n"));
        let stop = json!({ "type": "message_stop" });
        events.push(format!("event: message_stop\ndata: {stop}\n\n"));
        events
    }

    /// True once [`Self::finish_truncated`] closed a turn the upstream abandoned.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Indices that got a delta or stop without a matching `content_block_start`.
    pub fn orphan_indices(&self) -> &[u64] {
        &self.orphan_indices
    }

    /// Indices opened in this stream, in the order they were opened.
    pub fn open_indices(&self) -> &[u64] {
        &self.open_indices
    }

    /// `(index, type)` per opened block, in order.
    pub fn block_types(&self) -> &[(u64, String)] {
        &self.block_types
    }
}

/// Infer a block's type from the first event seen for it.
///
/// The delta names the content it carries, so this reads the answer rather than
/// guessing it: mislabelling a `tool_use` block as `text` would hand the client
/// partial JSON as prose. A bare `content_block_stop` carries no delta, and an
/// empty text block is the harmless reading of a block that produced nothing.
fn infer_block_type(data: &Value) -> &'static str {
    match data.pointer("/delta/type").and_then(Value::as_str) {
        Some("input_json_delta") => "tool_use",
        Some("thinking_delta") | Some("signature_delta") => "thinking",
        _ => "text",
    }
}

/// Build the `content_block_start` the upstream should have sent for `index`.
fn synthetic_start(index: u64, kind: &str) -> String {
    let block = match kind {
        // `tool_use` requires id and name; the deltas only carry the arguments, so
        // these are placeholders that keep the block well-formed.
        "tool_use" => {
            json!({ "type": "tool_use", "id": format!("synthetic_{index}"), "name": "unknown", "input": {} })
        }
        "thinking" => json!({ "type": "thinking", "thinking": "" }),
        _ => json!({ "type": "text", "text": "" }),
    };
    let payload = json!({
        "type": "content_block_start",
        "index": index,
        "content_block": block,
    });
    format!("event: content_block_start\ndata: {payload}\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(repair: &mut MessagesStreamRepair, name: &str, data: Value) -> Emit {
        repair.observe(name, &data)
    }

    #[test]
    fn paired_indices_produce_no_orphans() {
        let mut r = MessagesStreamRepair::new();
        for i in [0u64, 1] {
            ev(
                &mut r,
                "content_block_start",
                json!({"index": i, "content_block": {"type": "text"}}),
            );
            ev(
                &mut r,
                "content_block_delta",
                json!({"index": i, "delta": {"type": "text_delta"}}),
            );
            ev(&mut r, "content_block_stop", json!({"index": i}));
        }
        assert!(r.orphan_indices().is_empty());
        assert_eq!(r.open_indices(), &[0, 1]);
    }

    #[test]
    fn a_delta_on_an_unopened_index_is_an_orphan() {
        // The client's `Content block not found` condition, stated directly.
        let mut r = MessagesStreamRepair::new();
        ev(
            &mut r,
            "content_block_start",
            json!({"index": 0, "content_block": {"type": "text"}}),
        );
        ev(
            &mut r,
            "content_block_delta",
            json!({"index": 1, "delta": {"type": "text_delta"}}),
        );
        assert_eq!(r.orphan_indices(), &[1]);
    }

    #[test]
    fn an_orphan_index_is_opened_once_not_per_delta() {
        // The bug this guards: without marking the index open, every later delta
        // synthesises another start and the client gets duplicates instead of one.
        let mut r = MessagesStreamRepair::new();
        let first = ev(
            &mut r,
            "content_block_delta",
            json!({"index": 3, "delta": {"type": "text_delta"}}),
        );
        assert!(matches!(first, Emit::InsertBefore(_)));
        assert_eq!(
            ev(
                &mut r,
                "content_block_delta",
                json!({"index": 3, "delta": {"type": "text_delta"}})
            ),
            Emit::Passthrough
        );
        assert_eq!(
            ev(&mut r, "content_block_stop", json!({"index": 3})),
            Emit::Passthrough
        );
        assert_eq!(r.orphan_indices(), &[3]);
    }

    #[test]
    fn an_orphan_delta_gets_a_synthetic_start_before_it() {
        let mut r = MessagesStreamRepair::new();
        let out = ev(
            &mut r,
            "content_block_delta",
            json!({"index": 4, "delta": {"type": "text_delta"}}),
        );
        let Emit::InsertBefore(blocks) = out else {
            panic!("expected a synthetic start");
        };
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].starts_with("event: content_block_start\n"));
        assert!(blocks[0].contains("\"index\":4"));
        assert!(blocks[0].ends_with("\n\n"));
    }

    #[test]
    fn a_synthetic_start_takes_its_type_from_the_delta() {
        // Labelling a tool_use block as text would hand the client partial JSON as
        // prose, so the delta's own type has to drive this.
        let mut r = MessagesStreamRepair::new();
        let out = ev(
            &mut r,
            "content_block_delta",
            json!({"index": 4, "delta": {"type": "input_json_delta"}}),
        );
        let Emit::InsertBefore(blocks) = out else {
            panic!("expected a synthetic start");
        };
        assert!(blocks[0].contains("\"type\":\"tool_use\""));
        assert!(blocks[0].contains("\"id\":\"synthetic_4\""));

        let mut r2 = MessagesStreamRepair::new();
        let out2 = ev(
            &mut r2,
            "content_block_delta",
            json!({"index": 0, "delta": {"type": "thinking_delta"}}),
        );
        let Emit::InsertBefore(b2) = out2 else {
            panic!("expected a synthetic start");
        };
        assert!(b2[0].contains("\"type\":\"thinking\""));
    }

    #[test]
    fn a_bare_stop_on_an_unopened_index_still_opens_it() {
        let mut r = MessagesStreamRepair::new();
        let out = ev(&mut r, "content_block_stop", json!({"index": 2}));
        let Emit::InsertBefore(blocks) = out else {
            panic!("expected a synthetic start");
        };
        assert!(blocks[0].contains("\"type\":\"text\""));
    }

    /// The exact broken sequence measured from Agnes: index 4 skipped entirely.
    #[test]
    fn the_measured_agnes_gap_is_repaired() {
        let mut r = MessagesStreamRepair::new();
        for (i, kind) in [
            (0u64, "thinking"),
            (1, "text"),
            (2, "tool_use"),
            (3, "text"),
        ] {
            ev(
                &mut r,
                "content_block_start",
                json!({"index": i, "content_block": {"type": kind}}),
            );
        }
        // Agnes jumps to 5 without ever opening 4, then sends 4's delta.
        ev(
            &mut r,
            "content_block_start",
            json!({"index": 5, "content_block": {"type": "tool_use"}}),
        );
        let out = ev(
            &mut r,
            "content_block_delta",
            json!({"index": 4, "delta": {"type": "input_json_delta"}}),
        );
        assert!(
            matches!(out, Emit::InsertBefore(_)),
            "index 4 must be opened"
        );
        assert_eq!(r.orphan_indices(), &[4]);
        assert!(r.open_indices().contains(&4));
    }

    /// The exact sequence measured from Agnes.
    #[test]
    fn synthesises_a_delta_when_the_upstream_omits_one() {
        let mut r = MessagesStreamRepair::new();
        assert_eq!(
            ev(
                &mut r,
                "message_start",
                json!({
                    "type": "message_start",
                    "message": { "usage": { "input_tokens": 1234, "output_tokens": 0 } }
                })
            ),
            Emit::Passthrough
        );
        ev(
            &mut r,
            "content_block_start",
            json!({
                "type": "content_block_start", "index": 0,
                "content_block": { "type": "tool_use", "id": "toolu_1", "name": "Bash" }
            }),
        );
        ev(
            &mut r,
            "content_block_delta",
            json!({
                "type": "content_block_delta", "index": 0,
                "delta": { "type": "input_json_delta", "partial_json": "{}" }
            }),
        );
        ev(
            &mut r,
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": 0 }),
        );

        let emitted = ev(&mut r, "message_stop", json!({ "type": "message_stop" }));
        let Emit::InsertBefore(blocks) = emitted else {
            panic!("expected a synthesised delta, got {emitted:?}");
        };
        assert_eq!(blocks.len(), 1);
        let block = &blocks[0];
        assert!(
            block.starts_with("event: message_delta\n"),
            "bad framing: {block}"
        );
        assert!(
            block.ends_with("\n\n"),
            "SSE block must end blank-line: {block}"
        );

        let data: Value = serde_json::from_str(
            block
                .lines()
                .find_map(|l| l.strip_prefix("data: "))
                .unwrap(),
        )
        .expect("payload must be JSON");
        // A turn that opened a tool_use block stopped to call the tool.
        assert_eq!(data["delta"]["stop_reason"], "tool_use");
        // Usage from message_start is carried across, not zeroed.
        assert_eq!(data["usage"]["input_tokens"], 1234);
        assert!(r.repaired());
    }

    #[test]
    fn a_text_only_turn_ends_the_turn() {
        let mut r = MessagesStreamRepair::new();
        ev(
            &mut r,
            "message_start",
            json!({ "message": { "usage": { "input_tokens": 10 } } }),
        );
        ev(
            &mut r,
            "content_block_start",
            json!({
                "index": 0, "content_block": { "type": "text", "text": "" }
            }),
        );
        let Emit::InsertBefore(blocks) = ev(&mut r, "message_stop", json!({})) else {
            panic!("expected a synthesised delta");
        };
        let data: Value = serde_json::from_str(
            blocks[0]
                .lines()
                .find_map(|l| l.strip_prefix("data: "))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(data["delta"]["stop_reason"], "end_turn");
    }

    /// A conforming upstream must be left alone.
    #[test]
    fn a_stream_that_already_has_a_delta_is_untouched() {
        let mut r = MessagesStreamRepair::new();
        ev(
            &mut r,
            "message_start",
            json!({ "message": { "usage": { "input_tokens": 5 } } }),
        );
        ev(
            &mut r,
            "content_block_start",
            json!({
                "index": 0, "content_block": { "type": "text" }
            }),
        );
        assert_eq!(
            ev(
                &mut r,
                "message_delta",
                json!({
                    "type": "message_delta",
                    "delta": { "stop_reason": "end_turn" },
                    "usage": { "output_tokens": 42 }
                })
            ),
            Emit::Passthrough
        );
        assert_eq!(
            ev(&mut r, "message_stop", json!({})),
            Emit::Passthrough,
            "must not insert a second delta"
        );
        assert!(!r.repaired());
    }

    #[test]
    fn a_repeated_message_stop_does_not_emit_twice() {
        let mut r = MessagesStreamRepair::new();
        ev(&mut r, "message_start", json!({}));
        assert!(matches!(
            ev(&mut r, "message_stop", json!({})),
            Emit::InsertBefore(_)
        ));
        assert_eq!(
            ev(&mut r, "message_stop", json!({})),
            Emit::Passthrough,
            "a trailing message_stop must not produce another delta"
        );
    }

    #[test]
    fn unknown_events_pass_through_untouched() {
        let mut r = MessagesStreamRepair::new();
        assert_eq!(ev(&mut r, "ping", json!({})), Emit::Passthrough);
        assert_eq!(
            ev(&mut r, "some_future_event", json!({ "x": 1 })),
            Emit::Passthrough
        );
    }

    #[test]
    fn missing_usage_does_not_prevent_repair() {
        // An upstream that omits usage as well must still get a usable delta.
        let mut r = MessagesStreamRepair::new();
        ev(&mut r, "message_start", json!({ "type": "message_start" }));
        let Emit::InsertBefore(blocks) = ev(&mut r, "message_stop", json!({})) else {
            panic!("expected a delta");
        };
        let data: Value = serde_json::from_str(
            blocks[0]
                .lines()
                .find_map(|l| l.strip_prefix("data: "))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(data["usage"]["input_tokens"], 0);
        assert_eq!(data["delta"]["stop_reason"], "end_turn");
    }

    /// The measured sotamodel truncation: text flowing, then the body is cut off
    /// with the block still open and no `message_stop`.
    #[test]
    fn a_truncated_turn_is_closed_with_a_stop_reason() {
        let mut r = MessagesStreamRepair::new();
        ev(
            &mut r,
            "message_start",
            json!({ "type": "message_start",
                    "message": { "usage": { "input_tokens": 40, "output_tokens": 7 } } }),
        );
        ev(
            &mut r,
            "content_block_start",
            json!({ "type": "content_block_start", "index": 0,
                    "content_block": { "type": "text", "text": "" } }),
        );
        ev(
            &mut r,
            "content_block_delta",
            json!({ "type": "content_block_delta", "index": 0,
                    "delta": { "type": "text_delta", "text": "partial" } }),
        );
        let out = r.finish_truncated().join("");
        assert!(r.truncated());
        assert!(
            out.contains("event: content_block_stop"),
            "open block left dangling: {out}"
        );
        assert!(out.contains("event: message_delta"), "{out}");
        assert!(out.contains("event: message_stop"), "{out}");
        // max_tokens, not tool_use: see finish_truncated's rationale.
        assert!(out.contains("\"stop_reason\":\"max_tokens\""), "{out}");
        // The usage seen at message_start has to survive.
        assert!(out.contains("\"input_tokens\":40"), "{out}");
    }

    #[test]
    fn a_turn_that_already_stopped_is_not_closed_again() {
        let mut r = MessagesStreamRepair::new();
        ev(&mut r, "message_start", json!({ "type": "message_start" }));
        ev(
            &mut r,
            "message_delta",
            json!({ "type": "message_delta", "delta": { "stop_reason": "end_turn" } }),
        );
        ev(&mut r, "message_stop", json!({ "type": "message_stop" }));
        assert!(r.finish_truncated().is_empty());
        assert!(!r.truncated());
    }

    /// A truncated `tool_use` block must not be reported as `tool_use`: its
    /// argument JSON is incomplete, and the client would call the tool with it.
    #[test]
    fn a_truncated_tool_use_turn_does_not_claim_tool_use() {
        let mut r = MessagesStreamRepair::new();
        ev(&mut r, "message_start", json!({ "type": "message_start" }));
        ev(
            &mut r,
            "content_block_start",
            json!({ "type": "content_block_start", "index": 0,
                    "content_block": { "type": "tool_use", "id": "t1", "name": "shell" } }),
        );
        let out = r.finish_truncated().join("");
        assert!(out.contains("\"stop_reason\":\"max_tokens\""), "{out}");
        assert!(!out.contains("\"stop_reason\":\"tool_use\""), "{out}");
    }

    #[test]
    fn every_open_block_gets_a_stop() {
        let mut r = MessagesStreamRepair::new();
        ev(&mut r, "message_start", json!({ "type": "message_start" }));
        for index in 0..3 {
            ev(
                &mut r,
                "content_block_start",
                json!({ "type": "content_block_start", "index": index,
                        "content_block": { "type": "text", "text": "" } }),
            );
        }
        let out = r.finish_truncated();
        let stops = out
            .iter()
            .filter(|e| e.starts_with("event: content_block_stop"))
            .count();
        assert_eq!(stops, 3, "not every block closed: {out:?}");
    }
}
