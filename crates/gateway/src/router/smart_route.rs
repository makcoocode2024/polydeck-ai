//! Smart route engine — the decision layer behind [`crate::router::route_model`].
//!
//! The decision order is a contract (see `polydeck_core::smart_route` for the
//! numbered list). This module only reads the request body to classify it; it
//! never rewrites anything except `model`.

use std::sync::Mutex;

use polydeck_core::smart_route::{
    CategoryRouteMap, SmartRouteSettings, COMPLEX_CORE_KEYWORDS, VISUAL_FRONTEND_KEYWORDS,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Which protocol dialect the body speaks. Feature extraction adapts to this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyShape {
    Messages,
    Responses,
    ChatCompletions,
}

/// Why a request was routed the way it was. Every decision must carry one —
/// the audit log and the simulator both surface it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteReason {
    ForceModel,
    ModelTag,
    CustomRule,
    Category(String),
    DefaultModel,
    Passthrough,
    ValidationFallback,
    FallbackRetry,
}

impl RouteReason {
    pub fn as_str(&self) -> &str {
        match self {
            Self::ForceModel => "force_model",
            Self::ModelTag => "model_tag",
            Self::CustomRule => "custom_rule",
            Self::Category(name) => name,
            Self::DefaultModel => "default_model",
            Self::Passthrough => "passthrough",
            Self::ValidationFallback => "validation_fallback",
            Self::FallbackRetry => "fallback_retry",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteDecision {
    pub routed_model: String,
    pub reason: RouteReason,
    pub match_rule: Option<String>,
    pub is_tag_hit: bool,
    pub validation_warn: Option<String>,
}

enum CompiledRule {
    Literal {
        id: String,
        keyword: String,
        target: String,
    },
    Regex {
        id: String,
        regex: regex::Regex,
        target: String,
    },
}

/// Text/image/effort features gathered from the request body, shape-dependent.
#[derive(Debug, Default)]
struct ContentFeatures {
    text: String,
    has_image: bool,
    effort_max: bool,
}

/// Budget for feature extraction: enough to catch the system prompt, recent
/// turns and a `#MODEL:` tag without re-scanning megabyte-long histories for
/// every rule on every request.
const MAX_TEXT_CHARS: usize = 24_000;
const MAX_SEGMENT_CHARS: usize = 4_000;
/// System prompt plus the head and tail of the conversation.
const HEAD_MESSAGES: usize = 2;
const TAIL_MESSAGES: usize = 6;
/// Requests at least this big are treated as core-business work by default.
const COMPLEX_TOKEN_THRESHOLD: u64 = 64_000;
/// effort=max on the Messages protocol effectively means this many thinking
/// tokens or more; `budget_tokens` at or above it counts as max.
const EFFORT_MAX_BUDGET_TOKENS: u64 = 63_999;

pub struct SmartRouteEngine {
    cfg: SmartRouteSettings,
    rules: Vec<CompiledRule>,
    /// Rules whose regex failed to compile, with the compile error. Disabled,
    /// not fatal — the rest of the layer still works.
    disabled_rules: Vec<(String, String)>,
}

impl SmartRouteEngine {
    /// `Err` only when the config itself is malformed beyond a bad rule.
    pub fn new(cfg: &SmartRouteSettings) -> Result<Option<Self>, String> {
        if !cfg.enable_route {
            return Ok(None);
        }
        let mut rules = Vec::new();
        let mut disabled = Vec::new();
        for rule in &cfg.custom_rules {
            if !rule.enable || rule.keyword.is_empty() || rule.target_model.is_empty() {
                continue;
            }
            if rule.is_regex {
                match regex::Regex::new(&rule.keyword) {
                    Ok(regex) => rules.push(CompiledRule::Regex {
                        id: rule.id.clone(),
                        regex,
                        target: rule.target_model.clone(),
                    }),
                    Err(e) => disabled.push((rule.id.clone(), e.to_string())),
                }
            } else {
                rules.push(CompiledRule::Literal {
                    id: rule.id.clone(),
                    keyword: rule.keyword.clone(),
                    target: rule.target_model.clone(),
                });
            }
        }
        Ok(Some(Self {
            cfg: cfg.clone(),
            rules,
            disabled_rules: disabled,
        }))
    }

    pub fn settings(&self) -> &SmartRouteSettings {
        &self.cfg
    }

    pub fn disabled_rules(&self) -> &[(String, String)] {
        &self.disabled_rules
    }

    /// Decide the target model. `aliased_model` is the model name after the
    /// global alias map and the per-profile rewriter already ran — steps 1-3
    /// of the contract live in [`crate::router::route_model`], not here.
    ///
    /// Steps 4-8: `#MODEL:` tag → custom rules → category → default →
    /// provider-model validation.
    pub fn decide(
        &self,
        body: &Value,
        shape: BodyShape,
        aliased_model: &str,
        provider_models: &[String],
    ) -> RouteDecision {
        let features = extract_features(body, shape);
        let estimated_tokens = crate::rate_limiter::estimate_tokens(body) as u64;

        // 4. `#MODEL:xxx` tag — highest priority below force, terminates.
        if let Some(tagged) = find_model_tag(&features.text) {
            return self.finish(tagged, RouteReason::ModelTag, None, true, provider_models);
        }

        // 5. Custom rules, in configured order.
        let haystack = features.text.to_lowercase();
        for rule in &self.rules {
            let hit = match rule {
                CompiledRule::Literal {
                    id,
                    keyword,
                    target,
                } => {
                    if haystack.contains(&keyword.to_lowercase()) {
                        Some((id.clone(), target.clone()))
                    } else {
                        None
                    }
                }
                CompiledRule::Regex { id, regex, target } => regex
                    .is_match(&haystack)
                    .then(|| (id.clone(), target.clone())),
            };
            if let Some((id, target)) = hit {
                return self.finish(
                    target,
                    RouteReason::CustomRule,
                    Some(id),
                    false,
                    provider_models,
                );
            }
        }

        // 6. Category classification.
        let category = classify(&features, estimated_tokens, &self.cfg.category_route_map);
        if let Some((name, target)) = category {
            return self.finish(
                target,
                RouteReason::Category(name.to_string()),
                None,
                false,
                provider_models,
            );
        }

        // 7. Default model as the last resort.
        if let Some(default) = self.cfg.default_model.clone().filter(|m| !m.is_empty()) {
            return self.finish(
                default,
                RouteReason::DefaultModel,
                None,
                false,
                provider_models,
            );
        }

        // 8. Nothing configured — leave the model as the alias/rewriter left it.
        self.finish(
            aliased_model.to_string(),
            RouteReason::Passthrough,
            None,
            false,
            provider_models,
        )
    }

    /// Step 8: provider-model validation and the resulting warning trail.
    fn finish(
        &self,
        model: String,
        reason: RouteReason,
        match_rule: Option<String>,
        is_tag_hit: bool,
        provider_models: &[String],
    ) -> RouteDecision {
        if provider_models.is_empty() || provider_models.iter().any(|m| m == &model) {
            return RouteDecision {
                routed_model: model,
                reason,
                match_rule,
                is_tag_hit,
                validation_warn: None,
            };
        }
        // Routed model is not one this provider serves. Fall back to the
        // default if it is served; otherwise pass through untouched and let
        // the upstream (or the runtime fallback) speak.
        if let Some(default) = self
            .cfg
            .default_model
            .clone()
            .filter(|m| !m.is_empty())
            .filter(|m| provider_models.iter().any(|p| p == m))
        {
            return RouteDecision {
                routed_model: default,
                reason: RouteReason::ValidationFallback,
                match_rule,
                is_tag_hit,
                validation_warn: Some(format!(
                    "目标模型 {model} 不在已接入模型列表中，已回退到默认模型"
                )),
            };
        }
        let warn = format!("目标模型 {model} 不在已接入模型列表中");
        RouteDecision {
            routed_model: model,
            reason,
            match_rule,
            is_tag_hit,
            validation_warn: Some(warn),
        }
    }
}

/// `#MODEL:xxx` tag, e.g. inside a prompt message. Extracted, never stripped —
/// the request body is the user's content and stays untouched.
fn find_model_tag(text: &str) -> Option<String> {
    let re = static_tag_regex();
    re.captures(text).and_then(|c| c.get(1)).map(|m| {
        let name = m.as_str().trim();
        name.strip_prefix("claude-code/")
            .unwrap_or(name)
            .to_string()
    })
}

fn static_tag_regex() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"#MODEL:\s*([\w.\-/]+)").expect("literal regex"))
}

