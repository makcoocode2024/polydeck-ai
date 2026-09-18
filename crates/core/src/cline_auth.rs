//! Cline API authentication: OAuth token storage, auto-refresh, and device login.
//!
//! The Cline free-model API (`api.cline.bot`) uses WorkOS OAuth. An access token
//! expires after ~1 hour. This module stores both the access and refresh tokens
//! in the OS keyring, checks expiry before every request, and refreshes
//! transparently.

use crate::credentials;
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex as AsyncMutex;

/// Cline API base URL.
const CLINE_API_BASE: &str = "https://api.cline.bot";

/// Cline API host fragment for URL detection.
pub const CLINE_API_HOST: &str = "cline.bot";

/// Refresh 60 seconds before actual expiry to avoid race conditions.
const REFRESH_BUFFER_SECS: u64 = 60;

/// WorkOS endpoints (used by device auth flow).
const WORKOS_API_BASE: &str = "https://api.workos.com";
const WORKOS_CLIENT_ID: &str = "client_01K3A541FN8TA3EPPHTD2325AR";

/// Cline auth API paths.
const PATH_REGISTER: &str = "/api/v1/auth/register";
const PATH_REFRESH: &str = "/api/v1/auth/refresh";
const PATH_DEVICE_AUTH: &str = "/user_management/authorize/device";
const PATH_AUTHENTICATE: &str = "/user_management/authenticate";

/// Product identification headers required by the Cline API.
pub fn cline_product_headers() -> HashMap<String, String> {
    let mut h = HashMap::new();
    h.insert("HTTP-Referer".into(), "https://cline.bot".into());
    h.insert("X-Title".into(), "Cline".into());
    h.insert("X-IS-MULTIROOT".into(), "false".into());
    h.insert("X-CLIENT-TYPE".into(), "Cline".into());
    h.insert("User-Agent".into(), "Cline/0.0.83".into());
    h.insert("X-CLIENT-VERSION".into(), "0.0.83".into());
    h.insert("X-PLATFORM".into(), "cli".into());
    h.insert("X-PLATFORM-VERSION".into(), "0.0.83".into());
    h.insert("X-CORE-VERSION".into(), "0.0.83".into());
    h
}

/// True when a base URL points at the Cline API.
pub fn is_cline_api(base_url: &str) -> bool {
    base_url.contains(CLINE_API_HOST)
}

/// Ensure a key carries the `workos:` prefix.
pub fn ensure_workos_prefix(key: &str) -> String {
    if key.to_lowercase().starts_with("workos:") {
        key.to_string()
    } else {
        format!("workos:{key}")
    }
}

/// Stored Cline OAuth credentials.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClineCredentials {
    pub access: String,
    pub refresh: String,
    /// Expiration timestamp in milliseconds since epoch.
    pub expires: u64,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
}

impl ClineCredentials {
    /// Whether the access token has expired (or will within the buffer window).
    pub fn is_expired(&self) -> bool {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        now_ms + (REFRESH_BUFFER_SECS * 1000) >= self.expires
    }
}

// --- Keyring storage ---

fn cline_cred_key(profile_id: &str) -> String {
    format!("cline:{profile_id}:oauth")
}

/// Save Cline credentials for a profile.
pub fn save_credentials(profile_id: &str, creds: &ClineCredentials) -> AppResult<()> {
    let json = serde_json::to_string(creds)
        .map_err(|e| AppError::Credential(format!("序列化 Cline 凭据失败：{e}")))?;
    credentials::set(&cline_cred_key(profile_id), &json)
}

/// Load Cline credentials for a profile.
pub fn load_credentials(profile_id: &str) -> AppResult<ClineCredentials> {
    let json = credentials::get(&cline_cred_key(profile_id))?;
    serde_json::from_str(&json)
        .map_err(|e| AppError::Credential(format!("解析 Cline 凭据失败：{e}")))
}

