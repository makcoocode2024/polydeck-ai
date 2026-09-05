//! Shared Tauri application state

use polydeck_core::profile::ProfileManager;
use polydeck_gateway::{FailoverSlot, GatewayServer};
use polydeck_inject::InjectionManager;
use std::sync::Arc;
use tokio::sync::Mutex;

pub type GatewayState = Arc<Mutex<Option<GatewayServer>>>;
pub type ProfileState = Arc<Mutex<ProfileManager>>;
pub type InjectState = Arc<Mutex<InjectionManager>>;

/// The failover manager the running gateway is using, if any.
///
/// Held outside `GatewayState` so `ad_failover_status` can read it without taking
/// the gateway lock, and so it survives a route hot-swap. The slot is the same one
/// handed to `GatewayServer`, so what the IPC layer reports is what the router is
/// actually routing through — previously `ad_failover_status` returned a hardcoded
/// `running: false` because no manager was ever constructed.
pub type FailoverState = Arc<FailoverSlot>;
