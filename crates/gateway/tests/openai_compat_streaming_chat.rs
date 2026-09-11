//! The gate on the OpenAI-compatible relay workaround.
//!
//! The verdict lives in the provider's profile (`relay_chat_compat`, written by
//! `protocol::probe`), so the gate is a pure function of that plus the base URL
//! — no process-wide state, and nothing a user has to set by hand.

use polydeck_core::types::RelayChatCompat;
use polydeck_gateway::{should_buffer_chat, uses_custom_openai_base_url};

const OFFICIAL: &str = "https://api.openai.com/v1";
const RELAY: &str = "https://relay.example.com/v1";

/// Only a probed-`Buffered` relay engages the workaround.
#[test]
fn gate_needs_both_a_relay_and_a_buffered_verdict() {
    assert!(
        should_buffer_chat(RELAY, RelayChatCompat::Buffered),
        "a relay probed as malformed must engage the workaround"
    );

    // Never probed: a working upstream must not be put behind the workaround.
    assert!(
        !should_buffer_chat(RELAY, RelayChatCompat::Auto),
        "an unprobed relay must not engage the workaround"
    );
    assert!(
        !should_buffer_chat(RELAY, RelayChatCompat::Direct),
        "a relay probed as healthy must not engage the workaround"
    );
}

/// The official service is excluded outright, whatever the profile says — its
/// non-streaming body is correct, so buffering it would only add latency.
#[test]
fn official_openai_never_engages_the_workaround() {
    for verdict in [
        RelayChatCompat::Auto,
        RelayChatCompat::Buffered,
        RelayChatCompat::Direct,
    ] {
        assert!(
            !should_buffer_chat(OFFICIAL, verdict),
            "official URL must stay on the official path under {verdict:?}"
        );
    }
}

#[test]
fn official_url_variants_are_never_relays() {
    assert!(!uses_custom_openai_base_url(OFFICIAL));
    assert!(!uses_custom_openai_base_url("https://api.openai.com/v1/"));
    assert!(uses_custom_openai_base_url(
        "https://api.openai.com/v1/beta"
    ));
    assert!(uses_custom_openai_base_url("http://api.openai.com/v1"));
    assert!(uses_custom_openai_base_url("https://relay.example.com/v1"));
}
