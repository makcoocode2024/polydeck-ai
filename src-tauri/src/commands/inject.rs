use crate::state::InjectState;
use tauri::{command, State};

#[command]
pub async fn ad_inject_status(inject: State<'_, InjectState>) -> Result<serde_json::Value, String> {
    let inject = inject.lock().await;
    let status = inject.status();
    serde_json::to_value(status).map_err(|e| e.to_string())
}

#[command]
pub async fn ad_inject_install_native(inject: State<'_, InjectState>) -> Result<serde_json::Value, String> {
    let mut inject = inject.lock().await;
    let status = inject.install_native()?;
    serde_json::to_value(status).map_err(|e| e.to_string())
}

#[command]
pub async fn ad_inject_uninstall_native(inject: State<'_, InjectState>) -> Result<serde_json::Value, String> {
    let mut inject = inject.lock().await;
    let status = inject.uninstall_native()?;
    serde_json::to_value(status).map_err(|e| e.to_string())
}

#[command]
pub async fn ad_inject_repair(inject: State<'_, InjectState>) -> Result<serde_json::Value, String> {
    let mut inject = inject.lock().await;
    let status = inject.repair()?;
    serde_json::to_value(status).map_err(|e| e.to_string())
}
