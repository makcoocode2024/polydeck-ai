//! Server-sent event framing and stream relays.
//!
//! Block splitting, event parsing, and the two passthrough streams: one verbatim,
//! one that runs Anthropic events through `MessagesStreamRepair`. Kept together
//! because they all deal in raw SSE bytes rather than in request semantics.

use axum::{
    body::Body,
    http::{header, Response, StatusCode},
};
use bytes::Bytes;
use futures_util::StreamExt;
use polydeck_core::messages_stream::{Emit, MessagesStreamRepair};
use serde_json::Value;
use std::convert::Infallible;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, warn};

use super::SSE_STREAM_IDLE_TIMEOUT;

/// Split off the first complete SSE block in `buf`, returning its bytes.
///
/// A block ends at the first blank line. Both `\n\n` and `\r\n\r\n` terminate
/// one, so whichever *ends* earlier wins — testing `\n\n` first would split a
/// CRLF block through its own middle.
pub(super) fn take_sse_block(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    let lf = buf.windows(2).position(|w| w == b"\n\n").map(|i| i + 2);
    let crlf = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4);
    let end = match (lf, crlf) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => return None,
    };
    Some(buf.drain(..end).collect())
}

/// Read the `event:` name and `data:` payload out of one raw SSE block.
///
/// The event name is optional in SSE; Anthropic also carries a `type` in the
/// payload, so fall back to that when the field is absent.
pub(super) fn parse_sse_block(block: &[u8]) -> Option<(String, Value)> {
    let text = std::str::from_utf8(block).ok()?;
    let mut name: Option<&str> = None;
    let mut data = String::new();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("event:") {
            name = Some(v.trim());
        } else if let Some(v) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(v.trim_start());
        }
    }
    let parsed: Value = serde_json::from_str(&data).ok()?;
    let resolved = name
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .or_else(|| {
            parsed
                .get("type")
                .and_then(Value::as_str)
                .map(str::to_string)
        })?;
    Some((resolved, parsed))
}

/// Feed one raw SSE block through the repair, returning the bytes to write.
///
/// The original block is always forwarded verbatim, so a conforming upstream —
/// and any block that does not parse — reaches the client byte-for-byte.
pub(super) fn repair_sse_block(repair: &mut MessagesStreamRepair, block: Vec<u8>) -> Vec<Vec<u8>> {
    let Some((name, data)) = parse_sse_block(&block) else {
        // Distinguishes "the upstream omitted an event" from "this gateway split a
        // block through its middle". Both end as an unopened index at the client,
        // but only the second is ours to fix, so the raw bytes have to be visible.
        warn!(
            "unparseable SSE block, {} bytes: {:?}",
            block.len(),
            String::from_utf8_lossy(&block[..block.len().min(200)])
        );
        return vec![block];
    };
    match repair.observe(&name, &data) {
        Emit::Passthrough => vec![block],
        Emit::InsertBefore(inserts) => {
            let mut out: Vec<Vec<u8>> = inserts.into_iter().map(String::into_bytes).collect();
            out.push(block);
            out
        }
    }
}

/// Stream `/v1/messages` while supplying the `message_delta` Agnes omits.
///
/// Same passthrough as [`passthrough_raw_stream`], except the bytes are split on
/// SSE boundaries so [`MessagesStreamRepair`] can see the events. Without the
/// synthesised `message_delta` the client has no `stop_reason` and fails the turn
/// with `API Error: Content block not found`.
pub(super) fn passthrough_messages_stream(upstream_response: reqwest::Response) -> Response<Body> {
    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream_response
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned();
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, Infallible>>(100);
    tokio::spawn(async move {
        let mut byte_stream = upstream_response.bytes_stream();
        let mut repair = MessagesStreamRepair::new();
        let mut buf: Vec<u8> = Vec::new();
        loop {
            let next_item = tokio::time::timeout(SSE_STREAM_IDLE_TIMEOUT, byte_stream.next()).await;
            match next_item {
                Ok(Some(Ok(bytes))) => {
                    buf.extend_from_slice(&bytes);
                    while let Some(block) = take_sse_block(&mut buf) {
                        for out in repair_sse_block(&mut repair, block) {
                            if tx.send(Ok(Bytes::from(out))).await.is_err() {
                                // The client hung up mid-stream. That is what a
                                // client-side turn failure looks like from here, so
                                // the index state at that moment is the evidence.
                                warn!(
                                    "client disconnected mid-stream; orphans {:?}; opened {:?}; blocks {:?}",
                                    repair.orphan_indices(),
                                    repair.open_indices(),
                                    repair.block_types()
                                );
                                return;
                            }
                        }
                    }
                }
                // Both of these used to emit a bare `data: {"error":...}` frame,
                // which is not an Anthropic event at all — the client cannot parse
                // it, and the turn ended with no `message_stop` either way. Falling
                // through to finish_truncated below closes the turn instead, so the
                // text that did arrive survives.
                Ok(Some(Err(e))) => {
                    warn!("Upstream messages stream read error: {e}");
                    break;
                }
                Ok(None) => break,
                Err(_elapsed) => {
                    warn!("Upstream messages stream idle timeout (25s without data)");
                    break;
                }
            }
        }
        // A final block without its blank line still has to reach the client.
        if !buf.is_empty() {
            for out in repair_sse_block(&mut repair, std::mem::take(&mut buf)) {
                if tx.send(Ok(Bytes::from(out))).await.is_err() {
                    return;
                }
            }
        }
        for out in repair.finish_truncated() {
            if tx.send(Ok(Bytes::from(out))).await.is_err() {
                return;
            }
        }
        if repair.truncated() {
            warn!(
                "Messages stream ended with no message_stop; closed the turn, \
                 open blocks {:?}",
                repair.open_indices()
            );
        }
        if repair.repaired() {
            debug!("Supplied a message_delta the upstream omitted");
        }
        // An orphan index is the client's `Content block not found` condition, seen
        // from this side. Logged at warn because the client fails the whole turn.
        if !repair.orphan_indices().is_empty() {
            warn!(
                "orphan content_block indices {:?}; opened {:?}; blocks {:?}",
                repair.orphan_indices(),
                repair.open_indices(),
                repair.block_types()
            );
        } else {
            debug!(
                "content_block indices paired; opened {:?}",
                repair.open_indices()
            );
        }
    });
    Response::builder()
        .status(status)
        .header(
            header::CONTENT_TYPE,
            content_type.unwrap_or(header::HeaderValue::from_static("text/event-stream")),
        )
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .body(Body::from_stream(ReceiverStream::new(rx)))
        .unwrap()
}
