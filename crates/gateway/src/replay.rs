//! Whether a failed request may be replayed on a different provider.

use axum::http::HeaderMap;
use serde_json::Value;

pub const NO_REPLAY_HEADER: &str = "x-ai-deck-no-replay";
pub const IDEMPOTENCY_HEADER: &str = "idempotency-key";

const STATEFUL_FIELDS: [&str; 3] = ["store", "previous_response_id", "conversation"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayDecision {
    NeverSent,
    Idempotent,
    Unsafe(&'static str),
}

impl ReplayDecision {
    pub fn allowed(self) -> bool { !matches!(self, ReplayDecision::Unsafe(_)) }
    pub fn reason(self) -> &'static str {
        match self {
            ReplayDecision::Unsafe(reason) => reason,
            _ => "",
        }
    }
}

pub fn classify(headers: &HeaderMap, body: &Value, never_sent: bool) -> ReplayDecision {
    if never_sent { return ReplayDecision::NeverSent; }
    if header_is_set(headers, NO_REPLAY_HEADER) {
        return ReplayDecision::Unsafe("client sent x-ai-deck-no-replay");
    }
    if header_is_set(headers, IDEMPOTENCY_HEADER) {
        return ReplayDecision::Idempotent;
    }
    for field in STATEFUL_FIELDS {
        if is_stateful(body, field) {
            return ReplayDecision::Unsafe(match field {
                "store" => "request asks the provider to store the response",
                "previous_response_id" => "request continues a stored response",
                _ => "request is bound to a server-side conversation",
            });
        }
    }
    ReplayDecision::Idempotent
}

fn is_stateful(body: &Value, field: &str) -> bool {
    match body.get(field) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(enabled)) => *enabled,
        Some(Value::String(value)) => !value.is_empty(),
        Some(_) => true,
    }
}

fn header_is_set(headers: &HeaderMap, name: &str) -> bool {
    match headers.get(name).and_then(|v| v.to_str().ok()) {
        Some(value) => {
            let value = value.trim();
            !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use serde_json::json;

    #[allow(dead_code)]
    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_str(value).unwrap());
        }
        map
    }

    #[test]
    fn never_sent_is_always_replayable() {
        let decision = classify(&HeaderMap::new(), &json!({"store": true}), true);
        assert_eq!(decision, ReplayDecision::NeverSent);
        assert!(decision.allowed());
    }

    #[test]
    fn plain_completion_is_idempotent() {
        let body = json!({"model": "m", "messages": [{"role": "user", "content": "hi"}]});
        assert_eq!(classify(&HeaderMap::new(), &body, false), ReplayDecision::Idempotent);
    }

    #[test]
    fn storing_blocks_replay() {
        let decision = classify(&HeaderMap::new(), &json!({"store": true}), false);
        assert!(!decision.allowed());
    }

    #[test]
    fn store_false_does_not_block() {
        assert_eq!(classify(&HeaderMap::new(), &json!({"store": false}), false), ReplayDecision::Idempotent);
    }
}