/// Delete Cline credentials for a profile.
pub fn delete_credentials(profile_id: &str) -> AppResult<()> {
    credentials::delete(&cline_cred_key(profile_id))
}

/// Check if Cline credentials exist for a profile.
pub fn has_credentials(profile_id: &str) -> bool {
    credentials::exists(&cline_cred_key(profile_id))
}

// --- Token refresh ---

/// In-process lock to prevent concurrent refresh attempts for the same profile.
static REFRESH_LOCK: std::sync::LazyLock<AsyncMutex<()>> =
    std::sync::LazyLock::new(|| AsyncMutex::new(()));

fn build_client() -> AppResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .use_rustls_tls()
        .timeout(Duration::from_secs(30));
    if let Some(proxy_url) = crate::proxy_manager::get_configured_proxy() {
        if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
            builder = builder.proxy(proxy);
        }
    }
    builder
        .build()
        .map_err(|e| AppError::Network(format!("HTTP 客户端创建失败：{e}")))
}

/// Refresh the Cline access token using the refresh token.
///
/// Calls `POST /api/v1/auth/refresh` on api.cline.bot with the refresh token.
/// Returns updated credentials with a new access token, refresh token, and
/// expiry timestamp.
pub async fn refresh_token(creds: &ClineCredentials) -> AppResult<ClineCredentials> {
    let client = build_client()?;
    let url = format!("{CLINE_API_BASE}{PATH_REFRESH}");

    let body = serde_json::json!({
        "refreshToken": creds.refresh,
        "grantType": "refresh_token",
    });

    let mut req = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body);
    for (k, v) in cline_product_headers() {
        req = req.header(k.as_str(), v.as_str());
    }

    let resp = req
        .send()
        .await
        .map_err(|e| AppError::Network(format!("Cline token 刷新网络错误：{e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let is_invalid_grant =
            text.contains("invalid_grant") || text.contains("expired") || text.contains("revoked");
        if is_invalid_grant {
            return Err(AppError::Credential(
                "Cline refresh token 已失效，请重新登录".into(),
            ));
        }
        return Err(AppError::Credential(format!(
            "Cline token 刷新失败 HTTP {status}：{text}"
        )));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Network(format!("Cline token 刷新响应解析失败：{e}")))?;

    // Response format: {success: true, data: {accessToken, refreshToken?, expiresAt, userInfo}}
    let data = json.get("data").unwrap_or(&json);

    let access = data["accessToken"]
        .as_str()
        .or_else(|| data["access_token"].as_str())
        .or_else(|| json["accessToken"].as_str())
        .ok_or_else(|| AppError::Credential("刷新响应缺少 accessToken".into()))?
        .to_string();

    let refresh = data["refreshToken"]
        .as_str()
        .or_else(|| data["refresh_token"].as_str())
        .unwrap_or(&creds.refresh)
        .to_string();

    let expires = if let Some(exp_str) = data["expiresAt"].as_str() {
        // ISO date string: parse to unix ms
        chrono::DateTime::parse_from_rfc3339(exp_str)
            .map(|dt| dt.timestamp_millis() as u64)
            .unwrap_or_else(|_| {
                let now_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                now_ms + 3600 * 1000
            })
    } else if let Some(exp) = data["expiresAt"]
        .as_u64()
        .or_else(|| data["expires"].as_u64())
        .or_else(|| json["expires"].as_u64())
    {
        exp
    } else if let Some(exp_in) = data["expiresIn"]
        .as_u64()
        .or_else(|| data["expires_in"].as_u64())
    {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        now_ms + exp_in * 1000
    } else {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        now_ms + 3600 * 1000
    };

    let user_info = data.get("userInfo");
    let account_id = user_info
        .and_then(|u| u["clineUserId"].as_str())
        .map(|s| s.to_string())
        .or_else(|| creds.account_id.clone());
    let email = user_info
        .and_then(|u| u["email"].as_str())
        .map(|s| s.to_string())
        .or_else(|| creds.email.clone());

    Ok(ClineCredentials {
        access,
        refresh,
        expires,
        account_id,
        email,
    })
}

/// Get a valid access token for a Cline profile, refreshing if expired.
///
/// This is the main entry point for gateway and protocol code. It:
/// 1. Loads credentials from keyring
/// 2. Checks expiry
/// 3. Refreshes if needed (under a lock to prevent concurrent refreshes)
/// 4. Saves updated credentials back to keyring
/// 5. Also updates the plain api_key so unaware code paths still work
/// 6. Returns the `workos:`-prefixed access token ready for use
pub async fn get_valid_access_token(profile_id: &str) -> AppResult<String> {
    let creds = match load_credentials(profile_id) {
        Ok(c) => c,
        Err(_) => {
            // No Cline credentials stored — fall back to plain API key
            return credentials::get_api_key(profile_id);
        }
    };

    if !creds.is_expired() {
        return Ok(ensure_workos_prefix(&creds.access));
    }

    // Acquire async lock to prevent concurrent refresh
    let _lock = REFRESH_LOCK.lock().await;

    // Re-check after acquiring lock (another thread may have refreshed)
    let creds = match load_credentials(profile_id) {
        Ok(c) => c,
        Err(_) => return credentials::get_api_key(profile_id),
    };
    if !creds.is_expired() {
        return Ok(ensure_workos_prefix(&creds.access));
    }

    tracing::info!("Cline access token 已过期，正在刷新...");
    match refresh_token(&creds).await {
        Ok(refreshed) => {
            save_credentials(profile_id, &refreshed)?;
            // Keep the plain api_key in sync for code paths unaware of refresh
            let _ = credentials::set_api_key(profile_id, &refreshed.access);
            tracing::info!("Cline access token 刷新成功");
            Ok(ensure_workos_prefix(&refreshed.access))
        }
        Err(e) => {
            // If the token hasn't fully expired past the grace period, use old token
            let grace_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            if grace_ms <= creds.expires + 30_000 {
                tracing::warn!("Cline token 刷新失败，使用旧 token（尚在宽限期内）：{e}");
                Ok(ensure_workos_prefix(&creds.access))
            } else {
                Err(e)
            }
        }
    }
}

// --- Device authorization flow ---

/// Device authorization response from WorkOS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
}

