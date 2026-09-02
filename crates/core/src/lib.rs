//! AI Deck Core — business logic, no UI dependencies.
//!
//! This crate contains protocol detection, credential management, profile system,
//! reasoning engine, chat history, extension management, and all platform logic.

pub mod api_key_detector;
pub mod autolaunch;
pub mod binding;
pub mod chat_history;
pub mod claude_desktop;
pub mod client_detector;
pub mod client_rules;
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
pub mod responses_stream;
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

/// Where per-user application data lives: roaming (`%APPDATA%`) and local
/// (`%LOCALAPPDATA%`) on Windows, one directory for both on macOS.
///
/// Reading `APPDATA` directly is not testable. `AI_DECK_HOME_OVERRIDE` only
/// redirects [`user_home_dir`], so a writer that consults the environment
/// variable still lands in the developer's real application data during
/// `cargo test` — which is how a test here spent a day rewriting a real
/// `claude_desktop_config.json`. Hence the second step below: once a home
/// override is set, the real environment is never consulted again.
fn app_data_dir(local: bool) -> Option<PathBuf> {
    let dedicated = if local {
        "AI_DECK_LOCALAPPDATA_OVERRIDE"
    } else {
        "AI_DECK_APPDATA_OVERRIDE"
    };
    if let Some(dir) = std::env::var_os(dedicated) {
        return Some(PathBuf::from(dir));
    }

    // A home override is a blanket redirect, so derive from it and stop. Falling
    // through to APPDATA here would send tests at real user data.
    if let Some(override_home) = std::env::var_os("AI_DECK_HOME_OVERRIDE") {
        return Some(app_data_under(&PathBuf::from(override_home), local));
    }

    #[cfg(windows)]
    {
        let var = if local { "LOCALAPPDATA" } else { "APPDATA" };
        if let Some(dir) = std::env::var_os(var) {
            return Some(PathBuf::from(dir));
        }
    }

    user_home_dir().map(|home| app_data_under(&home, local))
}

/// The application data directory a given home implies, per platform.
fn app_data_under(home: &std::path::Path, local: bool) -> PathBuf {
    #[cfg(windows)]
    {
        let leaf = if local { "Local" } else { "Roaming" };
        home.join("AppData").join(leaf)
    }
    #[cfg(target_os = "macos")]
    {
        let _ = local;
        home.join("Library").join("Application Support")
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        if local {
            home.join(".local").join("share")
        } else {
            home.join(".config")
        }
    }
}

/// Roaming application data (`%APPDATA%` on Windows).
pub fn roaming_app_data_dir() -> Option<PathBuf> {
    app_data_dir(false)
}

/// Machine-local application data (`%LOCALAPPDATA%` on Windows).
pub fn local_app_data_dir() -> Option<PathBuf> {
    app_data_dir(true)
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

/// Serializes every test that repoints `AI_DECK_HOME_OVERRIDE`.
///
/// One guard for the whole crate, deliberately: the variable is process-global,
/// so a per-module mutex guards nothing against a test in another module. Two
/// such mutexes existed before this and let `client_rules` tests clear the
/// variable while a `profile_switch` test was mid-write, which sent that test at
/// the real `~/.claude/settings.json`. It failed only under CI timing.
#[cfg(test)]
pub(crate) static HOME_ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Acquire [`HOME_ENV_GUARD`], ignoring poisoning from an unrelated failed test.
#[cfg(test)]
pub(crate) fn lock_home_env() -> std::sync::MutexGuard<'static, ()> {
    HOME_ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner())
}
