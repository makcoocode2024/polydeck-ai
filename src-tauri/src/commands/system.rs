use crate::state::{CodexRefreshState, FailoverState, GatewayState, ProfileState};
use polydeck_core::client_rules::RuleKind;
use polydeck_core::tray_state::{TrayState, TrayStatus};
use tauri::command;
use tauri::State;

/// Health as the tray would show it, derived from the running gateway and the
/// failover chain.
///
/// Returned a constant `{"status": "idle"}` before, which never changed regardless
/// of whether the gateway was up or an upstream had been circuit-broken.
#[command]
pub async fn ad_tray_status(
    gw: State<'_, GatewayState>,
    pm: State<'_, ProfileState>,
    failover: State<'_, FailoverState>,
) -> Result<serde_json::Value, String> {
    let running = gw.lock().await.as_ref().is_some_and(|s| s.is_running());

    // No manager means failover is off, which is not an unhealthy state — only a
    // manager that reports a tripped chain is.
    let failover_ok = match failover.get().await {
        Some(manager) => {
            let status = manager.status().await;
            !status.all_providers_failed && !status.on_backup
        }
        None => true,
    };

    let mut state = TrayState::new();
    state.update_status(running, failover_ok);
    state.active_profile = {
        let pm_guard = pm.lock().await;
        let names: Vec<String> = pm_guard
            .bindings()
            .into_iter()
            .filter_map(|b| pm_guard.get_profile(&b.profile_id).map(|p| p.name))
            .collect();
        let mut unique: Vec<String> = Vec::new();
        for name in names {
            if !unique.contains(&name) {
                unique.push(name);
            }
        }
        (!unique.is_empty()).then(|| unique.join("、"))
    };

    let status = match state.status {
        TrayStatus::Healthy => "healthy",
        TrayStatus::Degraded => "degraded",
        TrayStatus::Failed => "failed",
        TrayStatus::Offline => "offline",
    };
    Ok(serde_json::json!({
        "status": status,
        "gatewayRunning": state.gateway_running,
        "activeProfile": state.active_profile,
    }))
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
    let targets =
        polydeck_core::client_rules::status(RuleKind::ChineseOutput).map_err(|e| e.to_string())?;
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
    let targets = polydeck_core::client_rules::apply(RuleKind::ChineseOutput, enabled)
        .map_err(|e| e.to_string())?;

    {
        let mut pm = pm.lock().await;
        let mut settings = pm.settings();
        settings.force_chinese_output = enabled;
        pm.update_settings(settings).map_err(|e| e.to_string())?;
    }

    Ok(serde_json::json!({ "enabled": enabled, "targets": targets }))
}

/// The tool-truthfulness setting, alongside what is actually in each client's
/// instructions file. Reported separately for the same reason as the pair above:
/// the block can be edited by hand, so the saved setting is not evidence.
#[command]
pub async fn ad_tool_truthfulness_status(
    pm: State<'_, ProfileState>,
) -> Result<serde_json::Value, String> {
    let enabled = {
        let pm = pm.lock().await;
        pm.settings().enforce_tool_truthfulness
    };
    let targets = polydeck_core::client_rules::status(RuleKind::ToolTruthfulness)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "enabled": enabled, "targets": targets }))
}

/// Write or remove the tool-truthfulness rule, then persist the setting.
///
/// Files first, then the setting, so the setting never claims a state the files
/// do not have. Only this rule's block is touched; the forced-Chinese block in
/// the same file is left alone.
#[command]
pub async fn ad_set_tool_truthfulness(
    enabled: bool,
    pm: State<'_, ProfileState>,
) -> Result<serde_json::Value, String> {
    let targets = polydeck_core::client_rules::apply(RuleKind::ToolTruthfulness, enabled)
        .map_err(|e| e.to_string())?;

    {
        let mut pm = pm.lock().await;
        let mut settings = pm.settings();
        settings.enforce_tool_truthfulness = enabled;
        pm.update_settings(settings).map_err(|e| e.to_string())?;
    }

    Ok(serde_json::json!({ "enabled": enabled, "targets": targets }))
}

/// Whether Claude Code is automatically grouped behind the local gateway, and
/// which bound clients that currently affects.
#[command]
pub async fn ad_auto_client_grouping_status(
    pm: State<'_, ProfileState>,
) -> Result<serde_json::Value, String> {
    let pm = pm.lock().await;
    let enabled = pm.settings().auto_client_grouping;
    let clients = bound_claude_clients(&pm);
    Ok(serde_json::json!({ "enabled": enabled, "clients": clients }))
}

fn bound_claude_clients(pm: &polydeck_core::profile::ProfileManager) -> Vec<String> {
    let mut clients: Vec<String> = pm
        .bindings()
        .into_iter()
        .map(|binding| polydeck_core::binding::normalize_client_id(&binding.client_id))
        .filter(|client| client == "claude-code")
        .collect();
    clients.sort();
    clients.dedup();
    clients
}

/// Toggle automatic Claude Code grouping and immediately rewrite its config.
///
/// The setting is persisted first, then every bound Claude Code config is
/// re-activated so enabling starts routing through the gateway and disabling
/// restores the profile's direct endpoint. Other clients are never touched.
#[command]
pub async fn ad_set_auto_client_grouping(
    enabled: bool,
    pm: State<'_, ProfileState>,
    gw: State<'_, GatewayState>,
    failover: State<'_, FailoverState>,
    refresh: State<'_, CodexRefreshState>,
) -> Result<serde_json::Value, String> {
    let mut rewritten = Vec::new();
    let mut warnings = Vec::new();
    {
        let mut pm = pm.lock().await;
        let clients = bound_claude_clients(&pm);

        let mut settings = pm.settings();
        settings.auto_client_grouping = enabled;
        pm.update_settings(settings).map_err(|e| e.to_string())?;

        for client in clients {
            let Some(profile) = pm.profile_for_client(&client) else {
                continue;
            };
            match polydeck_core::profile_switch::activate_profile(
                &mut pm,
                &profile.id,
                Some(std::slice::from_ref(&client)),
            )
            .await
            {
                Ok(result) => {
                    if result.codex_config_written {
                        refresh.request().await;
                    }
                    if result.clients_written.contains(&client) {
                        rewritten.push(client);
                    }
                    warnings.extend(result.warnings);
                }
                Err(error) => warnings.push(format!("{client} 配置重写失败：{error}")),
            }
        }
    }

    if let Err(error) = crate::commands::gateway::refresh_gateway(&gw, &pm, &failover).await {
        warnings.push(format!("网关同步失败：{error}"));
    }

    Ok(serde_json::json!({
        "enabled": enabled,
        "clients": bound_claude_clients(&*pm.lock().await),
        "rewrittenClients": rewritten,
        "warnings": warnings,
    }))
}