/// Start the Cline device authorization flow.
///
/// Returns the device code and verification URL for the user to visit.
pub async fn start_device_auth() -> AppResult<DeviceAuthResponse> {
    let client = build_client()?;
    let url = format!("{WORKOS_API_BASE}{PATH_DEVICE_AUTH}");

    // 对齐 SDK：form 编码 client_id（WorkOS /user_management/authorize/device）
    let params = [("client_id", WORKOS_CLIENT_ID)];

    let resp = client
        .post(&url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&params)
        .send()
        .await
        .map_err(|e| AppError::Network(format!("设备授权请求失败：{e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Protocol(format!(
            "设备授权失败 HTTP {status}：{text}"
        )));
    }

    resp.json::<DeviceAuthResponse>()
        .await
        .map_err(|e| AppError::Protocol(format!("设备授权响应解析失败：{e}")))
}

/// WorkOS token response from polling.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct WorkosTokenResponse {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    token_type: Option<String>,
}

/// Poll WorkOS for device auth completion, then register with Cline API.
///
/// Returns the complete Cline credentials ready to be saved.
pub async fn complete_device_auth(
    device_code: &str,
    expires_in: u64,
    poll_interval: u64,
) -> AppResult<ClineCredentials> {
    let client = build_client()?;
    let auth_url = format!("{WORKOS_API_BASE}{PATH_AUTHENTICATE}");
    let deadline = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + expires_in;

    let mut interval = poll_interval;

    loop {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now >= deadline {
            return Err(AppError::Protocol("设备授权超时".into()));
        }

        tokio::time::sleep(Duration::from_secs(interval)).await;

        let params = [
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code),
            ("client_id", WORKOS_CLIENT_ID),
        ];

        let resp = client
            .post(&auth_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&params)
            .send()
            .await
            .map_err(|e| AppError::Network(format!("设备授权轮询失败：{e}")))?;

        if resp.status().is_success() {
            let token: WorkosTokenResponse = resp
                .json()
                .await
                .map_err(|e| AppError::Protocol(format!("WorkOS 令牌解析失败：{e}")))?;

            // Register the token with Cline API to get Cline credentials
            return register_token(&client, &token.access_token, &token.refresh_token).await;
        }

        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let error = body["error"].as_str().unwrap_or("");

        match error {
            "authorization_pending" => continue,
            "slow_down" => {
                interval += 1;
                continue;
            }
            "access_denied" | "expired_token" | "invalid_grant" => {
                let desc = body["error_description"].as_str().unwrap_or("授权失败");
                return Err(AppError::Protocol(format!("设备授权被拒绝：{desc}")));
            }
            _ => {
                let desc = body["error_description"].as_str().unwrap_or("未知错误");
                return Err(AppError::Protocol(format!("设备授权轮询错误：{desc}")));
            }
        }
    }
}

