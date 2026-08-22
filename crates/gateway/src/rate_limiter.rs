//! Thread-safe Token Bucket Rate Limiter for Gateway.
//!
//! Enforces RPM (Requests Per Minute) and TPM (Tokens Per Minute) per Provider.
//! Provides asynchronous in-gateway queueing and adaptive backoff for upstream 429 errors,
//! shielding upper-layer Agents (Hermes, Codex, Claude) from rate limit failures.

use polydeck_core::profile::RateLimitSettings;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};

/// Token bucket rate limiter for a specific provider.
#[derive(Debug)]
pub struct ProviderRateLimiter {
    pub provider_id: String,
    pub enabled: bool,
    pub configured_rpm: u32,
    pub configured_tpm: u32,
    pub adaptive: bool,

    // Dynamic operational parameters
    current_rpm: f64,
    current_tpm: f64,
    rpm_tokens: f64,
    tpm_tokens: f64,
    last_replenish: Instant,

    // 429 adaptive backoff & recovery
    consecutive_429: u32,
    backoff_until: Option<Instant>,
}

impl ProviderRateLimiter {
    pub fn new(provider_id: String, settings: &RateLimitSettings) -> Self {
        let rpm = settings.rpm.max(1) as f64;
        let tpm = settings.tpm.max(100) as f64;
        Self {
            provider_id,
            enabled: settings.enabled,
            configured_rpm: settings.rpm,
            configured_tpm: settings.tpm,
            adaptive: settings.adaptive,
            current_rpm: rpm,
            current_tpm: tpm,
            rpm_tokens: rpm,
            tpm_tokens: tpm,
            last_replenish: Instant::now(),
            consecutive_429: 0,
            backoff_until: None,
        }
    }

    pub fn update_settings(&mut self, settings: &RateLimitSettings) {
        self.enabled = settings.enabled;
        self.configured_rpm = settings.rpm;
        self.configured_tpm = settings.tpm;
        self.adaptive = settings.adaptive;
        if self.consecutive_429 == 0 {
            self.current_rpm = settings.rpm.max(1) as f64;
            self.current_tpm = settings.tpm.max(100) as f64;
        }
    }

    fn replenish(&mut self, now: Instant) {
        let elapsed = now.duration_since(self.last_replenish).as_secs_f64();
        self.last_replenish = now;

        let rpm_fill_rate = self.current_rpm / 60.0;
        let tpm_fill_rate = self.current_tpm / 60.0;

        self.rpm_tokens =
            (self.rpm_tokens + elapsed * rpm_fill_rate).min(self.current_rpm.max(1.0));
        self.tpm_tokens =
            (self.tpm_tokens + elapsed * tpm_fill_rate).min(self.current_tpm.max(1000.0));
    }

    /// Acquire tokens for a request. If tokens are insufficient, asynchronously
    /// queues and waits until tokens are replenished, up to `max_wait`.
    pub async fn acquire(
        &mut self,
        requested_tokens: u32,
        max_wait: Duration,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }

