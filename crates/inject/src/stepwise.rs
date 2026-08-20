//! Bounded Stepwise suggestion requests using active Profile credentials.

use polydeck_core::profile::{ProviderConfig, StepwiseSettings};
use serde_json::Value;
use std::{collections::{HashMap, VecDeque}, time::{Duration, Instant}};
use thiserror::Error;

const MAX_CONTEXT_CHARS: usize = 16_000;
const MAX_SUGGESTIONS: u8 = 5;
const CACHE_TTL: Duration = Duration::from_secs(300);
const CACHE_LIMIT: usize = 64;

#[derive(Debug, Error)]
pub enum StepwiseError {
    #[error("no active Profile primary provider")]
    MissingProvider,
    #[error("active provider has no API credential")]
    MissingCredential,
    #[error("invalid Stepwise configuration: {0}")]
    InvalidConfig(String),
    #[error("Stepwise request timed out")]
    Timeout,
    #[error("Stepwise request failed")]
    Request,
    #[error("Stepwise response did not contain suggestions")]
    InvalidResponse,
}

pub trait CredentialResolver: Send + Sync {
    fn credential_for(&self, key_id: &str) -> Result<Option<String>, StepwiseError>;
}

#[derive(Clone)]
pub struct KeyringCredentialResolver;

impl CredentialResolver for KeyringCredentialResolver {
    fn credential_for(&self, key_id: &str) -> Result<Option<String>, StepwiseError> {
        polydeck_core::credentials::get_credential(key_id).map_err(|_| StepwiseError::MissingCredential)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CacheValue { suggestions: Vec<String>, created: Instant }

#[derive(Default)]
pub struct StepwiseCache {
    values: HashMap<String, CacheValue>,
    order: VecDeque<String>,
}

impl StepwiseCache {
    pub fn get(&mut self, key: &str) -> Option<Vec<String>> {
        let value = self.values.get(key)?;
        if value.created.elapsed() >= CACHE_TTL { self.values.remove(key); self.order.retain(|e| e != key); return None; }
        Some(value.suggestions.clone())
    }
    pub fn insert(&mut self, key: String, suggestions: Vec<String>) {
        if !self.values.contains_key(&key) { self.order.push_back(key.clone()); }
        self.values.insert(key, CacheValue { suggestions, created: Instant::now() });
        while self.order.len() > CACHE_LIMIT { if let Some(oldest) = self.order.pop_front() { self.values.remove(&oldest); } }
    }
}

pub struct StepwiseService<R: CredentialResolver> { resolver: R, client: reqwest::Client, cache: StepwiseCache }

impl<R: CredentialResolver> StepwiseService<R> {
    pub fn new(resolver: R) -> Self {
        Self { resolver, client: reqwest::Client::new(), cache: StepwiseCache::default() }
    }

    pub async fn suggestions(
        &mut self, provider: Option<&ProviderConfig>, settings: &StepwiseSettings, context: &str,
    ) -> Result<Vec<String>, StepwiseError> {
        let provider = provider.ok_or(StepwiseError::MissingProvider)?;
        let config = validated_settings(settings, provider)?;
        let context = truncate_context(context);
        if context.trim().is_empty() { return Err(StepwiseError::InvalidConfig("context is empty".into())); }
        let key = cache_key(provider, &config, &context);
        if let Some(s) = self.cache.get(&key) { return Ok(s); }
        let credential = self.resolver.credential_for(&provider.id)?.ok_or(StepwiseError::MissingCredential)?;
        let url = format!("{}/chat/completions", provider.base_url.trim_end_matches('/'));
        let request = self.client.post(url).bearer_auth(credential).json(&serde_json::json!({
            "model": config.model, "temperature": config.temperature, "max_tokens": 500,
            "messages": [
                {"role":"system","content": format!("Provide {} concise next actions. Return JSON array of strings only.", config.count)},
                {"role":"user","content": context}
            ]
        }));
        let response = tokio::time::timeout(Duration::from_secs(config.timeout_seconds), request.send())
            .await.map_err(|_| StepwiseError::Timeout)?.map_err(|_| StepwiseError::Request)?;
        let body: Value = response.json().await.map_err(|_| StepwiseError::InvalidResponse)?;
        let suggestions = parse_suggestions(&body, config.count)?;
        self.cache.insert(key, suggestions.clone());
        Ok(suggestions)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ValidatedSettings { model: String, count: u8, temperature: f32, timeout_seconds: u64 }

fn validated_settings(settings: &StepwiseSettings, provider: &ProviderConfig) -> Result<ValidatedSettings, StepwiseError> {
    if !(1..=MAX_SUGGESTIONS).contains(&settings.suggestion_count) { return Err(StepwiseError::InvalidConfig("count must be between 1 and 5".into())); }
    if !(0.0..=2.0).contains(&settings.temperature) { return Err(StepwiseError::InvalidConfig("temperature must be between 0 and 2".into())); }
    if !(1..=120).contains(&settings.timeout_secs) { return Err(StepwiseError::InvalidConfig("timeout must be between 1 and 120 seconds".into())); }
    Ok(ValidatedSettings {
        model: settings.model_override.as_deref().filter(|m| !m.trim().is_empty()).unwrap_or(&provider.default_model).to_string(),
        count: settings.suggestion_count, temperature: settings.temperature, timeout_seconds: settings.timeout_secs as u64,
    })
}

fn truncate_context(context: &str) -> String {
    if context.len() <= MAX_CONTEXT_CHARS { return context.to_string(); }
    let start = context.len() - MAX_CONTEXT_CHARS;
    let start = context.ceil_char_boundary(start);
    context[start..].to_string()
}

fn parse_suggestions(body: &Value, count: u8) -> Result<Vec<String>, StepwiseError> {
    let content = body.pointer("/choices/0/message/content").and_then(Value::as_str).ok_or(StepwiseError::InvalidResponse)?;
    let parsed: Value = serde_json::from_str(content).unwrap_or_else(|_| Value::String(content.to_string()));
    let values = match parsed {
        Value::Array(values) => values,
        Value::String(value) => value.lines().map(|l| Value::String(l.trim_start_matches(|c: char| c.is_ascii_digit() || matches!(c, '.' | '-' | ')' | ' ')).to_string())).collect(),
        _ => return Err(StepwiseError::InvalidResponse),
    };
    let suggestions: Vec<_> = values.into_iter()
        .filter_map(|v| v.as_str().map(str::trim).filter(|v| !v.is_empty()).map(ToOwned::to_owned))
        .take(count as usize).collect();
    if suggestions.is_empty() { return Err(StepwiseError::InvalidResponse); }
    Ok(suggestions)
}

fn cache_key(provider: &ProviderConfig, settings: &ValidatedSettings, context: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    digest.update(provider.id.as_bytes());
    digest.update(provider.base_url.as_bytes());
    digest.update(settings.model.as_bytes());
    digest.update(settings.count.to_le_bytes());
    digest.update(settings.temperature.to_le_bytes());
    digest.update(context.as_bytes());
    format!("{:x}", digest.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_array() {
        let body = serde_json::json!({"choices":[{"message":{"content":"[\"One\",\"Two\"]"}}]});
        assert_eq!(parse_suggestions(&body, 2).unwrap(), vec!["One", "Two"]);
    }

    #[test]
    fn parses_plaintext_lines() {
        let body = serde_json::json!({"choices":[{"message":{"content":"1. One\n2. Two"}}]});
        assert_eq!(parse_suggestions(&body, 2).unwrap(), vec!["One", "Two"]);
    }

    #[test]
    fn rejects_empty_suggestions() {
        let body = serde_json::json!({"choices":[{"message":{"content":"[]"}}]});
        assert!(parse_suggestions(&body, 2).is_err());
    }
}