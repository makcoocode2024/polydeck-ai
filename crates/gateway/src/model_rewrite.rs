//! Model name rewriting engine

use crate::config::{MatchKind, ModelRewriteRule};
use std::collections::HashMap;

/// One-way client-model → upstream-model mapping.
///
/// Deliberately has no reverse direction. Many client names collapse onto one
/// upstream model, so a reverse table can only guess, and it used to guess a
/// retired Claude name — which `/resume` then wrote back into the session config
/// and Claude Code flagged as deprecated. Responses instead echo the exact model
/// string the client sent, which the router already has on hand.
#[derive(Debug, Clone)]
pub struct ModelRewriter {
    exact_rules: HashMap<String, String>,
    regex_rules: Vec<(regex::Regex, String)>,
}

impl ModelRewriter {
    pub fn new(rules: &[ModelRewriteRule]) -> Result<Self, String> {
        let mut exact_rules = HashMap::new();
        let mut regex_rules = Vec::new();

        for rule in rules.iter().filter(|r| r.enabled) {
            let as_regex = match rule.match_kind {
                MatchKind::Literal => false,
                MatchKind::Regex => true,
                // Legacy hand-written configs carry no kind, so fall back to
                // guessing. Model names like `claude-opus-5[1m]` and `glm-4.6`
                // guess wrong, which is why generated rules always state a kind.
                MatchKind::Auto => !is_literal_pattern(&rule.from),
            };

            let compiled = if as_regex {
                regex::Regex::new(&rule.from).ok()
            } else {
                None
            };
            match compiled {
                Some(re) => regex_rules.push((re, rule.to.clone())),
                // An uncompilable pattern is still usable as a literal; keeping
                // it beats dropping the rule silently.
                None => {
                    exact_rules.insert(rule.from.clone(), rule.to.clone());
                }
            }
        }

        Ok(Self {
            exact_rules,
            regex_rules,
        })
    }

    pub fn rewrite_request(&self, model: &str) -> String {
        if let Some(rewritten) = self.exact_rules.get(model) {
            return rewritten.clone();
        }
        for (pattern, replacement) in &self.regex_rules {
            if pattern.is_match(model) {
                return pattern.replace(model, replacement.as_str()).to_string();
            }
        }
        model.to_string()
    }
}

/// Per-tier configuration for [`generate_provider_model_rewrites_with_overrides`].
///
/// `*_model` pick which provider model serves a tier, overriding the automatic
/// name-based guess. `*_display_name` are the names Claude Code was told to use
/// (see `polydeck_core::profile_switch`); they arrive on the wire and have to be
/// mapped back, which is the whole reason a display name can differ from the
/// provider's own model name.
///
/// Callers that resolve the tiers themselves — the live gateway does, via
/// `profile_switch::claude_tier_candidates` — should pass `*_model` for every
/// tier. The guess below is a fallback, and it is deliberately cruder than that
/// function; letting both run on the same provider is how the picker label and
/// the routing end up on different models.
#[derive(Debug, Clone, Copy, Default)]
pub struct TierOverrides<'a> {
    pub sonnet_model: Option<&'a str>,
    pub opus_model: Option<&'a str>,
    pub haiku_model: Option<&'a str>,
    pub sonnet_display_name: Option<&'a str>,
    pub opus_display_name: Option<&'a str>,
    pub haiku_display_name: Option<&'a str>,
}

/// Generate standard model rewrite rules for a provider's model list.
/// This ensures all aliases (opus, sonnet, haiku, default, opusplan) and standard Claude
/// model names map to the best candidates among available provider models.
pub fn generate_provider_model_rewrites(
    models: &[String],
    supports_1m: bool,
) -> Vec<ModelRewriteRule> {
    generate_provider_model_rewrites_with_overrides(models, supports_1m, TierOverrides::default())
}

