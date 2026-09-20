//! Smart route configuration — the user-configurable gateway routing layer.
//!
//! Everything here is stored globally in `AppSettings` (one config for every
//! bound profile) and compiled into a `SmartRouteEngine` by the gateway. The
//! decision order is a contract shared with the engine and the UI help text:
//!
//! 1. `force_model` non-empty → use it, done.
//! 2. Global alias map replaces the inbound model name.
//! 3. Per-profile `ModelRewriter` rules run (existing behaviour).
//! 4. `#MODEL:xxx` tag in the request content wins over everything below.
//! 5. Enabled custom rules, in order.
//! 6. Category classification: complex_core → visual_frontend → regular_dev.
//! 7. `default_model` as the last resort.
//! 8. Validation: the routed model must exist in the provider's model list,
//!    otherwise fall back to `default_model` and record a warning.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Built-in `complex_core` trigger keywords. The classification logic itself is
/// not user-configurable — only each category's target model is.
pub const COMPLEX_CORE_KEYWORDS: &[&str] = &[
    "多租户",
    "rbac",
    "行级权限",
    "校区隔离",
    "越权防护",
    "财务结算",
    "课时扣费",
    "退费",
    "转课",
    "课消",
    "排课引擎",
    "事务一致性",
    "资金分账",
    "业务状态机",
    "核心schema",
    "架构设计",
    "跨模块重构",
    "安全审计",
    "复杂事务sql",
    "permission model",
    "architecture design",
];

/// Built-in `visual_frontend` trigger keywords.
pub const VISUAL_FRONTEND_KEYWORDS: &[&str] = &[
    "vue",
    "react",
    "组件",
    "表单",
    "样式",
    "ui",
    "截图",
    "figma",
    "切图",
    "playwright",
    "视觉测试",
    "页面还原",
    "component",
    "stylesheet",
    "screenshot",
    "frontend",
];

/// One keyword/regex rule. Literal rules match by substring (case-folded for
/// ASCII); regex rules compile once when the engine is built — a rule that
/// fails to compile is disabled with the reason recorded, never fatal.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CustomRouteRule {
    pub id: String,
    pub keyword: String,
    pub target_model: String,
    #[serde(default)]
    pub is_regex: bool,
    #[serde(default)]
    pub enable: bool,
}

/// Target model per built-in category. `None` means "no target configured" and
/// the category silently does not participate.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CategoryRouteMap {
    #[serde(default)]
    pub complex_core: Option<String>,
    #[serde(default)]
    pub regular_dev: Option<String>,
    #[serde(default)]
    pub visual_frontend: Option<String>,
}

/// The whole smart-route layer, one instance for the app. Every field is
/// `serde(default)`-compatible so a `state.json` written before this feature
/// keeps parsing (same reason as every other `AppSettings` addition).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "camelCase", default)]
pub struct SmartRouteSettings {
    #[serde(default)]
    pub enable_route: bool,
    #[serde(default)]
    pub force_model: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub fallback_enable: bool,
    #[serde(default)]
    pub model_alias_map: HashMap<String, String>,
    #[serde(default)]
    pub category_route_map: CategoryRouteMap,
    #[serde(default)]
    pub custom_rules: Vec<CustomRouteRule>,
}

impl SmartRouteSettings {
    /// Compile every regex rule and report the ones that fail, so the UI can
    /// show a warning per bad rule instead of the whole layer silently dying.
    pub fn validate(&self) -> Vec<String> {
        self.custom_rules
            .iter()
            .filter(|r| r.is_regex && r.enable)
            .filter(|r| regex::Regex::new(&r.keyword).is_err())
            .map(|r| format!("规则 {} 的正则无法编译: {}", r.id, r.keyword))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_json_without_smart_route_field_parses_with_defaults() {
        let raw: &str = r#"{"theme":"system","autoStart":false,"minimizeToTray":true,"checkUpdates":"weekly","acceptInvalidCerts":false,"generateOnly":true,"injection":{"enabled":false,"portRangeStart":9222,"portRangeEnd":9322,"features":[]},"stepwise":{"enabled":false,"modelOverride":null,"suggestionCount":3,"temperature":0.7,"timeoutSecs":30}}"#;
        let settings: crate::profile::AppSettings = serde_json::from_str(raw)
            .expect("a settings block predating smart_route must still deserialize");
        assert!(!settings.smart_route.enable_route);
        assert!(settings.smart_route.custom_rules.is_empty());
    }

    #[test]
    fn smart_route_settings_round_trip() {
        let mut settings = SmartRouteSettings {
            enable_route: true,
            ..Default::default()
        };
        settings
            .model_alias_map
            .insert("claude-opus-5".into(), "glm-5.3".into());
        settings.category_route_map.complex_core = Some("glm-5.3".into());
        settings.custom_rules.push(CustomRouteRule {
            id: "r1".into(),
            keyword: "排课".into(),
            target_model: "model-a".into(),
            is_regex: false,
            enable: true,
        });
        let json = serde_json::to_string(&settings).unwrap();
        let back: SmartRouteSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, settings);
    }

    #[test]
    fn validate_reports_only_bad_regexes() {
        let settings = SmartRouteSettings {
            custom_rules: vec![
                CustomRouteRule {
                    id: "ok".into(),
                    keyword: "[0-9]+".into(),
                    target_model: "m".into(),
                    is_regex: true,
                    enable: true,
                },
                CustomRouteRule {
                    id: "bad".into(),
                    keyword: "([unclosed".into(),
                    target_model: "m".into(),
                    is_regex: true,
                    enable: true,
                },
                CustomRouteRule {
                    id: "disabled-bad".into(),
                    keyword: "([also-unclosed".into(),
                    target_model: "m".into(),
                    is_regex: true,
                    enable: false,
                },
            ],
            ..Default::default()
        };
        let warnings = settings.validate();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("bad"));
    }
}
