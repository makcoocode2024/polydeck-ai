//! Credential management via the operating system's secure storage.
//!
//! API keys, proxy tokens, WebDAV passwords, and chat backup keys are stored
//! in the OS credential store (Windows Credential Manager / macOS Keychain /
//! Linux Secret Service). They never appear in state.json, log files, or exports.

use crate::error::{AppError, AppResult};

const SERVICE: &str = "ai-deck";

/// In-memory credential store standing in for the OS keyring under `cargo test`.
///
/// Unconditional in test builds, not opt-in per test. Two reasons. It keeps the
/// suite off the developer's real credential store entirely — nothing can write
/// there by forgetting to opt in. And an opt-in store would be installed part-way
/// through a parallel run, so whether a given call saw the keyring or the map
/// would depend on thread scheduling.
///
/// `keyring`'s own mock cannot serve here: `MockCredentialBuilder` declares
/// `CredentialPersistence::EntryOnly` and builds a password-less credential per
/// `Entry::new`, while [`set`] and [`get`] each construct their own `Entry`. A
/// mocked write would land in an object dropped immediately after, and every read
/// would miss.
#[cfg(test)]
static TEST_STORE: std::sync::Mutex<Option<std::collections::HashMap<String, String>>> =
    std::sync::Mutex::new(None);

#[cfg(test)]
fn with_test_store<T>(
    f: impl FnOnce(&mut std::collections::HashMap<String, String>) -> T,
) -> Option<T> {
    let mut guard = TEST_STORE.lock().unwrap_or_else(|e| e.into_inner());
    Some(f(guard.get_or_insert_with(std::collections::HashMap::new)))
}

#[cfg(not(test))]
fn with_test_store<T>(
    _f: impl FnOnce(&mut std::collections::HashMap<String, String>) -> T,
) -> Option<T> {
    None
}

/// Store a credential in the OS keyring.
pub fn set(key: &str, value: &str) -> AppResult<()> {
    if with_test_store(|store| store.insert(key.to_string(), value.to_string())).is_some() {
        return Ok(());
    }
    let entry = keyring::Entry::new(SERVICE, key)
        .map_err(|e| AppError::Credential(format!("无法创建凭据条目 {key}：{e}")))?;
    entry
        .set_password(value)
        .map_err(|e| AppError::Credential(format!("无法保存凭据 {key}：{e}")))?;
    Ok(())
}

/// Retrieve a credential from the OS keyring.
pub fn get(key: &str) -> AppResult<String> {
    if let Some(hit) = with_test_store(|store| store.get(key).cloned()) {
        return hit.ok_or_else(|| AppError::Credential(format!("无法读取凭据 {key}：不存在")));
    }
    let entry = keyring::Entry::new(SERVICE, key)
        .map_err(|e| AppError::Credential(format!("无法创建凭据条目 {key}：{e}")))?;
    entry
        .get_password()
        .map_err(|e| AppError::Credential(format!("无法读取凭据 {key}：{e}")))
}

