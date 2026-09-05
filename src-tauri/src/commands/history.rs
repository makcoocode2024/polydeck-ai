use polydeck_core::chat_history::{ConsolidateReport, HistoryStore, SessionSummary};
use tauri::command;

#[command]
pub async fn ad_query_history() -> Result<Vec<SessionSummary>, String> {
    let store = HistoryStore::open().map_err(|e| e.to_string())?;
    store.list_summaries().map_err(|e| e.to_string())
}

#[command]
pub async fn ad_export_history(format: String) -> Result<String, String> {
    let store = HistoryStore::open().map_err(|e| e.to_string())?;
    store.export_all(&format).map_err(|e| e.to_string())
}

#[command]
pub async fn ad_create_encrypted_backup(password: String) -> Result<String, String> {
    HistoryStore::create_encrypted_backup_file(&password).map_err(|e| e.to_string())
}

#[command]
pub async fn ad_restore_encrypted_backup(path: String, password: String) -> Result<(), String> {
    HistoryStore::restore_encrypted_backup_file(&path, &password).map_err(|e| e.to_string())
}

/// Fold duplicate session rows together and normalize what they are compared on.
///
/// Exposed as an explicit action as well as running after every sync, because a
/// database carrying years of rows from earlier schema versions needs one deliberate
/// pass the user can trigger and see the result of.
#[command]
pub async fn ad_consolidate_history() -> Result<ConsolidateReport, String> {
    let store = HistoryStore::open().map_err(|e| e.to_string())?;
    store.consolidate().map_err(|e| e.to_string())
}

/// Re-index session files, then consolidate.
///
/// `HistoryStore::open` already syncs, so this exists to let the user force a refresh
/// after a client has written new sessions without restarting the app.
#[command]
pub async fn ad_sync_history() -> Result<usize, String> {
    let store = HistoryStore::open().map_err(|e| e.to_string())?;
    store.sync_all().map_err(|e| e.to_string())
}
