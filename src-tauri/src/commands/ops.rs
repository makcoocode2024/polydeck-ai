use tauri::command;

#[command]
pub async fn ad_run_diagnostics() -> Result<serde_json::Value, String> {
    let report = polydeck_core::diagnostics::run_diagnostics()
        .await
        .map_err(|e| e.to_string())?;
    serde_json::to_value(report).map_err(|e| e.to_string())
}

#[command]
pub async fn ad_check_update() -> Result<serde_json::Value, String> {
    let mut store = polydeck_core::updater::UpdateStore::new();
    let update = store.check_for_update().await.map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "available": update.is_some(),
        "version": update,
    }))
}

/// The newest log entries, most recent first.
///
/// Returned `Ok(vec![])` unconditionally before, so the Settings page log view was
/// permanently empty while `~/.ai-deck/logs/` held real data. `LogStore::get_logs`
/// already did the reading and redaction; nothing called it.
#[command]
pub async fn ad_get_logs(limit: u32) -> Result<Vec<polydeck_core::logging::LogEntry>, String> {
    let store = polydeck_core::logging::LogStore::new().map_err(|e| e.to_string())?;
    store
        .get_logs(None, limit as usize)
        .map_err(|e| e.to_string())
}
