use tauri::command;

#[command]
pub async fn ad_tray_status() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({"status": "idle"}))
}

#[command]
pub async fn ad_handle_deep_link(url: String) -> Result<serde_json::Value, String> {
    polydeck_core::deep_link::parse(&url)
        .map(|parsed| serde_json::to_value(parsed).unwrap_or_default())
        .map_err(|e| e.to_string())
}

#[command]
pub async fn ad_autolaunch_status() -> Result<serde_json::Value, String> {
    let status = polydeck_core::autolaunch::get_status();
    serde_json::to_value(status).map_err(|e| e.to_string())
}

#[command]
pub async fn ad_set_autolaunch(enabled: bool) -> Result<(), String> {
    polydeck_core::autolaunch::set_enabled(enabled).map_err(|e| e.to_string())
}
