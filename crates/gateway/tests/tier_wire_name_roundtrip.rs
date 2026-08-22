//! Every provider model must stay independently addressable through the gateway.
//!
//! `profile_switch` decides which names Claude Code is shown and `model_rewrite`
//! decides where those names go upstream. They are separate modules that have to
//! agree, and when they disagreed the picker labelled a tier `-max` while the
//! gateway forwarded the base model. These tests drive the real hand-off:
//! `claude_wire_names` -> `TierOverrides` -> `ModelRewriter`.

use polydeck_core::profile::{ProviderConfig, RateLimitSettings};
use polydeck_core::profile_switch::{claude_tier_candidates, claude_wire_names};
use polydeck_core::types::{CodexToolCompat, ProtocolKind, ReasoningConfidence};
use polydeck_gateway::model_rewrite::{
    generate_provider_model_rewrites_with_overrides, ModelRewriter, TierOverrides,
};

fn provider(
    models: &[&str],
    opus: Option<&str>,
    sonnet: Option<&str>,
    haiku: Option<&str>,
) -> ProviderConfig {
    ProviderConfig {
        id: "prov".into(),
        name: "Relay".into(),
        base_url: "https://relay.example".into(),
        protocol: ProtocolKind::Anthropic,
        is_primary: true,
        codex_compat: CodexToolCompat::ResponsesFunction,
        reasoning_confidence: ReasoningConfidence::Unknown,
        models: models.iter().map(|s| s.to_string()).collect(),
        default_model: opus.unwrap_or("model-S").to_string(),
        accept_invalid_certs: false,
        max_price_per_request: None,
        rate_limit: RateLimitSettings::default(),
        supports_1m_context: Some(false),
        default_effort_level: None,
        opus_model: opus.map(str::to_string),
        sonnet_model: sonnet.map(str::to_string),
        haiku_model: haiku.map(str::to_string),
        opus_display_name: None,
        sonnet_display_name: None,
        haiku_display_name: None,
    }
}

/// Wire the two modules together the way `ad_gateway_start` does.
fn rewriter_for(provider: &ProviderConfig) -> ModelRewriter {
    let (opus_wire, sonnet_wire, haiku_wire) = claude_wire_names(provider);
    let (opus_model, sonnet_model, haiku_model) = claude_tier_candidates(provider);
    let rules = generate_provider_model_rewrites_with_overrides(
        &provider.models,
        provider.supports_1m_context.unwrap_or(false),
        TierOverrides {
            // Resolved here, exactly as `ad_gateway_start` does, so the rewriter
            // never falls back to a second guess of its own.
            opus_model: Some(opus_model),
            sonnet_model: Some(sonnet_model),
            haiku_model: Some(haiku_model),
            opus_display_name: Some(opus_wire),
            sonnet_display_name: Some(sonnet_wire),
            haiku_display_name: Some(haiku_wire),
        },
    );
    ModelRewriter::new(&rules).unwrap()
}

/// The reported configuration: a relay exposing effort as distinct model ids,
/// with the Opus tier pinned to `-max` and the default Opus display name
/// colliding with the relay's own `claude-opus-5`.
fn reported_provider() -> ProviderConfig {
    provider(
        &[
            "model-S",
            "model-O",
            "claude-opus-5",
            "claude-opus-5-max",
            "model-A",
            "model-T",
            "claude-opus-5-xhigh",
        ],
        Some("claude-opus-5-max"),
        Some("claude-opus-5-xhigh"),
        Some("model-T"),
    )
}

#[test]
fn every_provider_model_addresses_itself() {
    let provider = reported_provider();
    let rewriter = rewriter_for(&provider);

    for model in &provider.models {
        assert_eq!(
            &rewriter.rewrite_request(model),
            model,
            "picking '{model}' from the discovery list must reach '{model}' upstream"
        );
    }
}

#[test]
fn tier_wire_names_reach_their_configured_upstream() {
    let provider = reported_provider();
    let (opus_wire, sonnet_wire, haiku_wire) = claude_wire_names(&provider);
    let (opus_model, sonnet_model, haiku_model) = claude_tier_candidates(&provider);
    let rewriter = rewriter_for(&provider);

    // Whatever name a tier is shown under, it must land on that tier's model.
    for (tier, wire, expected) in [
        ("Opus", opus_wire, opus_model),
        ("Sonnet", sonnet_wire, sonnet_model),
        ("Haiku", haiku_wire, haiku_model),
    ] {
        assert_eq!(
            rewriter.rewrite_request(wire),
            expected,
            "{tier} is shown as '{wire}', so it must forward '{expected}'"
        );
    }

    // The Opus slot took its upstream name because the display name collided.
    assert_eq!(opus_wire, "claude-opus-5-max");
    // The collided-with model is still reachable under its own name — the whole
    // point of ceding the display name.
    assert_eq!(rewriter.rewrite_request("claude-opus-5"), "claude-opus-5");
}

#[test]
fn short_aliases_follow_the_tier_overrides() {
    let rewriter = rewriter_for(&reported_provider());
    assert_eq!(rewriter.rewrite_request("opus"), "claude-opus-5-max");
    assert_eq!(rewriter.rewrite_request("opusplan"), "claude-opus-5-max");
    assert_eq!(rewriter.rewrite_request("sonnet"), "claude-opus-5-xhigh");
    assert_eq!(rewriter.rewrite_request("default"), "claude-opus-5-xhigh");
    assert_eq!(rewriter.rewrite_request("haiku"), "model-T");
}

