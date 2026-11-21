use crate::codex_refresh::RefreshStatus;
use crate::state::CodexRefreshState;
use tauri::State;

#[tauri::command]
pub async fn ad_get_codex_refresh_status(
    coordinator: State<'_, CodexRefreshState>,
) -> Result<RefreshStatus, String> {
    Ok(coordinator.status().await)
}

#[tauri::command]
pub async fn ad_retry_codex_refresh(
    coordinator: State<'_, CodexRefreshState>,
) -> Result<u64, String> {
    Ok(coordinator.request().await)
}