pub fn generate_provider_model_rewrites_with_overrides(
    models: &[String],
    supports_1m: bool,
    tiers: TierOverrides<'_>,
) -> Vec<ModelRewriteRule> {
    let (custom_sonnet, custom_opus, custom_haiku) =
        (tiers.sonnet_model, tiers.opus_model, tiers.haiku_model);
    let mut rules = Vec::new();
    if models.is_empty() {
        return rules;
    }

    let default_candidate = models.first().map(|s| s.as_str()).unwrap_or("gpt-4o");

    let auto_sonnet = models
        .iter()
        .find(|m| {
            let lower = m.to_ascii_lowercase();
            lower.contains("sonnet") || lower.contains("claude-3-7") || lower.contains("claude-3.7")
        })
        .map(|s| s.as_str())
        .unwrap_or(default_candidate);

    let auto_opus = models
        .iter()
        .find(|m| m.to_ascii_lowercase().contains("opus"))
        .map(|s| s.as_str())
        .unwrap_or(auto_sonnet);

    let auto_haiku = models
        .iter()
        .find(|m| {
            let lower = m.to_ascii_lowercase();
            lower.contains("haiku") || lower.contains("flash") || lower.contains("mini")
        })
        .map(|s| s.as_str())
        .unwrap_or(auto_sonnet);

    let sonnet_candidate = custom_sonnet
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(auto_sonnet);
    let opus_candidate = custom_opus
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(auto_opus);
    let haiku_candidate = custom_haiku
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(auto_haiku);

    // 1. Self-mapping for all available provider models (exact rules)
    for m in models {
        let trimmed = m.trim();
        if !trimmed.is_empty() {
            rules.push(
                ModelRewriteRule::exact(trimmed, trimmed)
                    .with_description("Provider model passthrough"),
            );
        }
    }

    // 2. Short aliases
    let sonnet_aliases = ["sonnet", "default", "claude-sonnet"];
    for alias in sonnet_aliases {
        rules.push(
            ModelRewriteRule::exact(alias, sonnet_candidate)
                .with_description("Sonnet alias mapping"),
        );
    }

    let opus_aliases = ["opus", "opusplan", "claude-opus"];
    for alias in opus_aliases {
        rules.push(
            ModelRewriteRule::exact(alias, opus_candidate).with_description("Opus alias mapping"),
        );
    }

    let haiku_aliases = ["haiku", "claude-haiku"];
    for alias in haiku_aliases {
        rules.push(
            ModelRewriteRule::exact(alias, haiku_candidate).with_description("Haiku alias mapping"),
        );
    }

    // 3. Known full Claude model names (Sonnet, Opus, Haiku)
    let sonnet_full = [
        "claude-3-5-sonnet",
        "claude-3-5-sonnet-20240620",
        "claude-3-5-sonnet-20241022",
        "claude-3-5-sonnet-latest",
        "claude-3-7-sonnet",
        "claude-3-7-sonnet-20250219",
        "claude-3-7-sonnet-latest",
        "claude-sonnet-4-5",
        "claude-sonnet-4-5-20250929",
        "claude-sonnet-4-6",
        "claude-sonnet-5",
    ];
    let provider_model_set: std::collections::HashSet<&str> =
        models.iter().map(|s| s.trim()).collect();

    for name in sonnet_full {
        if !provider_model_set.contains(name) {
            rules.push(
                ModelRewriteRule::exact(name, sonnet_candidate)
                    .with_description("Sonnet full model mapping"),
            );
        }
    }

    let opus_full = [
        "claude-3-opus",
        "claude-3-opus-20240229",
        "claude-3-opus-latest",
        "claude-opus-4-5",
        "claude-opus-4-5-20251101",
        "claude-opus-4-6",
        "claude-opus-4-7",
        "claude-opus-4-8",
        "claude-opus-5",
        "claude-opus-5-max",
        "claude-opus-5-xhigh",
    ];
    for name in opus_full {
        if !provider_model_set.contains(name) {
            rules.push(
                ModelRewriteRule::exact(name, opus_candidate)
                    .with_description("Opus full model mapping"),
            );
        }
    }

    let haiku_full = [
        "claude-3-haiku",
        "claude-3-haiku-20240307",
        "claude-3-5-haiku",
        "claude-3-5-haiku-20241022",
        "claude-3-5-haiku-latest",
        "claude-haiku-4-5",
        "claude-haiku-4-5-20251001",
        "claude-haiku-4-5-20251001-v1",
    ];
    for name in haiku_full {
        if !provider_model_set.contains(name) {
            rules.push(
                ModelRewriteRule::exact(name, haiku_candidate)
                    .with_description("Haiku full model mapping"),
            );
        }
    }

    // 3b. Display names Claude Code was configured with.
    //
    // These come last among the exact rules so they win: a display name may well
    // be one of the built-in names above, and it must resolve to its own tier
    // rather than to whichever tier that name looks like it belongs to.
    for (tier, display, candidate) in [
        ("Sonnet", tiers.sonnet_display_name, sonnet_candidate),
        ("Opus", tiers.opus_display_name, opus_candidate),
        ("Haiku", tiers.haiku_display_name, haiku_candidate),
    ] {
        let Some(display) = display.map(str::trim).filter(|d| !d.is_empty()) else {
            continue;
        };
        if display == candidate {
            // Already the tier's own model; the step-1 self-map says the same
            // thing. Testing that rather than mere membership in the provider's
            // list matters: a display name that is *some other* provider model
            // still has to be redirected here, or step 1's self-map wins and the
            // tier silently serves the wrong model.
            continue;
        }
        rules.push(
            ModelRewriteRule::exact(display, candidate)
                .with_description(format!("{tier} display name mapping")),
        );
        // A display name carrying no tier word (`my-big-model`) would miss the
        // regex in step 4, so spell its `[1m]` form out here.
        if !display.ends_with("[1m]") {
            let target = resolve_1m_target(candidate, models, supports_1m);
            rules.push(
                ModelRewriteRule::exact(
                    format!("{display}[1m]"),
                    target.as_deref().unwrap_or(candidate),
                )
                .with_description(format!("{tier} display name 1M mapping")),
            );
        }
    }

    // 4. `[1m]` suffix routing.
    //
    // Claude Code appends `[1m]` to ask for the 1M context window. Upstream only
    // understands the suffix if the provider advertises a `[1m]` name, so route
    // each tier to its advertised 1M model when there is one and drop the suffix
    // otherwise. Provider names that already carry `[1m]` pass through untouched
    // via the exact self-maps in step 1.
    for (tier, candidate) in [
        ("opus", opus_candidate),
        ("sonnet", sonnet_candidate),
        ("haiku", haiku_candidate),
    ] {
        let target = resolve_1m_target(candidate, models, supports_1m);
        let to = target.as_deref().unwrap_or(candidate);
        rules.push(
            ModelRewriteRule::regex(format!(r"(?i)^claude-.*{tier}.*\[1m\]$"), to)
                .with_description(format!("{tier} 1M mapping")),
        );
    }

    rules.push(
        ModelRewriteRule::regex(r"^(.+?)\[1m\]$", "$1")
            .with_description("Strip [1m] suffix for unsupported upstream"),
    );

    // 5. Regex catch-all fallback rules for unseen variants
    rules.push(
        ModelRewriteRule::regex(r"(?i)^claude-.*opus.*", opus_candidate)
            .with_description("Catch-all Opus pattern"),
    );
    rules.push(
        ModelRewriteRule::regex(r"(?i)^claude-.*sonnet.*", sonnet_candidate)
            .with_description("Catch-all Sonnet pattern"),
    );
    rules.push(
        ModelRewriteRule::regex(r"(?i)^claude-.*haiku.*", haiku_candidate)
            .with_description("Catch-all Haiku pattern"),
    );

    rules
}

