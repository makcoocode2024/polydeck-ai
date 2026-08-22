use polydeck_core::client_detector::DetectedClient;
use tauri::command;

#[command]
pub fn ad_get_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[command]
pub fn ad_ping() -> String {
    "pong".to_string()
}

#[command]
pub async fn ad_detect_clients() -> Result<Vec<DetectedClient>, String> {
    Ok(polydeck_core::client_detector::detect_all())
}
