//! AI client installation detection.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DetectedClient {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub version: Option<String>,
    pub config_path: Option<String>,
    pub supports_auto_config: bool,
}

pub fn detect_all() -> Vec<DetectedClient> {
    let home = crate::user_home_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default());

    #[cfg(windows)]
    let app_data = std::env::var_os("APPDATA").map(std::path::PathBuf::from);
    #[cfg(windows)]
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);

    // 1. Claude Desktop config path
    #[cfg(windows)]
    let claude_desktop_config = app_data
        .as_ref()
        .map(|p| p.join(r"Claude\claude_desktop_config.json"));
    #[cfg(target_os = "macos")]
    let claude_desktop_config =
        Some(home.join("Library/Application Support/Claude/claude_desktop_config.json"));
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let claude_desktop_config = Some(home.join(".config/Claude/claude_desktop_config.json"));

    // Claude Desktop installed check
    let claude_desktop_installed = {
        #[cfg(windows)]
        {
            let in_local = local_app_data
                .as_ref()
                .map(|p| {
                    p.join(r"AnthropicClaude\Claude.exe").exists()
                        || p.join(r"Programs\Claude\Claude.exe").exists()
                        || p.join(r"Claude\Claude.exe").exists()
                })
                .unwrap_or(false);
            let in_roaming = app_data
                .as_ref()
                .map(|p| p.join("Claude").exists())
                .unwrap_or(false);
            in_local || in_roaming || which_exists("claude-desktop") || which_exists("Claude")
        }
        #[cfg(target_os = "macos")]
        {
            std::path::Path::new("/Applications/Claude.app").exists()
                || which_exists("claude-desktop")
        }
        #[cfg(all(not(windows), not(target_os = "macos")))]
        {
            which_exists("claude-desktop") || home.join(".config/Claude").exists()
        }
    };

    // 2. Hermes installed check & config path
    let hermes_config = home.join(".hermes").join("config.yaml");
    let hermes_config_json = home.join(".hermes").join("config.json");
    let hermes_installed = which_exists("hermes") || home.join(".hermes").exists() || {
        #[cfg(windows)]
        {
            local_app_data
                .as_ref()
                .map(|p| p.join("hermes").exists())
                .unwrap_or(false)
        }
        #[cfg(not(windows))]
        {
            false
        }
    };

    vec![
        DetectedClient {
            id: "codex-cli".into(),
            name: "Codex CLI".into(),
            installed: which_exists("codex") || home.join(".codex").join("config.toml").exists(),
            version: None,
            config_path: if home.join(".codex").join("config.toml").exists() {
                Some(
                    home.join(".codex")
                        .join("config.toml")
                        .to_string_lossy()
                        .into(),
                )
            } else {
                None
            },
            supports_auto_config: true,
        },
        DetectedClient {
            id: "claude-code".into(),
            name: "Claude Code".into(),
            installed: which_exists("claude")
                || home.join(".claude").join("settings.json").exists(),
            version: None,
            config_path: if home.join(".claude").join("settings.json").exists() {
                Some(
                    home.join(".claude")
                        .join("settings.json")
                        .to_string_lossy()
                        .into(),
                )
            } else {
                None
            },
            supports_auto_config: true,
        },
        DetectedClient {
            id: "claude-desktop".into(),
            name: "Claude Desktop".into(),
            installed: claude_desktop_installed,
            version: None,
            config_path: claude_desktop_config.map(|p| p.to_string_lossy().into()),
            supports_auto_config: true,
        },
        DetectedClient {
            id: "hermes".into(),
            name: "Hermes".into(),
            installed: hermes_installed,
            version: None,
            config_path: if hermes_config.exists() {
                Some(hermes_config.to_string_lossy().into())
            } else if hermes_config_json.exists() {
                Some(hermes_config_json.to_string_lossy().into())
            } else if home.join(".hermes").exists() {
                Some(
                    home.join(".hermes")
                        .join("config.yaml")
                        .to_string_lossy()
                        .into(),
                )
            } else {
                None
            },
            supports_auto_config: true,
        },
        DetectedClient {
            id: "cursor".into(),
            name: "Cursor".into(),
            installed: which_exists("cursor") || {
                #[cfg(windows)]
                {
                    local_app_data
                        .as_ref()
                        .map(|p| {
                            p.join(r"Programs\cursor\Cursor.exe").exists()
                                || p.join(r"Programs\Cursor\Cursor.exe").exists()
                        })
                        .unwrap_or(false)
                }
                #[cfg(not(windows))]
                {
                    false
                }
            },
            version: None,
            config_path: None,
            supports_auto_config: false,
        },
        DetectedClient {
            id: "windsurf".into(),
            name: "Windsurf".into(),
            installed: which_exists("windsurf") || {
                #[cfg(windows)]
                {
                    local_app_data
                        .as_ref()
                        .map(|p| p.join(r"Programs\Windsurf\Windsurf.exe").exists())
                        .unwrap_or(false)
                }
                #[cfg(not(windows))]
                {
                    false
                }
            },
            version: None,
            config_path: None,
            supports_auto_config: false,
        },
        DetectedClient {
            id: "vscode".into(),
            name: "VS Code (Cline / Continue)".into(),
            installed: which_exists("code") || {
                #[cfg(windows)]
                {
                    local_app_data
                        .as_ref()
                        .map(|p| {
                            p.join(r"Programs\Microsoft VS Code\bin\code.cmd").exists()
                                || p.join(r"Programs\Microsoft VS Code\Code.exe").exists()
                        })
                        .unwrap_or(false)
                }
                #[cfg(not(windows))]
                {
                    false
                }
            },
            version: None,
            config_path: if home.join(".continue").join("config.json").exists() {
                Some(
                    home.join(".continue")
                        .join("config.json")
                        .to_string_lossy()
                        .into(),
                )
            } else {
                None
            },
            supports_auto_config: false,
        },
        DetectedClient {
            id: "cherry-studio".into(),
            name: "Cherry Studio".into(),
            installed: which_exists("cherry-studio") || {
                #[cfg(windows)]
                {
                    local_app_data
                        .as_ref()
                        .map(|p| p.join(r"Programs\cherry-studio\Cherry Studio.exe").exists())
                        .unwrap_or(false)
                        || app_data
                            .as_ref()
                            .map(|p| p.join("cherry-studio").exists())
                            .unwrap_or(false)
                }
                #[cfg(not(windows))]
                {
                    false
                }
            },
            version: None,
            config_path: None,
            supports_auto_config: false,
        },
        DetectedClient {
            id: "chatbox".into(),
            name: "Chatbox".into(),
            installed: which_exists("chatbox") || {
                #[cfg(windows)]
                {
                    local_app_data
                        .as_ref()
                        .map(|p| p.join(r"Programs\chatbox\Chatbox.exe").exists())
                        .unwrap_or(false)
                        || app_data
                            .as_ref()
                            .map(|p| p.join("xyz.chatbox.app").exists())
                            .unwrap_or(false)
                }
                #[cfg(not(windows))]
                {
                    false
                }
            },
            version: None,
            config_path: None,
            supports_auto_config: false,
        },
        DetectedClient {
            id: "opencode".into(),
            name: "OpenCode".into(),
            installed: which_exists("opencode") || {
                #[cfg(windows)]
                {
                    local_app_data
                        .as_ref()
                        .map(|p| p.join(r"Programs\OpenCode\OpenCode.exe").exists())
                        .unwrap_or(false)
                }
                #[cfg(not(windows))]
                {
                    false
                }
            },
            version: None,
            config_path: None,
            // No writer in `profile_switch::write_client_config`, and no local
            // config file to write one for.
            supports_auto_config: false,
        },
    ]
}