/// Find the upstream name that carries a 1M context window for `candidate`.
///
/// Returns `None` when the suffix has to be dropped, either because the provider
/// is not declared 1M-capable or because it advertises no `[1m]` variant of this
/// candidate. A name is never invented: sending an unadvertised `foo[1m]`
/// upstream would just 400.
fn resolve_1m_target(candidate: &str, models: &[String], supports_1m: bool) -> Option<String> {
    if candidate.ends_with("[1m]") {
        return Some(candidate.to_string());
    }
    if !supports_1m {
        return None;
    }
    let suffixed = format!("{candidate}[1m]");
    models
        .iter()
        .any(|m| m.trim() == suffixed)
        .then_some(suffixed)
}

fn is_literal_pattern(pattern: &str) -> bool {
    !pattern.chars().any(|c| {
        matches!(
            c,
            '.' | '*' | '+' | '?' | '[' | ']' | '(' | ')' | '{' | '}' | '^' | '$' | '|' | '\\'
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Mirrors a legacy hand-written rule: no `match_kind`, so it exercises the
    /// `Auto` heuristic rather than the explicit dispatch.
    fn make_rule(from: &str, to: &str) -> ModelRewriteRule {
        ModelRewriteRule {
            from: from.to_string(),
            to: to.to_string(),
            enabled: true,
            description: None,
            match_kind: MatchKind::Auto,
        }
    }

    #[test]
    fn exact_match_rewrite() {
        let rules = vec![
            make_rule("claude-sonnet-4-5", "glm-5.2"),
            make_rule("gpt-4o", "qwen-max"),
        ];
        let rewriter = ModelRewriter::new(&rules).unwrap();
        assert_eq!(rewriter.rewrite_request("claude-sonnet-4-5"), "glm-5.2");
        assert_eq!(rewriter.rewrite_request("gpt-4o"), "qwen-max");
        assert_eq!(rewriter.rewrite_request("unknown-model"), "unknown-model");
    }

    #[test]
    fn empty_rule_set_is_identity() {
        let rewriter = ModelRewriter::new(&[]).unwrap();
        assert_eq!(rewriter.rewrite_request("gpt-4o"), "gpt-4o");
    }

    #[test]
    fn disabled_rule_ignored() {
        let mut rule = make_rule("model-a", "model-b");
        rule.enabled = false;
        let rewriter = ModelRewriter::new(&[rule]).unwrap();
        assert_eq!(rewriter.rewrite_request("model-a"), "model-a");
    }

    #[test]
    fn test_generated_provider_model_rewrites() {
        let models = vec![
            "model-S".to_string(),
            "model-O".to_string(),
            "claude-opus-5".to_string(),
        ];
        let rules = generate_provider_model_rewrites(&models, false);
        let rewriter = ModelRewriter::new(&rules).unwrap();

        // 1. Short aliases
        assert_eq!(rewriter.rewrite_request("opus"), "claude-opus-5");
        assert_eq!(rewriter.rewrite_request("opusplan"), "claude-opus-5");
        assert_eq!(rewriter.rewrite_request("sonnet"), "model-S");
        assert_eq!(rewriter.rewrite_request("default"), "model-S");
        assert_eq!(rewriter.rewrite_request("haiku"), "model-S");

        // 2. Full names
        assert_eq!(
            rewriter.rewrite_request("claude-3-7-sonnet-20250219"),
            "model-S"
        );
        assert_eq!(
            rewriter.rewrite_request("claude-3-opus-20240229"),
            "claude-opus-5"
        );
        assert_eq!(
            rewriter.rewrite_request("claude-3-5-haiku-latest"),
            "model-S"
        );

        // 3. Provider models passthrough
        assert_eq!(rewriter.rewrite_request("model-S"), "model-S");
        assert_eq!(rewriter.rewrite_request("model-O"), "model-O");
        assert_eq!(rewriter.rewrite_request("claude-opus-5"), "claude-opus-5");

        // 4. Suffix [1m] stripped when supports_1m is false
        assert_eq!(rewriter.rewrite_request("claude-sonnet-4-5[1m]"), "model-S");
        assert_eq!(
            rewriter.rewrite_request("claude-opus-5[1m]"),
            "claude-opus-5"
        );
    }

    #[test]
    fn display_names_resolve_to_their_own_tier() {
        let models = vec!["glm-4.6".to_string(), "glm-4.5-air".to_string()];
        let tiers = TierOverrides {
            haiku_model: Some("glm-4.5-air"),
            // `claude-opus-5` is also in the built-in *sonnet-adjacent* name
            // lists; as a display name it must still land on the Opus tier.
            opus_display_name: Some("claude-opus-5"),
            sonnet_display_name: Some("my-big-model"),
            haiku_display_name: Some("claude-haiku-4-5"),
            ..Default::default()
        };
        let rules = generate_provider_model_rewrites_with_overrides(&models, false, tiers);
        let rewriter = ModelRewriter::new(&rules).unwrap();

        assert_eq!(rewriter.rewrite_request("claude-opus-5"), "glm-4.6");
        assert_eq!(rewriter.rewrite_request("claude-haiku-4-5"), "glm-4.5-air");
        // A display name with no tier word still resolves, which the catch-all
        // regexes could never do for it.
        assert_eq!(rewriter.rewrite_request("my-big-model"), "glm-4.6");
        assert_eq!(rewriter.rewrite_request("my-big-model[1m]"), "glm-4.6");
    }

    #[test]
    fn display_name_1m_reaches_the_advertised_variant() {
        let models = vec!["glm-4.6".to_string(), "glm-4.6[1m]".to_string()];
        let tiers = TierOverrides {
            opus_display_name: Some("my-big-model"),
            ..Default::default()
        };
        let rules = generate_provider_model_rewrites_with_overrides(&models, true, tiers);
        let rewriter = ModelRewriter::new(&rules).unwrap();
        assert_eq!(rewriter.rewrite_request("my-big-model[1m]"), "glm-4.6[1m]");
        assert_eq!(rewriter.rewrite_request("my-big-model"), "glm-4.6");
    }

    #[test]
    fn display_name_that_is_its_own_tier_model_passes_through() {
        let models = vec!["claude-opus-5".to_string()];
        let tiers = TierOverrides {
            opus_display_name: Some("claude-opus-5"),
            ..Default::default()
        };
        let rules = generate_provider_model_rewrites_with_overrides(&models, false, tiers);
        let rewriter = ModelRewriter::new(&rules).unwrap();
        assert_eq!(rewriter.rewrite_request("claude-opus-5"), "claude-opus-5");
    }

    /// A display name that is *another* provider model must still be redirected.
    ///
    /// Skipping on mere membership in the provider's list left step 1's self-map
    /// in charge, so an Opus tier explicitly pinned to `-max` quietly served the
    /// base model while the picker label promised `-max`.
    #[test]
    fn display_name_colliding_with_another_model_still_redirects() {
        let models = vec!["claude-opus-5".to_string(), "claude-opus-5-max".to_string()];
        let tiers = TierOverrides {
            opus_model: Some("claude-opus-5-max"),
            opus_display_name: Some("claude-opus-5"),
            ..Default::default()
        };
        let rules = generate_provider_model_rewrites_with_overrides(&models, false, tiers);
        let rewriter = ModelRewriter::new(&rules).unwrap();
        assert_eq!(
            rewriter.rewrite_request("claude-opus-5"),
            "claude-opus-5-max"
        );
        // The pinned model still addresses itself.
        assert_eq!(
            rewriter.rewrite_request("claude-opus-5-max"),
            "claude-opus-5-max"
        );
    }

    #[test]
    fn names_with_regex_metacharacters_match_literally() {
        let models = vec!["glm-4.6".to_string()];
        let rewriter =
            ModelRewriter::new(&generate_provider_model_rewrites(&models, false)).unwrap();
        assert_eq!(rewriter.rewrite_request("glm-4.6"), "glm-4.6");
        // `.` must not act as a wildcard.
        assert_eq!(rewriter.rewrite_request("glm-4x6"), "glm-4x6");
    }

    #[test]
    fn advertised_1m_name_passes_through() {
        let models = vec!["claude-opus-5".to_string(), "claude-opus-5[1m]".to_string()];
        let rewriter =
            ModelRewriter::new(&generate_provider_model_rewrites(&models, true)).unwrap();
        assert_eq!(
            rewriter.rewrite_request("claude-opus-5[1m]"),
            "claude-opus-5[1m]"
        );
        assert_eq!(rewriter.rewrite_request("claude-opus-5"), "claude-opus-5");
    }

    #[test]
    fn tier_1m_request_routes_to_advertised_variant() {
        let models = vec!["glm-4.6".to_string(), "glm-4.6[1m]".to_string()];
        let rewriter =
            ModelRewriter::new(&generate_provider_model_rewrites(&models, true)).unwrap();
        assert_eq!(
            rewriter.rewrite_request("claude-opus-4-5[1m]"),
            "glm-4.6[1m]"
        );
        assert_eq!(rewriter.rewrite_request("claude-opus-4-5"), "glm-4.6");
    }

    #[test]
    fn unadvertised_1m_suffix_is_stripped() {
        let models = vec!["glm-4.6".to_string()];
        let rewriter =
            ModelRewriter::new(&generate_provider_model_rewrites(&models, true)).unwrap();
        assert_eq!(rewriter.rewrite_request("claude-opus-4-5[1m]"), "glm-4.6");
        // Non-Claude names still lose a suffix upstream cannot parse.
        assert_eq!(rewriter.rewrite_request("some-model[1m]"), "some-model");
    }

    #[test]
    fn many_client_names_may_share_one_upstream_model() {
        // Collapsing is expected and is why there is no reverse map: `model-S`
        // alone cannot tell you which of these the client asked for.
        let rules = vec![
            make_rule("sonnet", "model-S"),
            make_rule("claude-3-7-sonnet", "model-S"),
        ];
        let rewriter = ModelRewriter::new(&rules).unwrap();
        assert_eq!(rewriter.rewrite_request("sonnet"), "model-S");
        assert_eq!(rewriter.rewrite_request("claude-3-7-sonnet"), "model-S");
    }
}
