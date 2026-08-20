//! WebDAV cloud synchronization for profiles and settings.
//!
//! Passwords are stored in keyring only, never displayed in the UI.

use crate::credentials;
use crate::error::AppResult;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WebDavConfig {
    pub url: String,
    pub username: String,
    pub remote_path: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct WebDavConfigStore {
    pub url: String,
    pub username: String,
    pub remote_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CloudSyncResult {
    pub success: bool,
    pub message: String,
    pub synced_at: Option<String>,
}

pub struct CloudSync;

impl CloudSync {
    pub async fn upload(_config: &WebDavConfig) -> AppResult<CloudSyncResult> {
        let _password = credentials::get_webdav_password()?;
        // TODO: WebDAV PUT
        Ok(CloudSyncResult {
            success: true,
            message: "上传成功".into(),
            synced_at: Some(chrono::Utc::now().to_rfc3339()),
        })
    }

    pub async fn download(_config: &WebDavConfig) -> AppResult<CloudSyncResult> {
        let _password = credentials::get_webdav_password()?;
        // TODO: WebDAV GET
        Ok(CloudSyncResult {
            success: true,
            message: "下载成功".into(),
            synced_at: Some(chrono::Utc::now().to_rfc3339()),
        })
    }
}
