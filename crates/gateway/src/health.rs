//! Health check endpoint and monitoring

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct HealthState {
    active_connections: Arc<AtomicUsize>,
    total_requests: Arc<AtomicU64>,
    last_upstream_latency_ms: Arc<AtomicU64>,
}

impl HealthState {
    pub fn new() -> Self {
        Self {
            active_connections: Arc::new(AtomicUsize::new(0)),
            total_requests: Arc::new(AtomicU64::new(0)),
            last_upstream_latency_ms: Arc::new(AtomicU64::new(0)),
        }
    }
    pub fn increment_connections(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }
    pub fn decrement_connections(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }
    pub fn record_request(&self, upstream_latency_ms: u64) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        self.last_upstream_latency_ms
            .store(upstream_latency_ms, Ordering::Relaxed);
    }
    pub fn active_connections(&self) -> usize {
        self.active_connections.load(Ordering::Relaxed)
    }
    pub fn total_requests(&self) -> u64 {
        self.total_requests.load(Ordering::Relaxed)
    }
    pub fn last_upstream_latency_ms(&self) -> u64 {
        self.last_upstream_latency_ms.load(Ordering::Relaxed)
    }
}

impl Default for HealthState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub active_connections: usize,
    pub total_requests: u64,
    pub upstream_latency_ms: u64,
}

pub async fn health_check(State(state): State<Arc<HealthState>>) -> impl IntoResponse {
    let response = HealthResponse {
        status: "ok".to_string(),
        active_connections: state.active_connections(),
        total_requests: state.total_requests(),
        upstream_latency_ms: state.last_upstream_latency_ms(),
    };
    (StatusCode::OK, Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_connections() {
        let state = HealthState::new();
        assert_eq!(state.active_connections(), 0);
        state.increment_connections();
        assert_eq!(state.active_connections(), 1);
        state.decrement_connections();
        assert_eq!(state.active_connections(), 0);
    }

    #[test]
    fn records_requests() {
        let state = HealthState::new();
        state.record_request(50);
        assert_eq!(state.total_requests(), 1);
        assert_eq!(state.last_upstream_latency_ms(), 50);
    }
}
