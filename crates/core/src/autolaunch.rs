//! Cross-platform auto-launch on boot.
//!
//! Windows is the only implemented platform. `get_status` reported a hardcoded
//! `enabled: false` and `set_enabled` only logged, so the Settings toggle wrote
//! nothing and reverted on restart while telling the user it had saved.
//!
//! The registry is driven through `reg.exe` rather than a raw `RegSetValueExW`
//! FFI call: one `HKCU\...\Run` string value is exactly what `reg.exe` exposes,
//! and shelling out keeps this module free of `unsafe`. Other platforms report
//! `supported: false` instead of pretending to succeed.

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `HKCU` autorun key. Per-user, so no elevation is needed.
#[cfg(windows)]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

/// Value name under [`RUN_KEY`].
#[cfg(windows)]
const RUN_VALUE: &str = "PolyDeck";

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AutoLaunchStatus {
    pub enabled: bool,
    pub method: String,
    /// False on platforms with no implementation, so the UI can disable the
    /// toggle rather than offer a switch that silently does nothing.
    pub supported: bool,
    /// The command line that will run at logon, when one is registered.
    pub command: Option<String>,
}

/// The current executable, quoted for a registry command string.
#[cfg(windows)]
fn launch_command() -> AppResult<String> {
    let exe = std::env::current_exe()
        .map_err(|e| AppError::Internal(format!("无法定位当前可执行文件：{e}")))?;
    Ok(format!("\"{}\"", exe.display()))
}

/// Read the registered command, if any.
#[cfg(windows)]
fn query_run_value() -> AppResult<Option<String>> {
    let output = std::process::Command::new("reg")
        .args(["query", RUN_KEY, "/v", RUN_VALUE])
        .output()
        .map_err(|e| AppError::Internal(format!("无法执行 reg query：{e}")))?;

    // A missing value exits non-zero, which is the normal "not enabled" case
    // rather than a failure worth surfacing.
    if !output.status.success() {
        return Ok(None);
    }

    let text = String::from_utf8_lossy(&output.stdout);
    // Line shape: `    PolyDeck    REG_SZ    "C:\path\polydeck.exe"`
    let command = text
        .lines()
        .find(|line| line.trim_start().starts_with(RUN_VALUE))
        .and_then(|line| line.split("REG_SZ").nth(1))
        .map(|rest| rest.trim().to_string())
        .filter(|rest| !rest.is_empty());
    Ok(command)
}

#[cfg(windows)]
pub fn get_status() -> AutoLaunchStatus {
    // A read failure is reported as "not enabled" with no command rather than
    // propagated: the toggle still needs a state to render.
    let command = query_run_value().unwrap_or(None);
    AutoLaunchStatus {
        enabled: command.is_some(),
        method: "registry".into(),
        supported: true,
        command,
    }
}

#[cfg(not(windows))]
pub fn get_status() -> AutoLaunchStatus {
    AutoLaunchStatus {
        enabled: false,
        method: "unsupported".into(),
        supported: false,
        command: None,
    }
}

#[cfg(windows)]
pub fn set_enabled(enabled: bool) -> AppResult<()> {
    let output = if enabled {
        let command = launch_command()?;
        std::process::Command::new("reg")
            .args([
                "add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/d", &command, "/f",
            ])
            .output()
    } else {
        std::process::Command::new("reg")
            .args(["delete", RUN_KEY, "/v", RUN_VALUE, "/f"])
            .output()
    }
    .map_err(|e| AppError::Internal(format!("无法执行 reg 命令：{e}")))?;

    // Deleting a value that is not there exits non-zero; the requested end state
    // still holds, so it is not an error.
    if !output.status.success() {
        if !enabled && query_run_value()?.is_none() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(AppError::Internal(format!(
            "写入开机自启注册表失败：{}",
            if stderr.is_empty() {
                "reg 命令返回失败".to_string()
            } else {
                stderr
            }
        )));
    }

    tracing::info!("开机自启已设置为 {enabled}");
    Ok(())
}

#[cfg(not(windows))]
pub fn set_enabled(_enabled: bool) -> AppResult<()> {
    Err(AppError::Internal("当前平台尚未实现开机自启".to_string()))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// The status must describe the registry, not a constant. It reported
    /// `enabled: false` unconditionally before, which disagreed with the key
    /// whenever autostart was actually registered.
    #[test]
    fn status_reflects_registry_and_survives_round_trip() {
        let original = query_run_value().expect("查询注册表失败");

        set_enabled(true).expect("写入自启失败");
        let on = get_status();
        assert!(on.enabled, "写入后状态必须为已启用");
        assert!(on.supported, "Windows 上必须报告 supported");
        assert!(
            on.command.is_some_and(|c| c.contains(".exe")),
            "启用后必须记录可执行文件路径"
        );

        set_enabled(false).expect("清除自启失败");
        assert!(!get_status().enabled, "清除后状态必须为未启用");

        // Deleting an absent value is the documented no-op, not a failure.
        set_enabled(false).expect("重复清除必须幂等");

        // Leave the developer's machine as it was found.
        if let Some(command) = original {
            let _ = std::process::Command::new("reg")
                .args([
                    "add", RUN_KEY, "/v", RUN_VALUE, "/t", "REG_SZ", "/d", &command, "/f",
                ])
                .output();
        }
    }
}
