//! AI Deck Tauri application entry point
//!
//! Modular IPC command layer replaces the monolithic God File pattern.

mod commands;
mod state;
mod tray;

use polydeck_core::profile::ProfileManager;
use polydeck_inject::InjectionManager;
use state::{GatewayState, InjectState, ProfileState};
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

const INJECT_SCRIPT_SOURCE: &str = "// AI Deck bridge placeholder";

pub fn run() {
    // First thing, before any state is loaded: the gateway and profile code paths
    // emit tracing events, and until this installs a file subscriber they go to a
    // stdout that a windowed build throws away. Nothing had called this, which is
    // why ~/.ai-deck/logs/ held nothing newer than 2026-08-19.
    if let Err(err) = polydeck_core::logging::LogRouter::init() {
        // No logging yet, so this is the one place a bare eprintln earns its keep.
        eprintln!("[PolyDeck] file logging unavailable: {err}");
    }

    let pm = ProfileManager::load().unwrap_or_default();
    let profile_state: ProfileState = Arc::new(Mutex::new(pm));
    let gateway_state: GatewayState = Arc::new(Mutex::new(None));
    let inject_state: InjectState =
        Arc::new(Mutex::new(InjectionManager::new(INJECT_SCRIPT_SOURCE)));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(profile_state)
        .manage(gateway_state)
        .manage(inject_state)
        .setup(|app| {
            // Setup system tray
            let _ = tray::create_tray(app.handle());

            // Auto-start gateway if active profile has gateway enabled
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let (Some(pm), Some(gw)) = (
                    app_handle.try_state::<ProfileState>(),
                    app_handle.try_state::<GatewayState>(),
                ) {
                    // A migrated state.json has bindings, but the clients' config
                    // files still carry the pre-binding bearer that the gateway will
                    // no longer accept. Re-activate each bound profile first to
                    // issue tokens and rewrite those files; without this every
                    // client that worked before the upgrade would 401.
                    let profiles_to_reapply: Vec<(String, Vec<String>)> = {
                        let pm_guard = pm.lock().await;
                        if pm_guard.needs_reapply() {
                            let mut by_profile: std::collections::HashMap<String, Vec<String>> =
                                std::collections::HashMap::new();
                            for binding in pm_guard.bindings() {
                                by_profile
                                    .entry(binding.profile_id)
                                    .or_default()
                                    .push(binding.client_id);
                            }
                            by_profile.into_iter().collect()
                        } else {
                            Vec::new()
                        }
                    };
                    for (profile_id, clients) in profiles_to_reapply {
                        let mut pm_guard = pm.lock().await;
                        if let Err(e) = polydeck_core::profile_switch::activate_profile(
                            &mut pm_guard,
                            &profile_id,
                            Some(&clients),
                        )
                        .await
                        {
                            tracing::warn!(
                                "迁移后重新下发方案 {profile_id} 的配置失败：{e}；相关客户端需要手动重新激活"
                            );
                        }
                    }

                    if let Err(e) = crate::commands::gateway::refresh_gateway(&gw, &pm).await {
                        tracing::warn!("启动时网关未能就绪：{e}");
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Prevent app from quitting on window close, hide to system tray instead
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            // core
            commands::core::ad_get_version,
            commands::core::ad_ping,
            commands::core::ad_detect_clients,
            // profile
            commands::profile::ad_list_profiles,
            commands::profile::ad_get_active_profile,
            commands::profile::ad_create_profile,
            commands::profile::ad_duplicate_profile,
            commands::profile::ad_update_profile,
            commands::profile::ad_delete_profile,
            commands::profile::ad_activate_profile,
            commands::profile::ad_deactivate_clients,
            commands::profile::ad_list_client_bindings,
            commands::profile::ad_client_connection_info,
            commands::profile::ad_rotate_client_token,
            commands::profile::ad_get_profile_templates,
            commands::profile::ad_probe_provider,
            commands::profile::ad_probe_rate_limits,
            commands::profile::ad_probe_thinking_support,
            commands::profile::ad_test_provider_chat,
            commands::profile::ad_set_profile_api_key,
            commands::profile::ad_get_profile_api_key,
            // gateway
            commands::gateway::ad_gateway_start,
            commands::gateway::ad_gateway_stop,
            commands::gateway::ad_gateway_status,
            // failover
            commands::failover::ad_failover_status,
            commands::failover::ad_failover_history,
            // extensions
            commands::extensions::ad_list_mcp_servers,
            commands::extensions::ad_list_skills,
            commands::extensions::ad_list_prompts,
            // history
            commands::history::ad_query_history,
            commands::history::ad_export_history,
            commands::history::ad_create_encrypted_backup,
            commands::history::ad_restore_encrypted_backup,
            // inject
            commands::inject::ad_inject_status,
            commands::inject::ad_inject_install_native,
            commands::inject::ad_inject_uninstall_native,
            commands::inject::ad_inject_repair,
            // system
            commands::system::ad_tray_status,
            commands::system::ad_handle_deep_link,
            commands::system::ad_autolaunch_status,
            commands::system::ad_set_autolaunch,
            commands::system::ad_force_chinese_status,
            commands::system::ad_set_force_chinese,
            commands::system::ad_tool_truthfulness_status,
            commands::system::ad_set_tool_truthfulness,
            // proxy
            commands::proxy::ad_detect_proxy,
            // ops
            commands::ops::ad_run_diagnostics,
            commands::ops::ad_check_update,
            commands::ops::ad_get_logs,
            // importer
            commands::importer::ad_detect_importable,
            commands::importer::ad_import_from_provider_deck,
        ])
        .run(tauri::generate_context!())
        .expect("error while running AI Deck");
}
