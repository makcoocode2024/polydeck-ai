//! Credential management via the operating system's secure storage.
//!
//! API keys, proxy tokens, WebDAV passwords, and chat backup keys are stored
//! in the OS credential store (Windows Credential Manager / macOS Keychain /
//! Linux Secret Service). They never appear in state.json, log files, or exports.

use crate::error::{AppError, AppResult};

const SERVICE: &str = "ai-deck";

/// Store a credential in the OS keyring.
pub fn set(key: &str, value: &str) -> AppResult<()> {
    let entry = keyring::Entry::new(SERVICE, key)
        .map_err(|e| AppError::Credential(format!("无法创建凭据条目 {key}：{e}")))?;
    entry
        .set_password(value)
        .map_err(|e| AppError::Credential(format!("无法保存凭据 {key}：{e}")))?;
    Ok(())
}

/// Retrieve a credential from the OS keyring.
pub fn get(key: &str) -> AppResult<String> {
    let entry = keyring::Entry::new(SERVICE, key)
        .map_err(|e| AppError::Credential(format!("无法创建凭据条目 {key}：{e}")))?;
    entry
        .get_password()
        .map_err(|e| AppError::Credential(format!("无法读取凭据 {key}：{e}")))
}

/// Delete a credential from the OS keyring.
pub fn delete(key: &str) -> AppResult<()> {
    let entry = keyring::Entry::new(SERVICE, key)
        .map_err(|e| AppError::Credential(format!("无法创建凭据条目 {key}：{e}")))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::Credential(format!("无法删除凭据 {key}：{e}"))),
    }
}

/// Check if a credential exists without reading its value.
pub fn exists(key: &str) -> bool {
    get(key).is_ok()
}

// --- Typed credential helpers ---

/// Store an API key for a provider profile.
pub fn set_api_key(profile_id: &str, api_key: &str) -> AppResult<()> {
    set(&format!("profile:{profile_id}:api_key"), api_key)
}

/// Get an API key for a provider profile.
pub fn get_api_key(profile_id: &str) -> AppResult<String> {
    get(&format!("profile:{profile_id}:api_key"))
}

/// Delete an API key for a provider profile.
pub fn delete_api_key(profile_id: &str) -> AppResult<()> {
    delete(&format!("profile:{profile_id}:api_key"))
}

/// Store a local proxy token for a provider.
pub fn set_proxy_token(provider_id: &str, token: &str) -> AppResult<()> {
    set(&format!("proxy:{provider_id}:token"), token)
}

/// Get a local proxy token for a provider.
pub fn get_proxy_token(provider_id: &str) -> AppResult<String> {
    get(&format!("proxy:{provider_id}:token"))
}

/// Store the chat backup encryption key (auto-generated on first use).
pub fn chat_backup_key() -> AppResult<Vec<u8>> {
    let key_name = "chat_backup_key";
    match get(key_name) {
        Ok(hex_key) => hex::decode(&hex_key)
            .map_err(|e| AppError::Credential(format!("备份密钥格式无效：{e}"))),
        Err(_) => {
            // Generate a new 256-bit key
            let mut key = [0u8; 32];
            getrandom::fill(&mut key)
                .map_err(|e| AppError::Credential(format!("无法生成随机密钥：{e}")))?;
            let hex_key = hex::encode(key);
            set(key_name, &hex_key)?;
            Ok(key.to_vec())
        }
    }
}

/// Store WebDAV password.
pub fn set_webdav_password(password: &str) -> AppResult<()> {
    set("webdav:password", password)
}

/// Get WebDAV password.
pub fn get_webdav_password() -> AppResult<String> {
    get("webdav:password")
}

/// Store an MCP server secret.
pub fn set_mcp_secret(server_id: &str, secret: &str) -> AppResult<()> {
    set(&format!("mcp:{server_id}"), secret)
}

/// Get an MCP server secret.
pub fn get_mcp_secret(server_id: &str) -> AppResult<String> {
    get(&format!("mcp:{server_id}"))
}

/// Redact a string for safe logging. Shows first 4 and last 2 characters.
pub fn redact(value: &str) -> String {
    if value.len() <= 8 {
        return "****".to_string();
    }
    let prefix = &value[..4];
    let suffix = &value[value.len() - 2..];
    format!("{prefix}...{suffix}")
}

/// Get a credential by key, returning None if not found instead of Err.
pub fn get_credential(key: &str) -> AppResult<Option<String>> {
    match get(key) {
        Ok(v) => Ok(Some(v)),
        Err(_) => Ok(None),
    }
}
