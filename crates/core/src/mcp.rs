//! MCP (Model Context Protocol) server management.
//! Built-in server catalog + custom servers, synced per-Profile.
//! Secrets stored as keyring references, never in plaintext.

use crate::error::AppResult;
use crate::profile::McpServerConfig;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub description: String,
    pub command: String,
    pub args: Vec<String>,
    pub env_keys: Vec<String>,
    pub is_builtin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub synced: Vec<String>,
    pub errors: Vec<String>,
}

pub fn builtin_servers() -> Vec<McpServer> {
    vec![
        McpServer {
            id: "filesystem".into(),
            name: "Filesystem".into(),
            description: "文件系统读写".into(),
            command: "npx".into(),
            args: vec![
                "-y".into(),
                "@modelcontextprotocol/server-filesystem".into(),
            ],
            env_keys: vec![],
            is_builtin: true,
        },
        McpServer {
            id: "github".into(),
            name: "GitHub".into(),
            description: "GitHub API 操作".into(),
            command: "npx".into(),
            args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
            env_keys: vec!["GITHUB_TOKEN".into()],
            is_builtin: true,
        },
    ]
}

pub fn sync_to_client(
    _profile_id: &str,
    _servers: &[McpServerConfig],
    _client: &str,
) -> AppResult<SyncResult> {
    Ok(SyncResult {
        synced: vec![],
        errors: vec![],
    })
}