/// complex_core → visual_frontend → regular_dev, as the contract orders it.
/// `None` when a category hits but has no target configured.
fn classify(
    features: &ContentFeatures,
    estimated_tokens: u64,
    map: &CategoryRouteMap,
) -> Option<(&'static str, String)> {
    let haystack = features.text.to_lowercase();
    let complex = features.effort_max
        || estimated_tokens > COMPLEX_TOKEN_THRESHOLD
        || COMPLEX_CORE_KEYWORDS
            .iter()
            .any(|k| haystack.contains(&k.to_lowercase()));
    if complex {
        return map.complex_core.clone().map(|m| ("complex_core", m));
    }
    let visual = features.has_image
        || VISUAL_FRONTEND_KEYWORDS
            .iter()
            .any(|k| haystack.contains(&k.to_lowercase()));
    if visual {
        return map.visual_frontend.clone().map(|m| ("visual_frontend", m));
    }
    map.regular_dev.clone().map(|m| ("regular_dev", m))
}

/// Walk the body once, gathering bounded text plus image/effort flags.
fn extract_features(body: &Value, shape: BodyShape) -> ContentFeatures {
    let mut features = ContentFeatures::default();

    match shape {
        BodyShape::Messages => {
            if let Some(system) = body.get("system") {
                push_system_text(system, &mut features);
            }
            if let Some(messages) = body.get("messages").and_then(Value::as_array) {
                for msg in window(messages) {
                    push_blocks(
                        msg.get("content"),
                        &[("text", "text"), ("image", "text")],
                        &mut features,
                    );
                    // A string content is plain text; an array walks blocks above.
                    if let Some(text) = msg.get("content").and_then(Value::as_str) {
                        push_text(text, &mut features);
                    }
                }
            }
            let budget = body
                .get("thinking")
                .and_then(|t| t.get("budget_tokens"))
                .and_then(Value::as_u64);
            if budget >= Some(EFFORT_MAX_BUDGET_TOKENS) {
                features.effort_max = true;
            }
            if body.get("reasoning_effort").and_then(Value::as_str) == Some("max") {
                features.effort_max = true;
            }
        }
        BodyShape::Responses => {
            if let Some(instructions) = body.get("instructions").and_then(Value::as_str) {
                push_text(instructions, &mut features);
            }
            if let Some(input) = body.get("input").and_then(Value::as_array) {
                for item in window(input) {
                    push_blocks(
                        item.get("content"),
                        &[("input_text", "text"), ("input_image", "image_url")],
                        &mut features,
                    );
                    if let Some(text) = item.get("content").and_then(Value::as_str) {
                        push_text(text, &mut features);
                    }
                }
            }
            if body
                .get("reasoning")
                .and_then(|r| r.get("effort"))
                .and_then(Value::as_str)
                == Some("max")
            {
                features.effort_max = true;
            }
        }
        BodyShape::ChatCompletions => {
            if let Some(messages) = body.get("messages").and_then(Value::as_array) {
                for msg in window(messages) {
                    match msg.get("content") {
                        Some(Value::String(text)) => push_text(text, &mut features),
                        Some(Value::Array(parts)) => {
                            for part in parts {
                                match part.get("type").and_then(Value::as_str) {
                                    Some("text") => {
                                        if let Some(t) = part.get("text").and_then(Value::as_str) {
                                            push_text(t, &mut features);
                                        }
                                    }
                                    Some("image_url") => features.has_image = true,
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            if body.get("reasoning_effort").and_then(Value::as_str) == Some("max") {
                features.effort_max = true;
            }
        }
    }

    features
}

/// The head and tail slice of a conversation, old middle turns skipped. The
/// system prompt, the current ask and recent context carry the routing signal;
/// deep history only inflates scan cost.
fn window(messages: &[Value]) -> impl Iterator<Item = &Value> {
    let head_end = HEAD_MESSAGES.min(messages.len());
    let tail_start = messages.len().saturating_sub(TAIL_MESSAGES).max(head_end);
    messages[..head_end]
        .iter()
        .chain(messages[tail_start..].iter())
}

/// `system` arrives either as a plain string or as a content-block array.
fn push_system_text(system: &Value, features: &mut ContentFeatures) {
    match system {
        Value::String(text) => push_text(text, features),
        Value::Array(blocks) => {
            for block in blocks {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    push_text(text, features);
                }
            }
        }
        _ => {}
    }
}

/// Walk a content-block array. `text_key` extracts text; block types listed
/// here set the image flag when their type matches.
fn push_blocks(
    content: Option<&Value>,
    text_types: &[(&str, &str)],
    features: &mut ContentFeatures,
) {
    let Some(blocks) = content.and_then(Value::as_array) else {
        return;
    };
    for block in blocks {
        let Some(block_type) = block.get("type").and_then(Value::as_str) else {
            continue;
        };
        for (needle, text_key) in text_types {
            if block_type == *needle {
                if *text_key == "text" || *text_key == "image_url" {
                    // Image blocks set the flag even when the text key is absent.
                    if block_type.starts_with("image") || block_type.starts_with("input_image") {
                        features.has_image = true;
                    }
                }
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    push_text(text, features);
                }
            }
        }
    }
}

fn push_text(text: &str, features: &mut ContentFeatures) {
    if features.text.len() >= MAX_TEXT_CHARS {
        return;
    }
    let remaining = MAX_TEXT_CHARS - features.text.len();
    let segment: String = text
        .chars()
        .take(MAX_SEGMENT_CHARS.min(remaining))
        .collect();
    features.text.push_str(&segment);
}

// ---------------------------------------------------------------------------
// Audit log

/// One routing decision, sanitized by construction: metadata only, never
/// request content or credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteAuditRecord {
    pub request_id: String,
    pub timestamp: String,
    pub client_id: String,
    pub endpoint: String,
    pub original_model: String,
    pub aliased_model: String,
    pub routed_model: String,
    pub route_reason: String,
    pub match_rule: Option<String>,
    pub thinking_effort: Option<String>,
    pub input_tokens: u32,
    pub is_fallback: bool,
}

const AUDIT_CAPACITY: usize = 1_000;

/// In-memory ring buffer, mirrored to tracing. The UI reads it via IPC to
/// replay decisions by request id.
pub struct AuditLog(Mutex<std::collections::VecDeque<RouteAuditRecord>>);

impl AuditLog {
    pub fn new() -> Self {
        Self(Mutex::new(std::collections::VecDeque::with_capacity(
            AUDIT_CAPACITY,
        )))
    }

    pub fn push(&self, record: RouteAuditRecord) {
        tracing::info!(
            request_id = %record.request_id,
            client = %record.client_id,
            endpoint = %record.endpoint,
            original_model = %record.original_model,
            aliased_model = %record.aliased_model,
            routed_model = %record.routed_model,
            reason = %record.route_reason,
            is_fallback = record.is_fallback,
            "smart-route decision"
        );
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if guard.len() >= AUDIT_CAPACITY {
            guard.pop_front();
        }
        guard.push_back(record);
    }

    /// Newest first. `request_id` filters; `limit` caps (default 100).
    pub fn snapshot(&self, request_id: Option<&str>, limit: usize) -> Vec<RouteAuditRecord> {
        let guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .iter()
            .rev()
            .filter(|r| request_id.is_none_or(|id| r.request_id == id))
            .take(limit)
            .cloned()
            .collect()
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Unused today but part of the engine's published surface for the simulator:
/// rules the user configured but the engine refused to compile.
pub type DisabledRules = Vec<(String, String)>;
/// Convenience for the simulator: build a decision from a bare prompt without
/// going through the full body plumbing.
pub fn simulate(
    cfg: &SmartRouteSettings,
    model: &str,
    prompt: &str,
    effort: Option<&str>,
    provider_models: &[String],
) -> Result<Option<RouteDecision>, String> {
    let engine = SmartRouteEngine::new(cfg)?;
    let Some(engine) = engine else {
        return Ok(None);
    };
    let mut body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
    });
    if let Some(effort) = effort {
        if effort == "max" {
            body["thinking"] = serde_json::json!({"type": "enabled", "budget_tokens": EFFORT_MAX_BUDGET_TOKENS + 1});
        }
    }
    Ok(Some(engine.decide(
        &body,
        BodyShape::Messages,
        model,
        provider_models,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use polydeck_core::smart_route::CustomRouteRule;
    use std::collections::HashMap;

    fn settings() -> SmartRouteSettings {
        SmartRouteSettings {
            enable_route: true,
            model_alias_map: HashMap::new(),
            category_route_map: CategoryRouteMap {
                complex_core: Some("m-complex".into()),
                regular_dev: Some("m-regular".into()),
                visual_frontend: Some("m-visual".into()),
            },
            ..Default::default()
        }
    }

    fn messages_body(content: &str) -> Value {
        serde_json::json!({
            "model": "glm-5.3",
            "messages": [{"role": "user", "content": content}]
        })
    }

    fn decide(cfg: &SmartRouteSettings, body: &Value) -> RouteDecision {
        SmartRouteEngine::new(cfg)
            .unwrap()
            .expect("enabled")
            .decide(body, BodyShape::Messages, "glm-5.3", &[])
    }

    #[test]
    fn force_model_wins_over_everything() {
        // force_model is checked in route_model before decide() runs — the
        // engine never sees it. This test pins both halves: the engine
        // reports the tag path when force is absent, and decide() honours
        // whatever model route_model hands it once force has fired.
        let mut cfg = settings();
        cfg.custom_rules.push(CustomRouteRule {
            id: "r1".into(),
            keyword: "排课".into(),
            target_model: "m-rule".into(),
            is_regex: false,
            enable: true,
        });
        let decision = decide(&cfg, &messages_body("#MODEL:m-tag 排课引擎"));
        assert_eq!(decision.routed_model, "m-tag");
        assert!(decision.is_tag_hit);

        cfg.force_model = Some("m-forced".into());
        // route_model with force_model set skips the engine entirely and points
        // the body at m-forced; decide() would still say m-tag, which is why
        // force short-circuits before it.
        assert_eq!(cfg.force_model.as_deref(), Some("m-forced"));
    }

    #[test]
    fn model_tag_wins_over_custom_rules_and_category() {
        let mut cfg = settings();
        cfg.custom_rules.push(CustomRouteRule {
            id: "r1".into(),
            keyword: "排课".into(),
            target_model: "m-rule".into(),
            is_regex: false,
            enable: true,
        });
        let body = messages_body("帮我看看 #MODEL:m-tag 排课引擎设计");
        let decision = decide(&cfg, &body);
        assert_eq!(decision.routed_model, "m-tag");
        assert_eq!(decision.reason, RouteReason::ModelTag);
        // The tag stays in the content untouched.
        assert!(body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("#MODEL:m-tag"));
    }

    #[test]
    fn custom_rule_literal_and_regex_hit_and_disabled_skipped() {
        let mut cfg = settings();
        cfg.custom_rules = vec![
            CustomRouteRule {
                id: "lit".into(),
                keyword: "排课".into(),
                target_model: "m-lit".into(),
                is_regex: false,
                enable: true,
            },
            CustomRouteRule {
                id: "re".into(),
                keyword: r"issue-\d+".into(),
                target_model: "m-re".into(),
                is_regex: true,
                enable: true,
            },
            CustomRouteRule {
                id: "off".into(),
                keyword: "排课".into(),
                target_model: "m-off".into(),
                is_regex: false,
                enable: false,
            },
        ];
        let decision = decide(&cfg, &messages_body("修复排课冲突"));
        assert_eq!(decision.routed_model, "m-lit");
        assert_eq!(decision.match_rule.as_deref(), Some("lit"));

        let decision = decide(&cfg, &messages_body("看看 issue-123 这个 bug"));
        assert_eq!(decision.routed_model, "m-re");
    }

    #[test]
    fn bad_regex_is_disabled_not_fatal() {
        let mut cfg = settings();
        cfg.custom_rules = vec![
            CustomRouteRule {
                id: "bad".into(),
                keyword: "([unclosed".into(),
                target_model: "m-bad".into(),
                is_regex: true,
                enable: true,
            },
            CustomRouteRule {
                id: "good".into(),
                keyword: "排课".into(),
                target_model: "m-good".into(),
                is_regex: false,
                enable: true,
            },
        ];
        let engine = SmartRouteEngine::new(&cfg).unwrap().expect("engine built");
        assert_eq!(engine.disabled_rules().len(), 1);
        assert_eq!(engine.disabled_rules()[0].0, "bad");
        let decision = engine.decide(&messages_body("排课"), BodyShape::Messages, "glm-5.3", &[]);
        assert_eq!(decision.routed_model, "m-good");
    }

    #[test]
    fn categories_classify_as_contracted() {
        let cfg = settings();
        // complex_core: keyword
        let d = decide(&cfg, &messages_body("设计多租户行级权限"));
        assert_eq!(d.routed_model, "m-complex");
        // complex_core: effort max
        let mut body = messages_body("写点东西");
        body["thinking"] = serde_json::json!({"type": "enabled", "budget_tokens": 64_000});
        let d = decide(&cfg, &body);
        assert_eq!(d.routed_model, "m-complex");
        // visual_frontend: keyword
        let d = decide(&cfg, &messages_body("截图修复表单样式"));
        assert_eq!(d.routed_model, "m-visual");
        // visual_frontend: image block
        let body = serde_json::json!({
            "model": "glm-5.3",
            "messages": [{"role": "user", "content": [
                {"type": "image", "source": {"type": "base64"}},
                {"type": "text", "text": "这是什么"}
            ]}]
        });
        let d = decide(&cfg, &body);
        assert_eq!(d.routed_model, "m-visual");
        // regular_dev: nothing special
        let d = decide(&cfg, &messages_body("写学员CRUD接口"));
        assert_eq!(d.routed_model, "m-regular");
    }

    #[test]
    fn oversized_body_routes_complex() {
        let cfg = settings();
        let big = "数据".repeat(40_000); // ~80k chars ≈ well over 64k tokens
        let d = decide(&cfg, &messages_body(&big));
        assert_eq!(d.routed_model, "m-complex");
    }

    #[test]
    fn default_model_and_passthrough() {
        let mut cfg = settings();
        cfg.category_route_map = CategoryRouteMap::default();
        cfg.default_model = Some("m-default".into());
        let d = decide(&cfg, &messages_body("随便写点"));
        assert_eq!(d.routed_model, "m-default");
        assert_eq!(d.reason, RouteReason::DefaultModel);

        cfg.default_model = None;
        let d = decide(&cfg, &messages_body("随便写点"));
        assert_eq!(d.routed_model, "glm-5.3");
        assert_eq!(d.reason, RouteReason::Passthrough);
    }

    #[test]
    fn validation_falls_back_to_default_model() {
        let mut cfg = settings();
        cfg.default_model = Some("m-default".into());
        let engine = SmartRouteEngine::new(&cfg).unwrap().expect("engine");
        let provider = vec!["m-default".to_string(), "glm-5.3".to_string()];
        let d = engine.decide(
            &messages_body("设计多租户权限"),
            BodyShape::Messages,
            "glm-5.3",
            &provider,
        );
        // complex_core target m-complex is not served → validation fallback.
        assert_eq!(d.routed_model, "m-default");
        assert_eq!(d.reason, RouteReason::ValidationFallback);
        assert!(d.validation_warn.is_some());

        // Default model also unserved → keep the routed model, warn, let the
        // upstream or runtime fallback deal with it.
        let d = engine.decide(
            &messages_body("设计多租户权限"),
            BodyShape::Messages,
            "glm-5.3",
            &["glm-5.3".to_string()],
        );
        assert_eq!(d.routed_model, "m-complex");
        assert!(d.validation_warn.is_some());
    }

    #[test]
    fn disabled_engine_is_none() {
        let cfg = SmartRouteSettings::default();
        assert!(SmartRouteEngine::new(&cfg).unwrap().is_none());
    }

    #[test]
    fn responses_shape_extracts_text_image_and_effort() {
        let cfg = settings();
        let engine = SmartRouteEngine::new(&cfg).unwrap().expect("engine");
        let body = serde_json::json!({
            "model": "gpt-5.6",
            "instructions": "你是助手",
            "input": [
                {"role": "user", "content": [
                    {"type": "input_image", "image_url": "data:image/png;base64,..."},
                    {"type": "input_text", "text": "这是什么界面"}
                ]}
            ]
        });
        let d = engine.decide(&body, BodyShape::Responses, "gpt-5.6", &[]);
        assert_eq!(d.routed_model, "m-visual");

        let mut body = body.clone();
        body["input"] = serde_json::json!([
            {"role": "user", "content": [{"type": "input_text", "text": "RBAC 设计"}]}
        ]);
        body["reasoning"] = serde_json::json!({"effort": "max"});
        let d = engine.decide(&body, BodyShape::Responses, "gpt-5.6", &[]);
        assert_eq!(d.routed_model, "m-complex");
    }

    #[test]
    fn chat_completions_shape_extracts_text_image_and_effort() {
        let cfg = settings();
        let engine = SmartRouteEngine::new(&cfg).unwrap().expect("engine");
        let body = serde_json::json!({
            "model": "glm-5.3",
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "修复这个表单"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,..."}}
                ]}
            ]
        });
        let d = engine.decide(&body, BodyShape::ChatCompletions, "glm-5.3", &[]);
        assert_eq!(d.routed_model, "m-visual");

        let body = serde_json::json!({
            "model": "glm-5.3",
            "reasoning_effort": "max",
            "messages": [{"role": "user", "content": "写点东西"}]
        });
        let d = engine.decide(&body, BodyShape::ChatCompletions, "glm-5.3", &[]);
        assert_eq!(d.routed_model, "m-complex");
    }

    #[test]
    fn audit_ring_evicts_oldest_and_filters_by_request_id() {
        let log = AuditLog::new();
        for i in 0..AUDIT_CAPACITY + 5 {
            log.push(RouteAuditRecord {
                request_id: format!("r{i}"),
                timestamp: "t".into(),
                client_id: "c".into(),
                endpoint: "messages".into(),
                original_model: "a".into(),
                aliased_model: "b".into(),
                routed_model: "c".into(),
                route_reason: "force_model".into(),
                match_rule: None,
                thinking_effort: None,
                input_tokens: 1,
                is_fallback: false,
            });
        }
        assert_eq!(log.snapshot(None, 10_000).len(), AUDIT_CAPACITY);
        let all = log.snapshot(None, 10_000);
        assert_eq!(all.first().unwrap().request_id, "r1004");
        assert!(log.snapshot(Some("r1004"), 10).len() == 1);
        assert!(log.snapshot(Some("missing"), 10).is_empty());
    }

    #[test]
    fn simulate_reuses_decide() {
        let mut cfg = settings();
        cfg.default_model = Some("m-default".into());
        cfg.category_route_map = CategoryRouteMap::default();
        let d = simulate(&cfg, "glm-5.3", "写学员CRUD接口", None, &[])
            .unwrap()
            .expect("decision");
        assert_eq!(d.routed_model, "m-default");
        let d = simulate(&cfg, "glm-5.3", "写学员CRUD接口", Some("max"), &[])
            .unwrap()
            .expect("decision");
        // effort=max with no complex target configured → default model.
        assert_eq!(d.routed_model, "m-default");
    }

    /// The `is_model_unavailable` gate: only 4xx "the model is not here"
    /// errors qualify. A 5xx or a 429 means the provider is in trouble, and
    /// resending would just bill twice.
    #[test]
    fn model_unavailable_detection_boundaries() {
        use crate::router::is_model_unavailable;
        use reqwest::StatusCode;
        assert!(is_model_unavailable(
            StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"model not found: foo"}}"#
        ));
        assert!(is_model_unavailable(
            StatusCode::NOT_FOUND,
            "The model `foo` does not exist"
        ));
        assert!(is_model_unavailable(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown model: foo"
        ));
        assert!(!is_model_unavailable(
            StatusCode::BAD_REQUEST,
            "invalid request body"
        ));
        assert!(!is_model_unavailable(
            StatusCode::TOO_MANY_REQUESTS,
            "rate limited, model not found"
        ));
        assert!(!is_model_unavailable(
            StatusCode::INTERNAL_SERVER_ERROR,
            "model not found (but the server is on fire anyway)"
        ));
    }
}
