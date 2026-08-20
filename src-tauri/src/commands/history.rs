use polydeck_core::chat_history::{HistoryStore, SessionSummary};
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
