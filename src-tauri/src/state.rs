//! Shared Tauri application state

use polydeck_core::profile::ProfileManager;
use polydeck_gateway::GatewayServer;
use polydeck_inject::InjectionManager;
use std::sync::Arc;
use tokio::sync::Mutex;

pub type GatewayState = Arc<Mutex<Option<GatewayServer>>>;
pub type ProfileState = Arc<Mutex<ProfileManager>>;
pub type InjectState = Arc<Mutex<InjectionManager>>;
