//! Cross-platform auto-launch on boot.

use crate::error::AppResult;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AutoLaunchStatus {
    pub enabled: bool,
    pub method: String,
}

pub fn get_status() -> AutoLaunchStatus {
    AutoLaunchStatus {
        enabled: false,
        method: "registry".into(),
    }
}

pub fn set_enabled(enabled: bool) -> AppResult<()> {
    #[cfg(windows)]
    {
        let _ = enabled;
        // TODO: Windows registry HKCU\Software\Microsoft\Windows\CurrentVersion\Run
        tracing::info!("auto-launch set to {enabled}");
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        tracing::info!("auto-launch set to {enabled} (not implemented on this platform)");
    }
    Ok(())
}