fn which_exists(name: &str) -> bool {
    // 1. Pure Rust PATH scan - 0ms, spawns no subprocess, zero window popups
    if let Some(path_var) = std::env::var_os("PATH") {
        let pathext = if cfg!(windows) {
            vec![".exe", ".cmd", ".bat", ".ps1", ""]
        } else {
            vec![""]
        };
        for dir in std::env::split_paths(&path_var) {
            for ext in &pathext {
                let candidate = dir.join(format!("{name}{ext}"));
                if candidate.is_file() {
                    return true;
                }
            }
        }
    }

    // 2. Known local install locations on Windows
    #[cfg(windows)]
    {
        if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
            let local = std::path::PathBuf::from(local_app_data);
            match name {
                "code" => {
                    if local
                        .join(r"Programs\Microsoft VS Code\bin\code.cmd")
                        .exists()
                        || local.join(r"Programs\Microsoft VS Code\Code.exe").exists()
                    {
                        return true;
                    }
                }
                "cursor" => {
                    if local.join(r"Programs\cursor\Cursor.exe").exists()
                        || local.join(r"Programs\Cursor\Cursor.exe").exists()
                    {
                        return true;
                    }
                }
                "windsurf" => {
                    if local.join(r"Programs\Windsurf\Windsurf.exe").exists() {
                        return true;
                    }
                }
                "opencode" => {
                    if local.join(r"Programs\OpenCode\OpenCode.exe").exists() {
                        return true;
                    }
                }
                "codex" if local.join(r"Programs\Codex\Codex.exe").exists() => {
                    return true;
                }
                _ => {}
            }
        }
        if let Some(app_data) = std::env::var_os("APPDATA") {
            let appdata = std::path::PathBuf::from(app_data);
            if appdata.join(r"npm").join(format!("{name}.cmd")).exists() {
                return true;
            }
        }
    }

    // 3. Fallback: Run where/which with CREATE_NO_WINDOW so it never opens a console window
    let mut cmd = std::process::Command::new(if cfg!(windows) { "where.exe" } else { "which" });
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `supports_auto_config` drives the "支持一键配置同步写入" label, so it must
    /// mean exactly one thing: activating a profile writes this client's endpoint.
    ///
    /// `opencode` claimed it without that being true — no branch in
    /// `write_client_config` matches it, so it falls through to the arm that logs
    /// and writes nothing, while still reading as configured.
    #[test]
    fn auto_config_flag_matches_which_clients_get_an_endpoint_written() {
        // Mirrors the dispatch in `profile_switch::write_client_config`.
        let writes_endpoint = |id: &str| {
            let id = id.to_ascii_lowercase();
            id.contains("codex") || id.contains("hermes") || id.contains("claude")
        };

        for client in detect_all() {
            assert_eq!(
                client.supports_auto_config,
                writes_endpoint(&client.id),
                "{} 的 supports_auto_config 与它是否真被写入端点不一致：\
                 该字段驱动“支持一键配置同步写入”文案，写不到端点就不能报 true",
                client.id
            );
        }
    }
}
