//! Provider health monitoring, circuit breaking, and ordered failover.

use crate::client::{Endpoint, UpstreamClient, UpstreamError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, watch, Mutex, RwLock};

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub default_model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderHealth {
    Healthy,
    Degraded,
    Failed,
    CircuitOpen,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub provider_id: String,
    pub state: ProviderHealth,
    pub circuit: CircuitState,
    pub consecutive_failures: u32,
    pub consecutive_successes: u32,
    pub latency_ms: Option<u64>,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

impl HealthStatus {
    fn unknown(provider_id: String) -> Self {
        Self {
            provider_id,
            state: ProviderHealth::Unknown,
            circuit: CircuitState::Closed,
            consecutive_failures: 0,
            consecutive_successes: 0,
            latency_ms: None,
            last_checked_at: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverEvent {
    pub timestamp: DateTime<Utc>,
    pub from_provider_id: String,
    pub to_provider_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverStatus {
    pub profile_id: String,
    pub running: bool,
    pub primary_provider_id: String,
    pub current_provider_id: String,
    pub on_backup: bool,
    pub all_providers_failed: bool,
    pub providers: Vec<HealthStatus>,
}

#[derive(Debug, Clone)]
pub struct FailoverOptions {
    pub threshold: u32,
    pub recovery_threshold: u32,
    pub check_interval: Duration,
    pub cooldown: Duration,
    pub auto_failback: bool,
}

impl Default for FailoverOptions {
    fn default() -> Self {
        Self {
            threshold: 3,
            recovery_threshold: 2,
            check_interval: Duration::from_secs(30),
            cooldown: Duration::from_secs(60),
            auto_failback: false,
        }
    }
}

struct State {
    current_provider_id: String,
    health: HashMap<String, HealthStatus>,
    opened_at: HashMap<String, Instant>,
    history: Vec<FailoverEvent>,
    running: bool,
    all_providers_failed: bool,
}

#[derive(Clone, Default)]
pub struct FailoverSlot {
    manager: Arc<RwLock<Option<Arc<FailoverManager>>>>,
}

impl FailoverSlot {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn from_manager(manager: Arc<FailoverManager>) -> Self {
        Self {
            manager: Arc::new(RwLock::new(Some(manager))),
        }
    }
    pub async fn replace(
        &self,
        manager: Option<Arc<FailoverManager>>,
    ) -> Option<Arc<FailoverManager>> {
        std::mem::replace(&mut *self.manager.write().await, manager)
    }
    pub async fn get(&self) -> Option<Arc<FailoverManager>> {
        self.manager.read().await.clone()
    }
}

pub struct FailoverManager {
    profile_id: String,
    primary: ProviderConfig,
    backups: Vec<ProviderConfig>,
    options: FailoverOptions,
    clients: HashMap<String, UpstreamClient>,
    state: Arc<RwLock<State>>,
    status_tx: watch::Sender<FailoverStatus>,
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
}

impl FailoverManager {
    pub fn new(
        profile_id: String,
        primary: ProviderConfig,
        backups: Vec<ProviderConfig>,
        options: FailoverOptions,
    ) -> Result<Arc<Self>, String> {
        if options.threshold == 0 || options.recovery_threshold == 0 {
            return Err("Failover thresholds must be at least 1".into());
        }
        let mut clients = HashMap::new();
        let mut health = HashMap::new();
        for provider in std::iter::once(&primary).chain(backups.iter()) {
            if clients.contains_key(&provider.id) {
                return Err(format!("Duplicate failover provider id '{}'", provider.id));
            }
            clients.insert(
                provider.id.clone(),
                UpstreamClient::new(
                    provider.base_url.clone(),
                    provider.api_key.clone(),
                    Duration::from_secs(120),
                    0,
                )?,
            );
            health.insert(
                provider.id.clone(),
                HealthStatus::unknown(provider.id.clone()),
            );
        }
        let initial = FailoverStatus {
            profile_id: profile_id.clone(),
            running: false,
            primary_provider_id: primary.id.clone(),
            current_provider_id: primary.id.clone(),
            on_backup: false,
            all_providers_failed: false,
            providers: health.values().cloned().collect(),
        };
        let (status_tx, _) = watch::channel(initial);
        Ok(Arc::new(Self {
            profile_id,
            state: Arc::new(RwLock::new(State {
                current_provider_id: primary.id.clone(),
                health,
                opened_at: HashMap::new(),
                history: Vec::new(),
                running: false,
                all_providers_failed: false,
            })),
            primary,
            backups,
            options,
            clients,
            status_tx,
            shutdown: Mutex::new(None),
        }))
    }

    fn chain(&self) -> impl Iterator<Item = &ProviderConfig> {
        std::iter::once(&self.primary).chain(self.backups.iter())
    }

    pub async fn current_client(&self) -> Result<(String, UpstreamClient), String> {
        let id = self.state.read().await.current_provider_id.clone();
        self.clients
            .get(&id)
            .cloned()
            .map(|client| (id.clone(), client))
            .ok_or_else(|| format!("Current provider '{}' has no client", id))
    }

    pub async fn report_success(&self, provider_id: &str, latency_ms: u64) {
        let mut state = self.state.write().await;
        if let Some(health) = state.health.get_mut(provider_id) {
            health.state = ProviderHealth::Healthy;
            health.circuit = CircuitState::Closed;
            health.consecutive_failures = 0;
            health.consecutive_successes = health.consecutive_successes.saturating_add(1);
            health.latency_ms = Some(latency_ms);
            health.last_checked_at = Some(Utc::now());
            health.last_error = None;
            state.opened_at.remove(provider_id);
        }
        self.publish_locked(&state);
    }

    pub async fn report_failure(
        &self,
        provider_id: &str,
        error: impl Into<String>,
    ) -> Option<String> {
        let error = error.into();
        let mut should_switch = false;
        {
            let mut state = self.state.write().await;
            if let Some(health) = state.health.get_mut(provider_id) {
                health.consecutive_failures = health.consecutive_failures.saturating_add(1);
                health.consecutive_successes = 0;
                health.state = ProviderHealth::Degraded;
                health.last_checked_at = Some(Utc::now());
                health.last_error = Some(error.clone());
                if health.consecutive_failures >= self.options.threshold {
                    health.state = ProviderHealth::CircuitOpen;
                    health.circuit = CircuitState::Open;
                    state
                        .opened_at
                        .insert(provider_id.to_string(), Instant::now());
                    should_switch = state.current_provider_id == provider_id;
                }
            }
            self.publish_locked(&state);
        }
        if should_switch {
            self.switch_to_next(provider_id, &error).await
        } else {
            None
        }
    }

    async fn switch_to_next(&self, from: &str, reason: &str) -> Option<String> {
        let candidates: Vec<String> = self
            .chain()
            .map(|p| p.id.clone())
            .filter(|id| id != from)
            .collect();
        for candidate in candidates {
            if !self.can_attempt(&candidate).await {
                continue;
            }
            if self.probe(&candidate).await.is_ok() {
                let mut state = self.state.write().await;
                state.current_provider_id = candidate.clone();
                state.all_providers_failed = false;
                state.history.push(FailoverEvent {
                    timestamp: Utc::now(),
                    from_provider_id: from.to_string(),
                    to_provider_id: candidate.clone(),
                    reason: reason.to_string(),
                });
                self.publish_locked(&state);
                return Some(candidate);
            }
        }
        let mut state = self.state.write().await;
        state.all_providers_failed = true;
        self.publish_locked(&state);
        None
    }

    async fn can_attempt(&self, provider_id: &str) -> bool {
        let mut state = self.state.write().await;
        let circuit = state.health.get(provider_id).map(|h| h.circuit);
        match circuit {
            Some(CircuitState::Open) => {
                let cooled = state
                    .opened_at
                    .get(provider_id)
                    .map(|at| at.elapsed() >= self.options.cooldown)
                    .unwrap_or(true);
                if cooled {
                    if let Some(health) = state.health.get_mut(provider_id) {
                        health.circuit = CircuitState::HalfOpen;
                        health.state = ProviderHealth::Degraded;
                    }
                    true
                } else {
                    false
                }
            }
            Some(CircuitState::HalfOpen) => false,
            Some(CircuitState::Closed) => true,
            None => false,
        }
    }

    async fn probe(&self, provider_id: &str) -> Result<u64, String> {
        let provider = self
            .chain()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| format!("Unknown provider '{}'", provider_id))?;
        let client = crate::client::build_http_client(&provider.base_url, PROBE_TIMEOUT)?;
        let start = Instant::now();
        let models_url = format!("{}/v1/models", provider.base_url.trim_end_matches('/'));
        let models = client
            .get(models_url)
            .bearer_auth(&provider.api_key)
            .send()
            .await;
        let mut result = match &models {
            Ok(response) if response.status().is_success() => Ok(()),
            Ok(response) => Err(format!("models probe returned {}", response.status())),
            Err(error) => Err(error.to_string()),
        };
        let answered = models.is_ok();
        if result.is_err() && answered && !provider.default_model.is_empty() {
            let chat_url = format!(
                "{}/v1/chat/completions",
                provider.base_url.trim_end_matches('/')
            );
            let chat = client.post(chat_url).bearer_auth(&provider.api_key).json(&json!({
                "model": provider.default_model, "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 1, "stream": false
            })).send().await;
            result = match chat {
                Ok(r) if r.status().is_success() => Ok(()),
                Ok(r) => Err(format!("probe returned {}", r.status())),
                Err(e) => Err(e.to_string()),
            };
        }
        let latency = start.elapsed().as_millis() as u64;
        match result {
            Ok(()) => {
                self.report_success(provider_id, latency).await;
                Ok(latency)
            }
            Err(error) => {
                self.report_probe_failure(provider_id, &error, latency)
                    .await;
                Err(error)
            }
        }
    }

    async fn report_probe_failure(&self, provider_id: &str, error: &str, latency: u64) {
        let mut state = self.state.write().await;
        if let Some(health) = state.health.get_mut(provider_id) {
            health.consecutive_failures = health.consecutive_failures.saturating_add(1);
            health.consecutive_successes = 0;
            health.state = ProviderHealth::Failed;
            health.latency_ms = Some(latency);
            health.last_checked_at = Some(Utc::now());
            health.last_error = Some(error.to_string());
            if health.consecutive_failures >= self.options.threshold
                || health.circuit == CircuitState::HalfOpen
            {
                health.circuit = CircuitState::Open;
                health.state = ProviderHealth::CircuitOpen;
                state
                    .opened_at
                    .insert(provider_id.to_string(), Instant::now());
            }
        }
        self.publish_locked(&state);
    }

    pub async fn start(self: &Arc<Self>) {
        self.stop().await;
        let (tx, mut rx) = oneshot::channel();
        *self.shutdown.lock().await = Some(tx);
        {
            let mut state = self.state.write().await;
            state.running = true;
            self.publish_locked(&state);
        }
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(manager.options.check_interval);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = interval.tick() => manager.monitor_once().await,
                    _ = &mut rx => break,
                }
            }
            let mut state = manager.state.write().await;
            state.running = false;
            manager.publish_locked(&state);
        });
    }

    async fn monitor_once(&self) {
        let current = self.state.read().await.current_provider_id.clone();
        if self.can_attempt(&current).await {
            if let Err(error) = self.probe(&current).await {
                let failures = self
                    .state
                    .read()
                    .await
                    .health
                    .get(&current)
                    .map(|h| h.consecutive_failures)
                    .unwrap_or(0);
                if failures >= self.options.threshold {
                    self.switch_to_next(&current, &error).await;
                }
            }
        }
        if self.options.auto_failback
            && current != self.primary.id
            && self.can_attempt(&self.primary.id).await
            && self.probe(&self.primary.id).await.is_ok()
        {
            let successes = self.state.read().await.health[&self.primary.id].consecutive_successes;
            if successes >= self.options.recovery_threshold {
                let mut state = self.state.write().await;
                state.current_provider_id = self.primary.id.clone();
                state.history.push(FailoverEvent {
                    timestamp: Utc::now(),
                    from_provider_id: current,
                    to_provider_id: self.primary.id.clone(),
                    reason: "primary provider recovered".into(),
                });
                self.publish_locked(&state);
            }
        }
    }

    pub async fn stop(&self) {
        if let Some(tx) = self.shutdown.lock().await.take() {
            let _ = tx.send(());
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<FailoverStatus> {
        self.status_tx.subscribe()
    }

    pub async fn status(&self) -> FailoverStatus {
        let state = self.state.read().await;
        self.snapshot(&state)
    }

    pub async fn provider_health(&self, provider_id: &str) -> Option<HealthStatus> {
        self.state.read().await.health.get(provider_id).cloned()
    }

    pub async fn history(&self, limit: u32) -> Vec<FailoverEvent> {
        self.state
            .read()
            .await
            .history
            .iter()
            .rev()
            .take(limit as usize)
            .cloned()
            .collect()
    }

    fn snapshot(&self, state: &State) -> FailoverStatus {
        let providers: Vec<_> = self
            .chain()
            .filter_map(|p| state.health.get(&p.id).cloned())
            .collect();
        FailoverStatus {
            profile_id: self.profile_id.clone(),
            running: state.running,
            primary_provider_id: self.primary.id.clone(),
            current_provider_id: state.current_provider_id.clone(),
            on_backup: state.current_provider_id != self.primary.id,
            all_providers_failed: state.all_providers_failed,
            providers,
        }
    }

    fn publish_locked(&self, state: &State) {
        self.status_tx.send_replace(self.snapshot(state));
    }

    pub async fn send(
        &self,
        endpoint: Endpoint,
        body: Value,
    ) -> Result<(String, reqwest::Response), UpstreamError> {
        let (provider_id, client) = self.current_client().await.map_err(|error| UpstreamError {
            message: error,
            never_sent: true,
        })?;
        let start = Instant::now();
        match client.send(endpoint, body).await {
            Ok(response)
                if response.status().is_server_error()
                    || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS =>
            {
                let status = response.status();
                self.report_failure(&provider_id, format!("upstream returned {}", status))
                    .await;
                Ok((provider_id, response))
            }
            Ok(response) => {
                self.report_success(&provider_id, start.elapsed().as_millis() as u64)
                    .await;
                Ok((provider_id, response))
            }
            Err(error) => {
                self.report_failure(&provider_id, error.message.clone())
                    .await;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(id: &str) -> ProviderConfig {
        ProviderConfig {
            id: id.into(),
            name: id.into(),
            base_url: "http://127.0.0.1:9".into(),
            api_key: "k".into(),
            default_model: "m".into(),
        }
    }

    fn manager(threshold: u32) -> Arc<FailoverManager> {
        FailoverManager::new(
            "profile".into(),
            provider("primary"),
            vec![provider("backup")],
            FailoverOptions {
                threshold,
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[tokio::test]
    async fn starts_on_primary() {
        let status = manager(3).status().await;
        assert_eq!(status.current_provider_id, "primary");
        assert!(!status.on_backup);
    }

    #[tokio::test]
    async fn failure_below_threshold_degrades() {
        let m = manager(3);
        m.report_failure("primary", "down").await;
        let h = m.provider_health("primary").await.unwrap();
        assert_eq!(h.state, ProviderHealth::Degraded);
        assert_eq!(h.consecutive_failures, 1);
    }

    #[tokio::test]
    async fn success_resets_failures() {
        let m = manager(3);
        m.report_failure("primary", "down").await;
        m.report_success("primary", 4).await;
        let h = m.provider_health("primary").await.unwrap();
        assert_eq!(h.consecutive_failures, 0);
    }

    #[tokio::test]
    async fn threshold_opens_circuit() {
        let m = manager(2);
        m.report_failure("primary", "one").await;
        m.report_failure("primary", "two").await;
        let h = m.provider_health("primary").await.unwrap();
        assert_eq!(h.circuit, CircuitState::Open);
    }

    #[test]
    fn rejects_zero_threshold() {
        assert!(FailoverManager::new(
            "p".into(),
            provider("a"),
            vec![],
            FailoverOptions {
                threshold: 0,
                ..Default::default()
            }
        )
        .is_err());
    }

    #[test]
    fn rejects_duplicate_provider_ids() {
        assert!(FailoverManager::new(
            "p".into(),
            provider("a"),
            vec![provider("a")],
            Default::default()
        )
        .is_err());
    }

    #[tokio::test]
    async fn slot_starts_empty() {
        assert!(FailoverSlot::new().get().await.is_none());
    }
}