/// Register WorkOS tokens with Cline API to get Cline credentials.
async fn register_token(
    client: &reqwest::Client,
    access_token: &str,
    refresh_token: &str,
) -> AppResult<ClineCredentials> {
    let url = format!("{CLINE_API_BASE}{PATH_REGISTER}");

    let body = serde_json::json!({
        "accessToken": access_token,
        "refreshToken": refresh_token,
    });

    let mut req = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body);
    for (k, v) in cline_product_headers() {
        req = req.header(k.as_str(), v.as_str());
    }

    let resp = req
        .send()
        .await
        .map_err(|e| AppError::Network(format!("Cline 令牌注册失败：{e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Protocol(format!(
            "Cline 令牌注册失败 HTTP {status}：{text}"
        )));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Protocol(format!("注册响应解析失败：{e}")))?;

    // 与 SDK `LJ`/`UJ` 对齐：响应格式为 { success, data: { accessToken, refreshToken,
    // expiresAt(ISO 字符串), tokenType, userInfo } }，字段全部嵌套在 `data` 下。
    let data = json.get("data").unwrap_or(&json);

    let access = data["accessToken"]
        .as_str()
        .or_else(|| data["access_token"].as_str())
        .or_else(|| json["accessToken"].as_str())
        .ok_or_else(|| AppError::Protocol("注册响应缺少 accessToken".into()))?
        .to_string();

    let refresh = data["refreshToken"]
        .as_str()
        .or_else(|| data["refresh_token"].as_str())
        .or_else(|| json["refreshToken"].as_str())
        .or_else(|| json["refresh_token"].as_str())
        .unwrap_or(refresh_token)
        .to_string();

    let expires = if let Some(exp_str) = data["expiresAt"].as_str() {
        // expiresAt 是 ISO 日期字符串（SDK 用 Date.parse），而非毫秒数字。
        chrono::DateTime::parse_from_rfc3339(exp_str)
            .map(|dt| dt.timestamp_millis() as u64)
            .unwrap_or_else(|_| {
                let now_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                now_ms + 3600 * 1000
            })
    } else if let Some(exp) = data["expiresAt"]
        .as_u64()
        .or_else(|| data["expires"].as_u64())
        .or_else(|| json["expires"].as_u64())
    {
        exp
    } else if let Some(exp_in) = data["expiresIn"]
        .as_u64()
        .or_else(|| data["expires_in"].as_u64())
    {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        now_ms + exp_in * 1000
    } else {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        now_ms + 3600 * 1000
    };

    let user_info = data.get("userInfo");
    let account_id = user_info
        .and_then(|u| u["clineUserId"].as_str())
        .map(|s| s.to_string());
    let email = user_info
        .and_then(|u| u["email"].as_str())
        .map(|s| s.to_string());

    Ok(ClineCredentials {
        access,
        refresh,
        expires,
        account_id,
        email,
    })
}
