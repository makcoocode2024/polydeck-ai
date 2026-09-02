//! Gateway server implementation

use crate::{
    client::UpstreamClient,
    config::{GatewayConfig, RouteConfig},
    failover::{FailoverManager, FailoverSlot},
    health::HealthState,
    middleware::{RouteTable, SharedRouteTable},
    model_rewrite::ModelRewriter,
    router::{build_router, AppState},
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use tokio::task::JoinHandle;
use tracing::info;

pub struct GatewayServer {
    config: GatewayConfig,
    actual_addr: Option<SocketAddr>,
    handle: Option<JoinHandle<Result<(), std::io::Error>>>,
    failover: Option<FailoverSlot>,
    /// Shared with the running router, so [`GatewayServer::apply_routes`] can
    /// replace the routing without restarting the listener.
    table: SharedRouteTable,
    /// Counters shared by every route: they describe this listener, not a profile.
    health: HealthState,
    rate_limiter_registry: Arc<crate::rate_limiter::RateLimiterRegistry>,
}

impl GatewayServer {
    pub fn new(config: GatewayConfig) -> Self {
        Self {
            config,
            actual_addr: None,
            handle: None,
            failover: None,
            table: Arc::new(tokio::sync::RwLock::new(RouteTable::default())),
            health: HealthState::new(),
            rate_limiter_registry: Arc::new(crate::rate_limiter::RateLimiterRegistry::new()),
        }
    }

    pub fn with_failover_slot(mut self, failover: FailoverSlot) -> Self {
        self.failover = Some(failover);
        self
    }

    pub fn with_failover(self, failover: Arc<FailoverManager>) -> Self {
        self.with_failover_slot(FailoverSlot::from_manager(failover))
    }

    pub fn failover(&self) -> Option<&FailoverSlot> {
        self.failover.as_ref()
    }

    pub async fn start(&mut self) -> Result<SocketAddr, String> {
        if self.is_running() {
            return Err(format!(
                "Gateway already running on {}",
                self.actual_addr
                    .map(|a| a.to_string())
                    .unwrap_or_else(|| "unknown address".into())
            ));
        }
        let table = self.compile_routes(&self.config.routes)?;
        *self.table.write().await = table;

        let app = build_router(self.health.clone(), Arc::clone(&self.table));
        let bind_addr = self
            .config
            .listen_addr
            .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 0)));
        if !bind_addr.ip().is_loopback() {
            return Err(format!(
                "Gateway must bind to a loopback address, got {}",
                bind_addr
            ));
        }
        let listener = tokio::net::TcpListener::bind(bind_addr)
            .await
            .map_err(|e| format!("Failed to bind to {}: {}", bind_addr, e))?;
        let actual_addr = listener
            .local_addr()
            .map_err(|e| format!("Failed to get local address: {}", e))?;
        info!("Gateway listening on {}", actual_addr);
        let handle = tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
        });
        self.actual_addr = Some(actual_addr);
        self.handle = Some(handle);
        Ok(actual_addr)
    }

    /// Turn route configs into a token → runtime table.
    ///
    /// Runtimes are deduplicated by upstream identity, so two clients on the same
    /// profile share one `AppState` and therefore one learned Auto-mode Responses
    /// probe — otherwise each would rediscover it, and one could sit in Bridge mode
    /// while the other used Native against the same upstream.
    fn compile_routes(&self, routes: &[RouteConfig]) -> Result<RouteTable, String> {
        let mut by_upstream: HashMap<String, Arc<AppState>> = HashMap::new();
        let mut entries: Vec<(String, Arc<AppState>)> = Vec::new();

        for route in routes {
            if route.upstream.local_token.trim().is_empty() {
                return Err(format!(
                    "client {} has no local token, so nothing could authenticate as it",
                    route.client_id
                ));
            }
            // Keyed on what actually determines behavior. Two routes with the same
            // provider and the same rewrites are the same runtime.
            let key = format!(
                "{}|{}|{:?}",
                route.upstream.provider_id.as_deref().unwrap_or_default(),
                route.upstream.base_url,
                route.model_rewrites
            );
            let state = match by_upstream.get(&key) {
                Some(existing) => Arc::clone(existing),
                None => {
                    let upstream = UpstreamClient::new(
                        route.upstream.base_url.clone(),
                        route.upstream.api_key.clone(),
                        self.config.timeout,
                        self.config.max_retries,
                    )?;
                    let state = Arc::new(AppState {
                        upstream,
                        rewriter: ModelRewriter::new(&route.model_rewrites)?,
                        health: self.health.clone(),
                        failover: self.failover.clone(),
                        responses_mode: route.upstream.responses_mode,
                        responses_native: Arc::new(OnceLock::new()),
                        max_price_per_request: route.upstream.max_price_per_request,
                        rate_limiter_registry: Arc::clone(&self.rate_limiter_registry),
                        primary_provider_id: route
                            .upstream
                            .provider_id
                            .clone()
                            .unwrap_or_else(|| "primary_provider".into()),
                        rate_limit_settings: route.upstream.rate_limit.clone(),
                        max_retries: self.config.max_retries,
                        default_effort_level: route.upstream.default_effort_level.clone(),
                        thinking_support: route.upstream.thinking_support,
                    });
                    by_upstream.insert(key, Arc::clone(&state));
                    state
                }
            };
            entries.push((route.upstream.local_token.clone(), state));
        }

        Ok(RouteTable::new(entries))
    }

    /// Swap the routing table without restarting the listener.
    ///
    /// The whole table is compiled before the swap, so a bad route leaves the
    /// running one untouched rather than half-applied. This is what lets a client be
    /// bound to a different profile without dropping in-flight requests on the
    /// others.
    pub async fn apply_routes(&mut self, routes: Vec<RouteConfig>) -> Result<(), String> {
        let compiled = self.compile_routes(&routes)?;
        *self.table.write().await = compiled;
        self.config.routes = routes;
        info!(
            "Gateway routing updated: {} client(s)",
            self.config.routes.len()
        );
        Ok(())
    }

    /// How many clients the running table serves.
    pub async fn route_count(&self) -> usize {
        self.table.read().await.len()
    }

    pub async fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
            let _ = handle.await;
            info!("Gateway stopped");
        }
        self.actual_addr = None;
    }

    pub fn addr(&self) -> Option<SocketAddr> {
        self.actual_addr
    }
    pub fn port(&self) -> Option<u16> {
        self.actual_addr.map(|a| a.port())
    }
    pub fn is_running(&self) -> bool {
        self.handle.is_some() && !self.handle.as_ref().unwrap().is_finished()
    }
}

