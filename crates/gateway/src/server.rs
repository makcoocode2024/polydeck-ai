//! Gateway server implementation

use crate::{
    client::UpstreamClient,
    config::GatewayConfig,
    failover::{FailoverManager, FailoverSlot},
    health::HealthState,
    middleware::MiddlewareState,
    model_rewrite::ModelRewriter,
    router::{build_router, AppState},
};
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use tokio::task::JoinHandle;
use tracing::info;

pub struct GatewayServer {
    config: GatewayConfig,
    actual_addr: Option<SocketAddr>,
    handle: Option<JoinHandle<Result<(), std::io::Error>>>,
    failover: Option<FailoverSlot>,
}

impl GatewayServer {
    pub fn new(config: GatewayConfig) -> Self {
        Self { config, actual_addr: None, handle: None, failover: None }
    }

    pub fn with_failover_slot(mut self, failover: FailoverSlot) -> Self {
        self.failover = Some(failover); self
    }

    pub fn with_failover(self, failover: Arc<FailoverManager>) -> Self {
        self.with_failover_slot(FailoverSlot::from_manager(failover))
    }

    pub fn failover(&self) -> Option<&FailoverSlot> { self.failover.as_ref() }

    pub async fn start(&mut self) -> Result<SocketAddr, String> {
        if self.is_running() {
            return Err(format!("Gateway already running on {}",
                self.actual_addr.map(|a| a.to_string()).unwrap_or_else(|| "unknown address".into())));
        }
        let upstream = UpstreamClient::new(
            self.config.upstream.base_url.clone(),
            self.config.upstream.api_key.clone(),
            self.config.timeout, self.config.max_retries,
        )?;
        let rewriter = ModelRewriter::new(&self.config.model_rewrites)?;
        let health_state = HealthState::new();
        let rate_limiter_registry = Arc::new(crate::rate_limiter::RateLimiterRegistry::new());
        let app_state = Arc::new(AppState {
            upstream, rewriter,
            health: health_state.clone(),
            failover: self.failover.clone(),
            responses_mode: self.config.upstream.responses_mode,
            responses_native: Arc::new(OnceLock::new()),
            max_price_per_request: self.config.upstream.max_price_per_request,
            rate_limiter_registry,
            primary_provider_id: self.config.upstream.provider_id.clone().unwrap_or_else(|| "primary_provider".into()),
            rate_limit_settings: self.config.upstream.rate_limit.clone(),
            max_retries: self.config.max_retries,
            default_effort_level: self.config.upstream.default_effort_level.clone(),
        });
        let middleware_state = Arc::new(MiddlewareState {
            local_token: self.config.upstream.local_token.clone(),
            upstream_api_key: self.config.upstream.api_key.clone(),
        });
        let app = build_router(app_state, middleware_state);
        let bind_addr = self.config.listen_addr
            .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 0)));
        if !bind_addr.ip().is_loopback() {
            return Err(format!("Gateway must bind to a loopback address, got {}", bind_addr));
        }
        let listener = tokio::net::TcpListener::bind(bind_addr).await
            .map_err(|e| format!("Failed to bind to {}: {}", bind_addr, e))?;
        let actual_addr = listener.local_addr()
            .map_err(|e| format!("Failed to get local address: {}", e))?;
        info!("Gateway listening on {}", actual_addr);
        let handle = tokio::spawn(async move {
            axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await
        });
        self.actual_addr = Some(actual_addr);
        self.handle = Some(handle);
        Ok(actual_addr)
    }

    pub async fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
            let _ = handle.await;
            info!("Gateway stopped");
        }
        self.actual_addr = None;
    }

    pub fn addr(&self) -> Option<SocketAddr> { self.actual_addr }
    pub fn port(&self) -> Option<u16> { self.actual_addr.map(|a| a.port()) }
    pub fn is_running(&self) -> bool {
        self.handle.is_some() && !self.handle.as_ref().unwrap().is_finished()
    }
}

impl Drop for GatewayServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() { handle.abort(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ResponsesMode, UpstreamConfig};
    use std::time::Duration;

    fn test_config() -> GatewayConfig {
        GatewayConfig {
            listen_addr: None,
            upstream: UpstreamConfig {
                provider_id: Some("test-provider".into()),
                base_url: "http://localhost:8080".to_string(),
                api_key: "test-key".to_string(),
                protocol: "openai".to_string(),
                local_token: "local-test-token".to_string(),
                max_price_per_request: None,
                responses_mode: ResponsesMode::Auto,
                rate_limit: polydeck_core::profile::RateLimitSettings::default(),
                default_effort_level: None,
            },
            model_rewrites: crate::model_rewrite::generate_provider_model_rewrites(
                &["gpt-4o".to_string(), "claude-3-5-sonnet".to_string()],
                false,
            ),
            timeout: Duration::from_secs(30),
            max_retries: 3,
        }
    }

    #[tokio::test]
    async fn creates_server() {
        let server = GatewayServer::new(test_config());
        assert!(!server.is_running());
    }

    #[tokio::test]
    async fn starts_and_binds_port() {
        let mut server = GatewayServer::new(test_config());
        let addr = server.start().await.unwrap();
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
        assert!(addr.port() > 0);
        assert!(server.is_running());
        server.stop().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!server.is_running());
    }

    #[tokio::test]
    async fn refuses_non_loopback() {
        let mut config = test_config();
        config.listen_addr = Some(SocketAddr::from(([0, 0, 0, 0], 0)));
        let mut server = GatewayServer::new(config);
        let error = server.start().await.unwrap_err();
        assert!(error.contains("loopback"));
    }

    #[tokio::test]
    async fn port_is_none_before_start() {
        assert_eq!(GatewayServer::new(test_config()).port(), None);
    }
}
