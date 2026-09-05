//! WebDAV cloud synchronization for profiles and settings.
//!
//! Passwords are stored in keyring only, never displayed in the UI.
//!
//! `upload`/`download` used to return `success: true` after doing nothing but
//! reading the password, so the UI reported a completed sync while no request
//! had left the machine. Both now perform the transfer and report what the
//! server actually answered.

use crate::credentials;
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What gets synced: the profile/settings document.
const STATE_FILE: &str = "state.json";

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
    /// Bytes transferred, so the UI can distinguish a real sync from a no-op.
    pub bytes: Option<u64>,
}

/// Join the base URL and remote path into the file's absolute URL.
///
/// Kept separate so the joining rules are testable without a server: a missing
/// or doubled slash silently produces a 404 that looks like a credential fault.
fn remote_file_url(config: &WebDavConfig) -> AppResult<String> {
    let base = config.url.trim_end_matches('/');
    if base.is_empty() {
        return Err(AppError::InvalidInput("WebDAV 地址不能为空".into()));
    }
    if !base.starts_with("http://") && !base.starts_with("https://") {
        return Err(AppError::InvalidInput(format!(
            "WebDAV 地址必须以 http:// 或 https:// 开头，收到：{base}"
        )));
    }
    let path = config.remote_path.trim_matches('/');
    if path.is_empty() {
        return Ok(format!("{base}/{STATE_FILE}"));
    }
    Ok(format!("{base}/{path}/{STATE_FILE}"))
}

pub struct CloudSync;

impl CloudSync {
    /// PUT the local state document to the server.
    pub async fn upload(config: &WebDavConfig) -> AppResult<CloudSyncResult> {
        let password = credentials::get_webdav_password()?;
        let url = remote_file_url(config)?;

        let state_path = crate::profile::data_directory()?.join(STATE_FILE);
        let body = std::fs::read(&state_path).map_err(|e| {
            AppError::Storage(format!("无法读取本地配置 {}：{e}", state_path.display()))
        })?;
        let bytes = body.len() as u64;

        let response = reqwest::Client::new()
            .put(&url)
            .basic_auth(&config.username, Some(&password))
            .body(body)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            return Err(AppError::Network(format!("上传失败，服务器返回 {status}")));
        }

        Ok(CloudSyncResult {
            success: true,
            message: format!("已上传 {bytes} 字节到 {url}"),
            synced_at: Some(chrono::Utc::now().to_rfc3339()),
            bytes: Some(bytes),
        })
    }

    /// GET the remote state document and replace the local one.
    ///
    /// The existing file is copied to `state.json.bak` first: a download that
    /// overwrites the only copy of the user's profiles is not recoverable.
    pub async fn download(config: &WebDavConfig) -> AppResult<CloudSyncResult> {
        let password = credentials::get_webdav_password()?;
        let url = remote_file_url(config)?;

        let response = reqwest::Client::new()
            .get(&url)
            .basic_auth(&config.username, Some(&password))
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            return Err(AppError::Network(format!("下载失败，服务器返回 {status}")));
        }

        let body = response.bytes().await?;
        // Reject a body that is not the document we sync, before it replaces a
        // working state file. An HTML login page answers 200 just as happily.
        serde_json::from_slice::<serde_json::Value>(&body).map_err(|e| {
            AppError::Protocol(format!("远端内容不是有效的 JSON 配置，已放弃覆盖本地：{e}"))
        })?;

        let state_path = crate::profile::data_directory()?.join(STATE_FILE);
        if let Some(parent) = state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if state_path.exists() {
            let backup = state_path.with_extension("json.bak");
            std::fs::copy(&state_path, &backup)
                .map_err(|e| AppError::Storage(format!("备份本地配置失败，已放弃覆盖：{e}")))?;
        }
        std::fs::write(&state_path, &body)?;

        Ok(CloudSyncResult {
            success: true,
            message: format!(
                "已从 {url} 下载 {} 字节，原文件备份为 state.json.bak",
                body.len()
            ),
            synced_at: Some(chrono::Utc::now().to_rfc3339()),
            bytes: Some(body.len() as u64),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(url: &str, remote_path: &str) -> WebDavConfig {
        WebDavConfig {
            url: url.into(),
            username: "u".into(),
            remote_path: remote_path.into(),
            enabled: true,
        }
    }

    /// A doubled or missing slash produces a 404 that is easy to misread as an
    /// auth failure, so the joining is pinned rather than left to chance.
    #[test]
    fn builds_remote_url_regardless_of_slashes() {
        for (base, path) in [
            ("https://dav.example.com", "polydeck"),
            ("https://dav.example.com/", "/polydeck/"),
            ("https://dav.example.com//", "polydeck//"),
        ] {
            assert_eq!(
                remote_file_url(&config(base, path)).unwrap(),
                "https://dav.example.com/polydeck/state.json",
                "base={base} path={path} 必须归一化到同一个 URL"
            );
        }

        assert_eq!(
            remote_file_url(&config("https://dav.example.com", "")).unwrap(),
            "https://dav.example.com/state.json",
            "远端路径为空时直接放在根下"
        );
    }

    /// An empty or scheme-less address must fail before a request is attempted,
    /// rather than surfacing as an opaque reqwest error.
    #[test]
    fn rejects_unusable_addresses() {
        assert!(remote_file_url(&config("", "p")).is_err(), "空地址必须报错");
        assert!(
            remote_file_url(&config("dav.example.com", "p")).is_err(),
            "缺少 scheme 必须报错"
        );
    }
}