        let start = Instant::now();
        loop {
            let now = Instant::now();
            if start.elapsed() >= max_wait {
                return Err(format!(
                    "Provider '{}' 速率超限排队超时 ({:.1}s) - RPM: {:.1}/{}, TPM: {:.0}/{}",
                    self.provider_id,
                    max_wait.as_secs_f64(),
                    self.current_rpm,
                    self.configured_rpm,
                    self.current_tpm,
                    self.configured_tpm
                ));
            }

            // Check if we are currently in 429 backoff cooldown
            if let Some(until) = self.backoff_until {
                if now < until {
                    let wait_time = until.duration_since(now);
                    if start.elapsed() + wait_time >= max_wait {
                        return Err(format!(
                            "Provider '{}' 正在 429 退避冷却中，等待超时",
                            self.provider_id
                        ));
                    }
                    debug!(
                        "Provider '{}' in 429 cooldown, waiting {:.2}s...",
                        self.provider_id,
                        wait_time.as_secs_f64()
                    );
                    tokio::time::sleep(wait_time).await;
                    continue;
                } else {
                    self.backoff_until = None;
                }
            }

            self.replenish(now);

            let rpm_needed = 1.0;
            let tpm_needed = (requested_tokens as f64).min(self.current_tpm);

            if self.rpm_tokens >= rpm_needed && self.tpm_tokens >= tpm_needed {
                self.rpm_tokens -= rpm_needed;
                self.tpm_tokens -= tpm_needed;
                return Ok(());
            }

            // Calculate required wait time
            let rpm_rate = (self.current_rpm / 60.0).max(0.01);
            let tpm_rate = (self.current_tpm / 60.0).max(0.1);

            let rpm_wait = if self.rpm_tokens < rpm_needed {
                (rpm_needed - self.rpm_tokens) / rpm_rate
            } else {
                0.0
            };

            let tpm_wait = if self.tpm_tokens < tpm_needed {
                (tpm_needed - self.tpm_tokens) / tpm_rate
            } else {
                0.0
            };

            let wait_secs = rpm_wait.max(tpm_wait).max(0.05);
            let wait_duration = Duration::from_secs_f64(wait_secs);

            if start.elapsed() + wait_duration >= max_wait {
                let remaining = max_wait.saturating_sub(start.elapsed());
                if remaining.as_millis() < 50 {
                    return Err(format!(
                        "Provider '{}' 令牌桶耗尽，排队超时",
                        self.provider_id
                    ));
                }
                tokio::time::sleep(remaining).await;
            } else {
                debug!(
                    "Provider '{}' throttling queue wait {:.2}s (RPM tokens: {:.2}, TPM tokens: {:.0})",
                    self.provider_id, wait_secs, self.rpm_tokens, self.tpm_tokens
                );
                tokio::time::sleep(wait_duration).await;
            }
        }
    }

    /// Triggered when upstream responds with HTTP 429 (Rate Limit Exceeded).
    pub fn on_429(&mut self, retry_after: Option<Duration>) {
        self.consecutive_429 = self.consecutive_429.saturating_add(1);
        if self.adaptive {
            // Adaptively scale down operating limits by 30%
            self.current_rpm = (self.current_rpm * 0.7).max(5.0);
            self.current_tpm = (self.current_tpm * 0.7).max(5000.0);
            warn!(
                "Provider '{}' 上游返回 429，自适应降速至 {:.1} RPM, {:.0} TPM (连续 429: {})",
                self.provider_id, self.current_rpm, self.current_tpm, self.consecutive_429
            );
        }

        let backoff = retry_after.unwrap_or_else(|| {
            let multiplier = 2u64.saturating_pow(self.consecutive_429.min(4));
            Duration::from_secs(multiplier.clamp(2, 30))
        });
        self.backoff_until = Some(Instant::now() + backoff);
        info!(
            "Provider '{}' 设置 429 冷却退避等待: {:.2}s",
            self.provider_id,
            backoff.as_secs_f64()
        );
    }

    /// Triggered on successful upstream request.
    pub fn on_success(&mut self) {
        if self.consecutive_429 > 0 {
            self.consecutive_429 = 0;
        }
        if self.adaptive {
            // Slowly recover operational rates towards configured limits
            let max_rpm = self.configured_rpm.max(1) as f64;
            let max_tpm = self.configured_tpm.max(100) as f64;
            if self.current_rpm < max_rpm {
                self.current_rpm = (self.current_rpm * 1.05).min(max_rpm);
            }
            if self.current_tpm < max_tpm {
                self.current_tpm = (self.current_tpm * 1.05).min(max_tpm);
            }
        }
    }

    pub fn current_rpm(&self) -> f64 {
        self.current_rpm
    }

    pub fn current_tpm(&self) -> f64 {
        self.current_tpm
    }

    pub fn available_rpm_tokens(&self) -> f64 {
        self.rpm_tokens
    }

    pub fn available_tpm_tokens(&self) -> f64 {
        self.tpm_tokens
    }
}

/// Registry holding per-provider rate limiters in a thread-safe container.
#[derive(Debug, Default)]
pub struct RateLimiterRegistry {
    limiters: RwLock<HashMap<String, Arc<Mutex<ProviderRateLimiter>>>>,
}

impl RateLimiterRegistry {
    pub fn new() -> Self {
        Self {
            limiters: RwLock::new(HashMap::new()),
        }
    }

    /// Get existing or create a new rate limiter for the given provider.
    pub async fn get_or_create(
        &self,
        provider_id: &str,
        settings: &RateLimitSettings,
    ) -> Arc<Mutex<ProviderRateLimiter>> {
        {
            let read_guard = self.limiters.read().await;
            if let Some(limiter) = read_guard.get(provider_id) {
                let mut guard = limiter.lock().await;
                guard.update_settings(settings);
                return limiter.clone();
            }
        }

        let mut write_guard = self.limiters.write().await;
        if let Some(limiter) = write_guard.get(provider_id) {
            let mut guard = limiter.lock().await;
            guard.update_settings(settings);
            return limiter.clone();
        }

        let limiter = Arc::new(Mutex::new(ProviderRateLimiter::new(
            provider_id.to_string(),
            settings,
        )));
        write_guard.insert(provider_id.to_string(), limiter.clone());
        limiter
    }
}

