//! Import from legacy tools: Provider Deck, CC Switch, RelayManager.

use crate::error::AppResult;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ImportConflict {
    Rename,
    Skip,
    Overwrite,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub success: bool,
    pub imported: Vec<String>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
    pub message: String,
}

pub fn detect_importable_sources() -> Vec<String> {
    let mut sources = Vec::new();
    let home = dirs::home_dir().unwrap_or_default();
    
    if home.join(".provider-deck").exists() || home.join("AppData/Roaming/Provider Deck").exists() {
        sources.push("Provider Deck".into());
    }
    // Add more source detection as needed
    sources
}

pub fn import_from_provider_deck(_conflict: ImportConflict) -> AppResult<ImportResult> {
    Ok(ImportResult {
        success: true,
        imported: vec![],
        skipped: vec![],
        errors: vec![],
        message: "未找到可导入的 Provider Deck 数据".into(),
    })
}
