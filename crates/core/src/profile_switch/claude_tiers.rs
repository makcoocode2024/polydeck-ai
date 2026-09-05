//! Resolving Claude Code's opus/sonnet/haiku tiers onto real upstream models.
//!
//! Claude Code addresses models through three fixed aliases and infers context
//! window, price, and feature set from the *name* it is shown, falling back to a
//! 200K unknown-model profile for anything it does not recognise. So each tier
//! needs both a wire name the upstream serves and a display name Claude Code
//! recognises, and the two are not always the same value.

use std::collections::HashSet;

pub const CLAUDE_CODE_SONNET_ALIASES: &[&str] = &["sonnet", "default", "claude-sonnet"];
pub const CLAUDE_CODE_OPUS_ALIASES: &[&str] = &["opus", "opusplan", "claude-opus"];
pub const CLAUDE_CODE_HAIKU_ALIASES: &[&str] = &["haiku", "claude-haiku"];

// Names Claude Code is shown for each tier when the provider does not override
// them. They have to be current built-in Anthropic IDs: Claude Code decides a
// model's context window, price and feature set from the name, and falls back to
// a 200K unknown-model profile for anything it does not recognise.
//
// Bump these when Anthropic ships a new generation. Fable and Mythos are absent
// on purpose — Claude Code has no alias tier for them.
pub const DEFAULT_OPUS_DISPLAY_NAME: &str = "claude-opus-5";
pub const DEFAULT_SONNET_DISPLAY_NAME: &str = "claude-sonnet-5";
/// Haiku 4.5 caps at 200K, so this tier has no `[1m]` form.
pub const DEFAULT_HAIKU_DISPLAY_NAME: &str = "claude-haiku-4-5";

/// Point every `keys` entry at `wire` in a `modelOverrides` map.
///
/// A `[1m]` key keeps its suffix only when `tier_supports_1m`; otherwise it
/// collapses onto the plain wire name, since asking for a context window the
/// upstream cannot serve fails the request outright.
pub(super) fn insert_tier(
    overrides: &mut serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
    wire: &str,
    tier_supports_1m: bool,
) {
    let wire_1m = format!("{wire}[1m]");
    for key in keys {
        let value = if key.ends_with("[1m]") && tier_supports_1m && !wire.ends_with("[1m]") {
            wire_1m.as_str()
        } else {
            wire
        };
        overrides.insert(
            (*key).to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
}

/// Trimmed `value`, or `fallback` when it is absent or blank.
fn trimmed_or<'a>(value: Option<&'a str>, fallback: &'a str) -> &'a str {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
}

/// The `(opus, sonnet, haiku)` names Claude Code is shown for this provider.
///
/// The gateway has to resolve the same names this module writes into
/// `~/.claude.json`, so both sides read them from here.
pub fn claude_display_names(provider: &crate::profile::ProviderConfig) -> (&str, &str, &str) {
    (
        trimmed_or(
            provider.opus_display_name.as_deref(),
            DEFAULT_OPUS_DISPLAY_NAME,
        ),
        trimmed_or(
            provider.sonnet_display_name.as_deref(),
            DEFAULT_SONNET_DISPLAY_NAME,
        ),
        trimmed_or(
            provider.haiku_display_name.as_deref(),
            DEFAULT_HAIKU_DISPLAY_NAME,
        ),
    )
}

/// The model a client should default to for this provider.
///
/// Shared so the tier resolution below and `write_claude_config` cannot drift
/// apart on which model counts as the default.
pub(super) fn claude_default_model(provider: &crate::profile::ProviderConfig) -> &str {
    if !provider.default_model.trim().is_empty() {
        provider.default_model.trim()
    } else if let Some(first) = provider.models.first().filter(|s| !s.trim().is_empty()) {
        first.trim()
    } else {
        // Last-resort fallback: use the generic "sonnet" alias rather than a
        // pinned retired ID like claude-3-7-sonnet-latest, which would make
        // Claude Code show a retirement banner.
        "sonnet"
    }
}

/// Pick the model that best serves a tier among those matching it.
///
/// Relays decorate names freely (`claude-opus-5-A`, `Claude-5-opus-preview`,
/// `Claude-Opus-5-thinking`), so a catalog often matches one tier several times.
/// Taking the first match made the answer depend on catalog order, which then
/// decided whether the tier could keep its canonical display name — the same two
/// models in the other order produced a different picker label.
///
/// So prefer, in order: the tier's canonical name exactly, then the least
/// decorated match (shortest, ties by catalog order). The plain `claude-opus-5`
/// wins over `claude-opus-5-A` however the relay lists them, and a decorated
/// name is still picked when it is all there is.
fn pick_tier_model<'a>(
    models: &'a [String],
    canonical: &str,
    matches_tier: impl Fn(&str) -> bool,
) -> Option<&'a str> {
    let candidates: Vec<&'a str> = models
        .iter()
        .map(|m| m.trim())
        .filter(|m| !m.is_empty() && matches_tier(&m.to_ascii_lowercase()))
        .collect();

    candidates
        .iter()
        .find(|m| m.eq_ignore_ascii_case(canonical))
        .or_else(|| candidates.iter().min_by_key(|m| m.len()))
        .copied()
}

