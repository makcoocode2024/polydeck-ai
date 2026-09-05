use crate::state::FailoverState;
use tauri::{command, State};

/// The live circuit-breaker state, or an explicit "not running" snapshot.
///
/// This returned a hardcoded `{"running": false, "providers": []}` before, which
/// was indistinguishable from a healthy idle gateway and hid the fact that no
/// `FailoverManager` was ever constructed. It now reads the same slot the router
/// routes through, so the health, circuit state, and current upstream reported here
/// are the ones actually in effect.
#[command]
pub async fn ad_failover_status(
    failover: State<'_, FailoverState>,
) -> Result<serde_json::Value, String> {
    match failover.get().await {
        Some(manager) => serde_json::to_value(manager.status().await).map_err(|e| e.to_string()),
        // No manager means no bound profile enabled failover, which is a valid
        // state rather than an error. Shaped like FailoverStatus so the frontend
        // has one type to read.
        None => Ok(serde_json::json!({
            "profile_id": "",
            "running": false,
            "primary_provider_id": "",
            "current_provider_id": "",
            "on_backup": false,
            "all_providers_failed": false,
            "providers": [],
        })),
    }
}

/// Recorded provider switches, newest first. Empty when failover is not running.
#[command]
pub async fn ad_failover_history(
    limit: u32,
    failover: State<'_, FailoverState>,
) -> Result<Vec<serde_json::Value>, String> {
    let Some(manager) = failover.get().await else {
        return Ok(vec![]);
    };
    manager
        .history(limit)
        .await
        .into_iter()
        .map(|event| serde_json::to_value(event).map_err(|e| e.to_string()))
        .collect()
}
