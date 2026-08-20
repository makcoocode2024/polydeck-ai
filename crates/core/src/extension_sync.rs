//! Extension sync: MCP servers, Skills, Prompts to client config directories.

use crate::error::AppResult;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionPaths {
    pub codex_config: Option<String>,
    pub claude_config: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionSyncPlan {
    pub mcp_changes: Vec<String>,
    pub skill_changes: Vec<String>,
    pub prompt_changes: Vec<String>,
}

pub fn sync_extensions(_profile_id: &str) -> AppResult<ExtensionSyncPlan> {
    Ok(ExtensionSyncPlan {
        mcp_changes: vec![],
        skill_changes: vec![],
        prompt_changes: vec![],
    })
}
