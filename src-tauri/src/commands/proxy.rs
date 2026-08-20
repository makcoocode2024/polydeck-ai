use tauri::command;

#[command]
pub async fn ad_detect_proxy() -> Result<serde_json::Value, String> {
    let manager = polydeck_core::proxy_manager::ProxyManager::new();
    let status = manager.get_status();
    serde_json::to_value(status).map_err(|e| e.to_string())
}