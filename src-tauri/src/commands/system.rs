use crate::state::ProfileState;
use tauri::command;
use tauri::State;

#[command]
pub async fn ad_tray_status() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({"status": "idle"}))
}

#[command]
pub async fn ad_handle_deep_link(url: String) -> Result<serde_json::Value, String> {
    polydeck_core::deep_link::parse(&url)
        .map(|parsed| serde_json::to_value(parsed).unwrap_or_default())
        .map_err(|e| e.to_string())
}

#[command]
pub async fn ad_autolaunch_status() -> Result<serde_json::Value, String> {
    let status = polydeck_core::autolaunch::get_status();
    serde_json::to_value(status).map_err(|e| e.to_string())
}

#[command]
pub async fn ad_set_autolaunch(enabled: bool) -> Result<(), String> {
    polydeck_core::autolaunch::set_enabled(enabled).map_err(|e| e.to_string())
}

/// The forced-Chinese-output setting, alongside what is actually in each
/// client's instructions file.
///
/// The two are reported separately because they drift: the user can edit or
/// delete the block by hand, and the UI should show what is really on disk
/// rather than only what was last saved.
#[command]
pub async fn ad_force_chinese_status(
    pm: State<'_, ProfileState>,
) -> Result<serde_json::Value, String> {
    let enabled = {
        let pm = pm.lock().await;
        pm.settings().force_chinese_output
    };
    let targets = polydeck_core::language_rule::status().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "enabled": enabled, "targets": targets }))
}

/// Write or remove the rule, then persist the setting.
///
/// The files are written first: if that fails outright the setting is left
/// alone, so it never claims a state the files do not have. A per-client failure
/// is reported inside `targets` and still persists the setting, since the other
/// clients did change.
#[command]
pub async fn ad_set_force_chinese(
    enabled: bool,
    pm: State<'_, ProfileState>,
) -> Result<serde_json::Value, String> {
    let targets = polydeck_core::language_rule::apply(enabled).map_err(|e| e.to_string())?;

    {
        let mut pm = pm.lock().await;
        let mut settings = pm.settings();
        settings.force_chinese_output = enabled;
        pm.update_settings(settings).map_err(|e| e.to_string())?;
    }

    Ok(serde_json::json!({ "enabled": enabled, "targets": targets }))
}