/// Estimate token consumption from a request JSON body.
pub fn estimate_tokens(body: &Value) -> u32 {
    let mut char_count = 0usize;

    if let Some(messages) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in messages {
            if let Some(content) = msg.get("content") {
                if let Some(s) = content.as_str() {
                    char_count += s.len();
                } else if let Some(arr) = content.as_array() {
                    for part in arr {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            char_count += text.len();
                        }
                    }
                }
            }
        }
    }

    if let Some(prompt) = body.get("prompt").and_then(|p| p.as_str()) {
        char_count += prompt.len();
    }

    if let Some(input) = body.get("input") {
        if let Some(s) = input.as_str() {
            char_count += s.len();
        } else if let Some(arr) = input.as_array() {
            for item in arr {
                if let Some(s) = item.as_str() {
                    char_count += s.len();
                }
            }
        }
    }

    let estimated = (char_count as f64 / 3.5).ceil() as u32;
    estimated.clamp(200, 100_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_disabled_rate_limiter_allows_all() {
        let settings = RateLimitSettings {
            enabled: false,
            rpm: 1,
            tpm: 10,
            adaptive: false,
        };
        let mut limiter = ProviderRateLimiter::new("prov_disabled".into(), &settings);
        for _ in 0..10 {
            let res = limiter.acquire(1000, Duration::from_millis(50)).await;
            assert!(res.is_ok());
        }
    }

    #[tokio::test]
    async fn test_rpm_token_consumption_and_replenishment() {
        let settings = RateLimitSettings {
            enabled: true,
            rpm: 60, // 1 token per second
            tpm: 100_000,
            adaptive: false,
        };
        let mut limiter = ProviderRateLimiter::new("prov_rpm".into(), &settings);

        // Consume initial tokens
        assert!(limiter
            .acquire(100, Duration::from_millis(10))
            .await
            .is_ok());
        assert_eq!(limiter.available_rpm_tokens().floor(), 59.0);
    }

    #[tokio::test]
    async fn test_adaptive_429_throttling_and_recovery() {
        let settings = RateLimitSettings {
            enabled: true,
            rpm: 60,
            tpm: 100_000,
            adaptive: true,
        };
        let mut limiter = ProviderRateLimiter::new("prov_adapt".into(), &settings);

        // Simulate 429
        limiter.on_429(Some(Duration::from_millis(5)));
        assert!(limiter.current_rpm() < 60.0, "RPM should be throttled down");
        assert!(
            limiter.current_tpm() < 100_000.0,
            "TPM should be throttled down"
        );

        // Wait for cooldown
        tokio::time::sleep(Duration::from_millis(10)).await;

        // On success, rates should gradually recover
        let throttled_rpm = limiter.current_rpm();
        limiter.on_success();
        assert!(
            limiter.current_rpm() >= throttled_rpm,
            "RPM should begin recovering"
        );
    }

    #[tokio::test]
    async fn test_provider_isolation() {
        let registry = RateLimiterRegistry::new();
        let settings_a = RateLimitSettings {
            enabled: true,
            rpm: 10,
            tpm: 10_000,
            adaptive: true,
        };
        let settings_b = RateLimitSettings {
            enabled: true,
            rpm: 100,
            tpm: 200_000,
            adaptive: false,
        };

        let lim_a = registry.get_or_create("prov_a", &settings_a).await;
        let lim_b = registry.get_or_create("prov_b", &settings_b).await;

        {
            let mut guard_a = lim_a.lock().await;
            guard_a.on_429(Some(Duration::from_millis(10)));
            assert!(guard_a.current_rpm() < 10.0);
        }

        {
            let guard_b = lim_b.lock().await;
            assert_eq!(
                guard_b.current_rpm(),
                100.0,
                "Provider B must remain unaffected by Provider A's 429"
            );
        }
    }

    #[test]
    fn test_estimate_tokens() {
        let body = serde_json::json!({
            "messages": [
                {"role": "system", "content": "You are a helpful coding assistant."},
                {"role": "user", "content": "Write a python script to parse logs."}
            ]
        });
        let tokens = estimate_tokens(&body);
        assert!(tokens >= 200);
        assert!(tokens < 500);
    }
    #[tokio::test]
    async fn test_non_adaptive_does_not_downscale_rates_on_429() {
        let settings = RateLimitSettings {
            enabled: true,
            rpm: 60,
            tpm: 100_000,
            adaptive: false,
        };
        let mut limiter = ProviderRateLimiter::new("prov_no_adapt".into(), &settings);
        limiter.on_429(Some(Duration::from_millis(5)));
        assert_eq!(
            limiter.current_rpm(),
            60.0,
            "RPM should not change when adaptive is false"
        );
        assert_eq!(
            limiter.current_tpm(),
            100_000.0,
            "TPM should not change when adaptive is false"
        );
    }

    #[tokio::test]
    async fn test_update_settings_and_registry_reuse() {
        let registry = RateLimiterRegistry::new();
        let settings_v1 = RateLimitSettings {
            enabled: true,
            rpm: 30,
            tpm: 50_000,
            adaptive: false,
        };
        let lim = registry.get_or_create("prov_dynamic", &settings_v1).await;
        {
            let guard = lim.lock().await;
            assert_eq!(guard.configured_rpm, 30);
        }

        let settings_v2 = RateLimitSettings {
            enabled: true,
            rpm: 120,
            tpm: 200_000,
            adaptive: true,
        };
        let lim2 = registry.get_or_create("prov_dynamic", &settings_v2).await;
        {
            let guard = lim2.lock().await;
            assert_eq!(guard.configured_rpm, 120);
            assert_eq!(guard.configured_tpm, 200_000);
            assert!(guard.adaptive);
        }
    }
}