impl Drop for GatewayServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ResponsesMode, UpstreamConfig};
    use std::time::Duration;

    fn test_upstream(token: &str, base_url: &str) -> UpstreamConfig {
        UpstreamConfig {
            provider_id: Some("test-provider".into()),
            base_url: base_url.to_string(),
            api_key: "test-key".to_string(),
            protocol: "openai".to_string(),
            local_token: token.to_string(),
            max_price_per_request: None,
            responses_mode: ResponsesMode::Auto,
            rate_limit: polydeck_core::profile::RateLimitSettings::default(),
            default_effort_level: None,
            thinking_support: polydeck_core::types::ThinkingSupport::default(),
        }
    }

    fn test_config() -> GatewayConfig {
        let mut config = GatewayConfig::single(
            test_upstream("local-test-token", "http://localhost:8080"),
            crate::model_rewrite::generate_provider_model_rewrites(
                &["gpt-4o".to_string(), "claude-3-5-sonnet".to_string()],
                false,
            ),
        );
        config.timeout = Duration::from_secs(30);
        config
    }

    /// A route with no token could never be authenticated as, so it is refused at
    /// compile time rather than becoming a silently unreachable entry.
    #[tokio::test]
    async fn a_route_without_a_token_is_refused() {
        let config = GatewayConfig::single(test_upstream("", "http://localhost:8080"), vec![]);
        let mut server = GatewayServer::new(config);
        let err = server.start().await.unwrap_err();
        assert!(err.contains("local token"), "实际报错：{err}");
    }

    /// Two clients on the same upstream must share one runtime, or each learns the
    /// Auto-mode Responses probe separately and they can disagree about the same
    /// upstream.
    #[tokio::test]
    async fn routes_sharing_an_upstream_share_one_runtime() {
        let mut config =
            GatewayConfig::single(test_upstream("adk_one", "http://localhost:8080"), vec![]);
        config.routes.push(crate::config::RouteConfig {
            client_id: "second".into(),
            upstream: test_upstream("adk_two", "http://localhost:8080"),
            model_rewrites: vec![],
        });

        let server = GatewayServer::new(config);
        let table = server.compile_routes(&server.config.routes).unwrap();
        let a = table.resolve("adk_one").unwrap();
        let b = table.resolve("adk_two").unwrap();
        assert!(
            Arc::ptr_eq(&a, &b),
            "同一上游的两个客户端应共用一个 runtime"
        );
    }

    /// Different upstreams must not be collapsed, which is the whole point.
    #[tokio::test]
    async fn routes_on_different_upstreams_stay_separate() {
        let mut config =
            GatewayConfig::single(test_upstream("adk_one", "http://localhost:8080"), vec![]);
        config.routes.push(crate::config::RouteConfig {
            client_id: "second".into(),
            upstream: test_upstream("adk_two", "http://localhost:9090"),
            model_rewrites: vec![],
        });

        let server = GatewayServer::new(config);
        let table = server.compile_routes(&server.config.routes).unwrap();
        assert!(!Arc::ptr_eq(
            &table.resolve("adk_one").unwrap(),
            &table.resolve("adk_two").unwrap()
        ));
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