/// The `(opus, sonnet, haiku)` provider models that actually serve each Claude
/// Code tier: the explicit `*_model` override when set, else a name-based guess.
///
/// The guess is keyword-based and case-insensitive, so irregular relay spellings
/// (`Claude-5-opus`, `claude-opus-5-A`, `anthropic/claude-opus-5`) still land on
/// the right tier. A catalog whose names carry no tier word at all cannot be
/// guessed — every tier then falls back to one model, and the `*_model` fields
/// are the only way to spread the tiers out.
pub fn claude_tier_candidates(provider: &crate::profile::ProviderConfig) -> (&str, &str, &str) {
    let model_to_use = claude_default_model(provider);

    let sonnet = provider
        .sonnet_model
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            pick_tier_model(&provider.models, DEFAULT_SONNET_DISPLAY_NAME, |lower| {
                lower.contains("sonnet")
                    || lower.contains("claude-3-7")
                    || lower.contains("claude-3.7")
            })
            .unwrap_or_else(|| {
                if model_to_use.to_ascii_lowercase().contains("sonnet") {
                    model_to_use
                } else {
                    provider
                        .models
                        .first()
                        .map(|s| s.as_str())
                        .unwrap_or(model_to_use)
                }
            })
        });

    let opus = provider
        .opus_model
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            pick_tier_model(&provider.models, DEFAULT_OPUS_DISPLAY_NAME, |lower| {
                lower.contains("opus")
            })
            .unwrap_or(sonnet)
        });

    let haiku = provider
        .haiku_model
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            pick_tier_model(&provider.models, DEFAULT_HAIKU_DISPLAY_NAME, |lower| {
                lower.contains("haiku") || lower.contains("flash") || lower.contains("mini")
            })
            .unwrap_or(sonnet)
        });

    (opus, sonnet, haiku)
}

/// The `(opus, sonnet, haiku)` names that travel on the wire when the gateway is
/// in front, which are also the names Claude Code shows.
///
/// Normally this is the display name: it has to be a built-in Anthropic ID or
/// Claude Code cannot size or price the model, and the gateway maps it back to
/// the provider's real model.
///
/// A display name that collides with a *different* provider model is the one
/// case where that breaks down. Redirecting it would make the collided-with
/// model unreachable — nobody could address the real `claude-opus-5` once it
/// resolves to `claude-opus-5-max` — so this tier carries its upstream name
/// instead. That name is one the provider serves, so the gateway's own
/// passthrough rule routes it, the picker label matches what runs, and every
/// provider model stays independently addressable.
///
/// The rule is structural, not tied to any naming convention: it asks only
/// whether the shown name would shadow a different model, so `-max`, `-ultra`,
/// `:max` and names with no tier word behave the same way.
///
/// Note this also overrides an *explicitly configured* display name when that
/// name collides. Keeping it would strand the model it shadows, and a display
/// name pointing at another of the provider's own models is a misconfiguration
/// either way — but it does mean the setting is silently not honored.
///
/// The gateway has to resolve the same names this module writes, so both sides
/// read them from here.
pub fn claude_wire_names<'a>(
    provider: &'a crate::profile::ProviderConfig,
) -> (&'a str, &'a str, &'a str) {
    let (opus_display, sonnet_display, haiku_display) = claude_display_names(provider);
    let (opus_candidate, sonnet_candidate, haiku_candidate) = claude_tier_candidates(provider);
    let served: HashSet<&str> = provider.models.iter().map(|m| m.trim()).collect();

    let resolve = |display: &'a str, candidate: &'a str| -> &'a str {
        if display != candidate && served.contains(display) {
            candidate
        } else {
            display
        }
    };

    (
        resolve(opus_display, opus_candidate),
        resolve(sonnet_display, sonnet_candidate),
        resolve(haiku_display, haiku_candidate),
    )
}

