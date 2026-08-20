//! AI Deck Gateway - Local HTTP proxy for AI API requests
//!
//! Provides protocol conversion, model name rewriting, failover, and request
//! proxying for AI service APIs.

pub mod client;
pub mod config;
pub mod failover;
pub mod health;
pub mod middleware;
pub mod model_rewrite;
pub mod rate_limiter;
pub mod replay;
pub mod router;
pub mod server;
pub mod stream_adapter;

pub use config::{GatewayConfig, ModelRewriteRule, ResponsesMode, UpstreamConfig};
pub use failover::{
    FailoverEvent, FailoverManager, FailoverOptions, FailoverSlot, FailoverStatus, HealthStatus,
};
pub use server::GatewayServer;

pub use rate_limiter::{estimate_tokens, ProviderRateLimiter, RateLimiterRegistry};
