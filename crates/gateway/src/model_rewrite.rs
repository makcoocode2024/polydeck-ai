//! Model name rewriting engine

use crate::config::ModelRewriteRule;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ModelRewriter {
    exact_rules: HashMap<String, String>,
    regex_rules: Vec<(regex::Regex, String)>,
    reverse_map: HashMap<String, String>,
}

impl ModelRewriter {
    pub fn new(rules: &[ModelRewriteRule]) -> Result<Self, String> {
        let mut exact_rules = HashMap::new();
        let mut regex_rules = Vec::new();
        let mut reverse_map = HashMap::new();

        for rule in rules.iter().filter(|r| r.enabled) {
            if let Ok(re) = regex::Regex::new(&rule.from) {
                if is_literal_pattern(&rule.from) {
                    exact_rules.insert(rule.from.clone(), rule.to.clone());
                    reverse_map.insert(rule.to.clone(), rule.from.clone());
                } else {
                    regex_rules.push((re, rule.to.clone()));
                }
            } else {
                exact_rules.insert(rule.from.clone(), rule.to.clone());
                reverse_map.insert(rule.to.clone(), rule.from.clone());
            }
        }

        Ok(Self { exact_rules, regex_rules, reverse_map })
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

    pub fn rewrite_response(&self, model: &str) -> String {
        self.reverse_map.get(model).cloned().unwrap_or_else(|| model.to_string())
    }
}

fn is_literal_pattern(pattern: &str) -> bool {
    !pattern.chars().any(|c| matches!(c, '.' | '*' | '+' | '?' | '[' | ']' | '(' | ')' | '{' | '}' | '^' | '$' | '|' | '\\'))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_rule(from: &str, to: &str) -> ModelRewriteRule {
        ModelRewriteRule { from: from.to_string(), to: to.to_string(), enabled: true, description: None }
    }

    #[test]
    fn exact_match_rewrite() {
        let rules = vec![make_rule("claude-sonnet-4-5", "glm-5.2"), make_rule("gpt-4o", "qwen-max")];
        let rewriter = ModelRewriter::new(&rules).unwrap();
        assert_eq!(rewriter.rewrite_request("claude-sonnet-4-5"), "glm-5.2");
        assert_eq!(rewriter.rewrite_request("gpt-4o"), "qwen-max");
        assert_eq!(rewriter.rewrite_request("unknown-model"), "unknown-model");
    }

    #[test]
    fn reverse_mapping() {
        let rules = vec![make_rule("claude-sonnet-4-5", "glm-5.2")];
        let rewriter = ModelRewriter::new(&rules).unwrap();
        assert_eq!(rewriter.rewrite_response("glm-5.2"), "claude-sonnet-4-5");
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
}