/// Warnings worth surfacing when a profile is activated, about how its tier
/// wiring actually resolves.
///
/// Two silent failure modes get a message here:
/// - Two tiers landing on one provider model, at least one of them guessed.
///   A catalog with no tier words in its names hands every tier the same
///   fallback, and nothing in the UI says the tiers are not actually spread
///   out. Both explicitly pinned to one model is the user's own choice and
///   stays quiet.
/// - An explicit `*_display_name` that `claude_wire_names` had to override
///   because it collided with a different provider model. The setting is
///   silently not honored otherwise.
pub fn claude_tier_warnings(provider: &crate::profile::ProviderConfig) -> Vec<String> {
    let (opus, sonnet, haiku) = claude_tier_candidates(provider);
    let explicitly_set = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some()
    };

    let mut warnings = Vec::new();

    // Collapsed tiers, one message per model they share.
    let mut by_model: Vec<(&str, Vec<(&str, bool)>)> = Vec::new();
    for (label, model, is_explicit) in [
        ("Opus", opus, explicitly_set(&provider.opus_model)),
        ("Sonnet", sonnet, explicitly_set(&provider.sonnet_model)),
        ("Haiku", haiku, explicitly_set(&provider.haiku_model)),
    ] {
        match by_model.iter_mut().find(|(m, _)| *m == model) {
            Some((_, labels)) => labels.push((label, is_explicit)),
            None => by_model.push((model, vec![(label, is_explicit)])),
        }
    }
    for (model, labels) in by_model {
        if labels.len() > 1 && labels.iter().any(|(_, is_explicit)| !is_explicit) {
            let names = labels
                .iter()
                .map(|(label, _)| *label)
                .collect::<Vec<_>>()
                .join("、");
            warnings.push(format!(
                "档位 {names} 都解析到模型 {model}，分档未生效；若目录里没有能区分档位的模型名，请在配置中分别指定 opus_model / sonnet_model / haiku_model"
            ));
        }
    }

    // Explicit display names the wire path had to give up because they collided
    // with another provider model.
    let (opus_display, sonnet_display, haiku_display) = claude_display_names(provider);
    let (opus_wire, sonnet_wire, haiku_wire) = claude_wire_names(provider);
    for (label, display, wire, is_explicit) in [
        (
            "Opus",
            opus_display,
            opus_wire,
            explicitly_set(&provider.opus_display_name),
        ),
        (
            "Sonnet",
            sonnet_display,
            sonnet_wire,
            explicitly_set(&provider.sonnet_display_name),
        ),
        (
            "Haiku",
            haiku_display,
            haiku_wire,
            explicitly_set(&provider.haiku_display_name),
        ),
    ] {
        if is_explicit && display != wire {
            warnings.push(format!(
                "{label} 档位的显示名 {display} 与提供方另一个模型重名，为避免它不可寻址，已改用上游名 {wire} 展示"
            ));
        }
    }

    warnings
}

/// Strip a trailing `/v1` (and any trailing slashes) from an Anthropic base URL.
///
/// Claude Code talks to Anthropic-shaped endpoints through the official SDK,
/// which appends the `/v1/...` path segment on its own. Keeping a `/v1` suffix
/// in `ANTHROPIC_BASE_URL` makes it request `/v1/v1/messages`, which 404s.
pub(super) fn strip_anthropic_version_suffix(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    match trimmed.strip_suffix("/v1") {
        Some(stripped) => stripped.trim_end_matches('/').to_string(),
        None => trimmed.to_string(),
    }
}