/// Delete a credential from the OS keyring.
pub fn delete(key: &str) -> AppResult<()> {
    if with_test_store(|store| store.remove(key)).is_some() {
        return Ok(());
    }
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

/// Prefix on every gateway client token.
///
/// Present so a token is recognizable on sight in `~/.codex/config.toml` or a log
/// line, and distinguishable from an upstream provider key that happens to sit in
/// the same field.
pub const CLIENT_TOKEN_PREFIX: &str = "adk_";

fn client_token_key(client_id: &str) -> String {
    format!("client:{client_id}:gateway_token")
}

/// The gateway token for `client_id`, minting one on first use.
///
/// Keyed by client rather than by (client, profile): rebinding a client to
/// another profile then needs no new token, so nothing has to be rewritten for
/// authentication's sake alone. Rotation stays an explicit act.
pub fn ensure_client_token(client_id: &str) -> AppResult<String> {
    let key = client_token_key(client_id);
    match get(&key) {
        Ok(token) if !token.trim().is_empty() => Ok(token),
        // A blank stored value is treated as absent. It cannot authenticate
        // anything, and leaving it would make the client unroutable with no way
        // to recover from the UI.
        _ => {
            let token = mint_client_token()?;
            set(&key, &token)?;
            Ok(token)
        }
    }
}

/// Replace `client_id`'s token with a fresh one and return it.
///
/// The caller must rewrite that client's config afterwards; until it does, the
/// client authenticates with a token the gateway no longer knows.
pub fn rotate_client_token(client_id: &str) -> AppResult<String> {
    let token = mint_client_token()?;
    set(&client_token_key(client_id), &token)?;
    Ok(token)
}

/// Read `client_id`'s token without minting one.
///
/// For callers that need to report whether a token exists rather than cause one
/// to exist — `ensure_client_token` would hide the difference.
pub fn get_client_token(client_id: &str) -> AppResult<Option<String>> {
    match get(&client_token_key(client_id)) {
        Ok(token) if !token.trim().is_empty() => Ok(Some(token)),
        _ => Ok(None),
    }
}

/// Forget `client_id`'s token. Absent is success.
pub fn delete_client_token(client_id: &str) -> AppResult<()> {
    delete(&client_token_key(client_id))
}

fn mint_client_token() -> AppResult<String> {
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw)
        .map_err(|e| AppError::Credential(format!("无法生成客户端令牌：{e}")))?;
    Ok(format!("{CLIENT_TOKEN_PREFIX}{}", hex::encode(raw)))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Reusing the token is the whole reason it is keyed by client: rebinding a
    /// client to another profile must not need a new one.
    #[test]
    fn ensure_client_token_mints_once_then_reuses() {
        let first = ensure_client_token("test-ensure-reuse").unwrap();
        let second = ensure_client_token("test-ensure-reuse").unwrap();

        assert_eq!(first, second, "第二次调用应复用已有令牌而不是重新签发");
        assert!(first.starts_with(CLIENT_TOKEN_PREFIX));
        // 32 random bytes as hex, plus the prefix.
        assert_eq!(first.len(), CLIENT_TOKEN_PREFIX.len() + 64);
    }

    #[test]
    fn tokens_differ_per_client() {
        let a = ensure_client_token("test-distinct-a").unwrap();
        let b = ensure_client_token("test-distinct-b").unwrap();
        assert_ne!(a, b, "两个客户端不能共用一个令牌，否则网关无法区分来源");
    }

    #[test]
    fn rotate_replaces_the_stored_token() {
        let before = ensure_client_token("test-rotate").unwrap();
        let rotated = rotate_client_token("test-rotate").unwrap();

        assert_ne!(before, rotated);
        assert_eq!(
            ensure_client_token("test-rotate").unwrap(),
            rotated,
            "轮换后再取应拿到新令牌"
        );
    }

    /// `get_client_token` reports; `ensure_client_token` causes. A UI asking
    /// "does this client have a token yet" must not create one by asking.
    #[test]
    fn get_does_not_mint() {
        assert_eq!(get_client_token("test-get-no-mint").unwrap(), None);
        assert_eq!(
            get_client_token("test-get-no-mint").unwrap(),
            None,
            "查询不应产生副作用"
        );

        let minted = ensure_client_token("test-get-no-mint").unwrap();
        assert_eq!(get_client_token("test-get-no-mint").unwrap(), Some(minted));
    }

    #[test]
    fn delete_is_idempotent_and_forces_a_new_token() {
        let before = ensure_client_token("test-delete").unwrap();
        delete_client_token("test-delete").unwrap();
        delete_client_token("test-delete").unwrap();

        assert_eq!(get_client_token("test-delete").unwrap(), None);
        assert_ne!(
            ensure_client_token("test-delete").unwrap(),
            before,
            "删除后应签发一个新令牌"
        );
    }

    /// A blank stored value cannot authenticate anything. Treating it as present
    /// would leave the client unroutable with no way to recover from the UI.
    #[test]
    fn blank_stored_token_is_replaced_not_returned() {
        set(&client_token_key("test-blank"), "   ").unwrap();

        assert_eq!(get_client_token("test-blank").unwrap(), None);
        let fresh = ensure_client_token("test-blank").unwrap();
        assert!(fresh.starts_with(CLIENT_TOKEN_PREFIX));
    }
}
