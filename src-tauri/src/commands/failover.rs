use tauri::command;

#[command]
pub async fn ad_failover_status() -> Result<serde_json::Value, String> {
    // TODO: integrate with FailoverManager via state
    Ok(serde_json::json!({"running": false, "providers": []}))
}

#[command]
pub async fn ad_failover_history(limit: u32) -> Result<Vec<serde_json::Value>, String> {
    let _ = limit;
    Ok(vec![])
}
