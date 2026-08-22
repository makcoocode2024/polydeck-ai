//! System tray state management.
//! Real-time health status display: Healthy/Degraded/Failed/Offline.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum TrayStatus {
    Healthy,
    Degraded,
    Failed,
    Offline,
}

pub struct TrayState {
    pub status: TrayStatus,
    pub gateway_running: bool,
    pub active_profile: Option<String>,
}

impl Default for TrayState {
    fn default() -> Self {
        Self::new()
    }
}

impl TrayState {
    pub fn new() -> Self {
        Self {
            status: TrayStatus::Offline,
            gateway_running: false,
            active_profile: None,
        }
    }

    pub fn update_status(&mut self, gateway: bool, failover_ok: bool) {
        self.gateway_running = gateway;
        self.status = match (gateway, failover_ok) {
            (true, true) => TrayStatus::Healthy,
            (true, false) => TrayStatus::Degraded,
            (false, _) => TrayStatus::Offline,
        };
    }
}

pub fn render_status_icon(_status: TrayStatus) -> Vec<u8> {
    vec![] // TODO: Generate tray icon based on status
}
