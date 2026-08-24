//! AI Deck Core — business logic, no UI dependencies.
//!
//! This crate contains protocol detection, credential management, profile system,
//! reasoning engine, chat history, extension management, and all platform logic.

pub mod api_key_detector;
pub mod autolaunch;
pub mod chat_history;
pub mod client_detector;
pub mod cloud_sync;
pub mod credentials;
pub mod deep_link;
pub mod diagnostics;
pub mod encrypted_backup;
pub mod error;
pub mod extension_sync;
pub mod history_watcher;
pub mod importer;
pub mod logging;
pub mod mcp;
pub mod messages_stream;
pub mod profile;
pub mod profile_switch;
pub mod profile_templates;
pub mod prompts;
pub mod protocol;
pub mod proxy_manager;
pub mod reasoning_discovery;
pub mod reasoning_verification;
pub mod responses_chat;
pub mod session_parser;
pub mod skills;
pub mod storage;
pub mod tray_state;
pub mod types;
pub mod updater;

pub use error::{AppError, AppResult};
use std::path::PathBuf;

/// Resolve the active user home directory reliably on all platforms (including Windows envs).
pub fn user_home_dir() -> Option<PathBuf> {
    if let Some(override_home) = std::env::var_os("AI_DECK_HOME_OVERRIDE") {
        let p = PathBuf::from(override_home);
        return Some(p);
    }
    #[cfg(windows)]
    {
        if let Some(up) = std::env::var_os("USERPROFILE") {
            let p = PathBuf::from(up);
            if p.exists() {
                return Some(p);
            }
        }
        if let Some(hd) = std::env::var_os("HOMEDRIVE") {
            if let Some(hp) = std::env::var_os("HOMEPATH") {
                let p = PathBuf::from(format!("{}{}", hd.to_string_lossy(), hp.to_string_lossy()));
                if p.exists() {
                    return Some(p);
                }
            }
        }
        let admin = PathBuf::from(r"C:\Users\admin");
        if admin.exists() {
            return Some(admin);
        }
    }
    if let Some(h) = dirs::home_dir() {
        if h.exists() {
            return Some(h);
        }
    }
    dirs::home_dir()
}

/// Returns all possible candidate home directories for multi-user/historical scanning.
pub fn candidate_home_dirs() -> Vec<PathBuf> {
    let mut list = Vec::new();
    if let Some(h) = user_home_dir() {
        if !list.contains(&h) {
            list.push(h);
        }
    }
    if let Some(dh) = dirs::home_dir() {
        if dh.exists() && !list.contains(&dh) {
            list.push(dh);
        }
    }
    #[cfg(windows)]
    {
        let admin = PathBuf::from(r"C:\Users\admin");
        if admin.exists() && !list.contains(&admin) {
            list.push(admin);
        }
    }
    list
}

/// Shared application state container used by the Tauri shell.
pub struct AppState {
    pub version: String,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
