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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(repair: &mut MessagesStreamRepair, name: &str, data: Value) -> Emit {
        repair.observe(name, &data)
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
}