#[test]
fn discovery_prefixed_names_resolve_for_non_claude_models() {
    // Claude Code prefixes non-Claude ids in the picker; the router strips the
    // prefix before rewriting, so the bare name has to resolve on its own.
    let rewriter = rewriter_for(&reported_provider());
    for bare in ["model-S", "model-O", "model-A", "model-T"] {
        assert_eq!(&rewriter.rewrite_request(bare), bare);
    }
}

/// The collision rule is structural, so it has to hold for provider shapes that
/// share no naming convention with the one that exposed the bug.
///
/// Three properties are asserted for each: every advertised model addresses
/// itself, each tier's shown name reaches that tier's model, and the short
/// aliases agree with the tiers.
#[test]
fn properties_hold_across_unrelated_provider_shapes() {
    let shapes: Vec<(&str, ProviderConfig)> = vec![
        (
            "unknown names, no overrides",
            provider(&["gpt-5.6-luna", "deepseek-v4-pro-0813"], None, None, None),
        ),
        (
            "effort suffix that is not -max",
            provider(
                &["claude-opus-5", "claude-opus-5-ultra"],
                Some("claude-opus-5-ultra"),
                None,
                None,
            ),
        ),
        // Opus pinned to a model named after a *different* tier.
        (
            "tier pinned across tiers",
            provider(
                &["claude-sonnet-5", "claude-haiku-4-5"],
                Some("claude-sonnet-5"),
                None,
                None,
            ),
        ),
        // All three display names served, every tier pinned elsewhere.
        (
            "all display names collide",
            provider(
                &[
                    "claude-opus-5",
                    "claude-sonnet-5",
                    "claude-haiku-4-5",
                    "big",
                    "mid",
                    "small",
                ],
                Some("big"),
                Some("mid"),
                Some("small"),
            ),
        ),
        ("single model", provider(&["only-model"], None, None, None)),
        // The ceded name carries no tier word for the `[1m]` regexes to match.
        (
            "ceded name has no tier word",
            provider(&["claude-opus-5", "model-O"], Some("model-O"), None, None),
        ),
        // `.` and `[` must not act as regex metacharacters.
        (
            "regex metacharacters",
            provider(&["glm-4.6", "glm-4.6[1m]"], None, None, None),
        ),
        // Case-variant names are distinct upstream ids and must stay distinct.
        (
            "case-variant name",
            provider(
                &["Claude-Opus-5", "claude-opus-5-max"],
                Some("claude-opus-5-max"),
                None,
                None,
            ),
        ),
        // Irregular relay spellings, which no tier override pins down.
        (
            "reordered tier word",
            provider(&["Claude-5-opus", "Claude-5-sonnet"], None, None, None),
        ),
        (
            "decorated tier word",
            provider(&["claude-opus-5-A", "claude-sonnet-5-A"], None, None, None),
        ),
        (
            "vendor-prefixed",
            provider(
                &["anthropic/claude-opus-5", "anthropic/claude-sonnet-5"],
                None,
                None,
                None,
            ),
        ),
        (
            "dots as separators",
            provider(&["Claude.Opus.5", "Claude.Sonnet.4.5"], None, None, None),
        ),
        // Canonical name and a decorated sibling, in the order that used to break.
        (
            "canonical listed after variant",
            provider(&["claude-opus-5-A", "claude-opus-5"], None, None, None),
        ),
    ];

    for (label, p) in shapes {
        let rewriter = rewriter_for(&p);
        let (opus_wire, sonnet_wire, haiku_wire) = claude_wire_names(&p);
        let (opus_model, sonnet_model, haiku_model) = claude_tier_candidates(&p);

        for model in &p.models {
            assert_eq!(
                &rewriter.rewrite_request(model),
                model,
                "[{label}] '{model}' must address itself"
            );
        }
        for (tier, wire, expected) in [
            ("opus", opus_wire, opus_model),
            ("sonnet", sonnet_wire, sonnet_model),
            ("haiku", haiku_wire, haiku_model),
        ] {
            assert_eq!(
                rewriter.rewrite_request(wire),
                expected,
                "[{label}] {tier} shown as '{wire}' must forward '{expected}'"
            );
        }
        for (alias, expected) in [
            ("opus", opus_model),
            ("sonnet", sonnet_model),
            ("haiku", haiku_model),
        ] {
            assert_eq!(
                rewriter.rewrite_request(alias),
                expected,
                "[{label}] alias '{alias}' must forward '{expected}'"
            );
        }
    }
}

#[test]
fn a_provider_serving_no_claude_names_keeps_display_indirection() {
    // The ordinary third-party case, which must not regress: display names are
    // what let Claude Code size and price the model, so they stay in use and the
    // gateway maps them onto the real models.
    let provider = provider(&["glm-4.6", "glm-4.5-air"], None, None, Some("glm-4.5-air"));
    let rewriter = rewriter_for(&provider);
    let (opus_wire, sonnet_wire, haiku_wire) = claude_wire_names(&provider);

    assert_eq!(opus_wire, "claude-opus-5");
    assert_eq!(rewriter.rewrite_request(opus_wire), "glm-4.6");
    assert_eq!(rewriter.rewrite_request(sonnet_wire), "glm-4.6");
    assert_eq!(rewriter.rewrite_request(haiku_wire), "glm-4.5-air");
    // And the provider's own names still work.
    assert_eq!(rewriter.rewrite_request("glm-4.6"), "glm-4.6");
    assert_eq!(rewriter.rewrite_request("glm-4.5-air"), "glm-4.5-air");
}
