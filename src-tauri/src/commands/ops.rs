use tauri::command;

#[command]
pub async fn ad_run_diagnostics() -> Result<serde_json::Value, String> {
    let report = polydeck_core::diagnostics::run_diagnostics().await.map_err(|e| e.to_string())?;
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

#[command]
pub async fn ad_get_logs(limit: u32) -> Result<Vec<String>, String> {
    let _ = limit;
    Ok(vec![])
}