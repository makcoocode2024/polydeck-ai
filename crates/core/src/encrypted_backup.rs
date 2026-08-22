//! Encrypted backup/restore for chat history using XChaCha20-Poly1305.
//!
//! Encryption keys are stored in the OS keyring. Backup files are JSON
//! with hex-encoded nonce and ciphertext. Tampering is detected and rejected.

use crate::credentials;
use crate::error::{AppError, AppResult};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

const BACKUP_FORMAT: &str = "ai-deck.history-backup";
const BACKUP_VERSION: u32 = 1;
const NONCE_SIZE: usize = 24;
const KEY_SIZE: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EncryptedFile {
    format: String,
    version: u32,
    algorithm: String,
    nonce: String,
    ciphertext: String,
    exported_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct BackupRecord {
    pub id: String,
    pub file_name: String,
    pub path: String,
    pub created_at: String,
    pub size: u64,
    pub session_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub success: bool,
    pub message: String,
    pub imported_count: usize,
    pub snapshot_id: Option<String>,
}

pub fn encrypt_data(plaintext: &[u8]) -> AppResult<Vec<u8>> {
    let key = credentials::chat_backup_key()?;
    if key.len() != KEY_SIZE {
        return Err(AppError::Encryption("备份密钥长度无效".into()));
    }
    let cipher = XChaCha20Poly1305::new_from_slice(&key)
        .map_err(|_| AppError::Encryption("无法初始化加密器".into()))?;

    let mut nonce_bytes = [0u8; NONCE_SIZE];
    getrandom::fill(&mut nonce_bytes)
        .map_err(|e| AppError::Encryption(format!("随机数生成失败：{e}")))?;

    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce_bytes), plaintext)
        .map_err(|_| AppError::Encryption("加密失败".into()))?;

    let file = EncryptedFile {
        format: BACKUP_FORMAT.into(),
        version: BACKUP_VERSION,
        algorithm: "XChaCha20-Poly1305".into(),
        nonce: hex::encode(nonce_bytes),
        ciphertext: hex::encode(ciphertext),
        exported_at: chrono::Utc::now().to_rfc3339(),
    };

    serde_json::to_vec_pretty(&file).map_err(|e| AppError::Encryption(e.to_string()))
}

pub fn decrypt_data(encrypted_json: &[u8]) -> AppResult<Vec<u8>> {
    let file: EncryptedFile = serde_json::from_slice(encrypted_json)
        .map_err(|e| AppError::Encryption(format!("备份文件格式无效：{e}")))?;

    if file.format != BACKUP_FORMAT {
        return Err(AppError::Encryption("不是 AI Deck 备份文件".into()));
    }
    if file.version > BACKUP_VERSION {
        return Err(AppError::Encryption(format!(
            "备份版本 {} 高于当前支持版本 {BACKUP_VERSION}",
            file.version
        )));
    }

    let key = credentials::chat_backup_key()?;
    let nonce = hex::decode(&file.nonce).map_err(|_| AppError::Encryption("随机数无效".into()))?;
    let ciphertext =
        hex::decode(&file.ciphertext).map_err(|_| AppError::Encryption("密文无效".into()))?;

    let cipher = XChaCha20Poly1305::new_from_slice(&key)
        .map_err(|_| AppError::Encryption("无法初始化解密器".into()))?;

    cipher
        .decrypt(XNonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| {
            AppError::Encryption(
                "解密失败：文件已损坏、被修改，或不是由当前系统用户创建的备份".into(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        // This test requires keyring access, skip in CI or headless environments
        if std::env::var("CI").is_ok() {
            return;
        }
        let data = b"hello world test data";
        let encrypted = match encrypt_data(data) {
            Ok(enc) => enc,
            Err(AppError::Credential(_)) => return,
            Err(e) => panic!("unexpected error: {e}"),
        };
        let decrypted = decrypt_data(&encrypted).unwrap();
        assert_eq!(decrypted, data);
    }
}
