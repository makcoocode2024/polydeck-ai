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
                    let pm_guard = pm.lock().await;
                    if let Some(active) = pm_guard.active_profile() {
                        if active.gateway_enabled {
                            if let Some(primary) = active
                                .providers
                                .iter()
                                .find(|p| p.is_primary)
                                .or_else(|| active.providers.first())
                            {
                                let gw_config = crate::commands::gateway::build_gateway_config(
                                    &active.id, primary,
                                );
                                let mut server = polydeck_gateway::GatewayServer::new(gw_config);
                                if let Ok(_) = server.start().await {
                                    let mut gw_guard = gw.lock().await;
                                    *gw_guard = Some(server);
                                }
                            }
                        }
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
            commands::profile::ad_switch_profile,
            commands::profile::ad_get_profile_templates,
            commands::profile::ad_probe_provider,
            commands::profile::ad_probe_rate_limits,
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
