//! System tray implementation for AI Deck

use crate::state::{GatewayState, ProfileState};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager,
};

pub const TRAY_ID: &str = "ai-deck-tray";

pub fn create_tray(app: &AppHandle) -> Result<TrayIcon, tauri::Error> {
    let menu = build_tray_menu(app)?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon not found".into()))?;

    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("AI Deck - 大模型网关与开发工具箱")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            handle_menu_event(app, event.id().as_ref());
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                toggle_main_window(app);
            }
        })
        .build(app)?;

    Ok(tray)
}

pub fn toggle_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let is_visible = window.is_visible().unwrap_or(false);
        let is_minimized = window.is_minimized().unwrap_or(false);

        if is_visible && !is_minimized {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
    }
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn hide_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

pub fn build_tray_menu(app: &AppHandle) -> Result<Menu<tauri::Wry>, tauri::Error> {
    let menu = Menu::new(app)?;

    let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let hide_item = MenuItem::with_id(app, "hide", "隐藏至托盘", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;

    // Profiles submenu
    let pm_state = app.try_state::<ProfileState>();
    let profiles_submenu = if let Some(pm) = pm_state {
        let guard = tauri::async_runtime::block_on(async { pm.lock().await });
        let profiles = guard.list_profiles();

        let mut sub_items: Vec<Box<dyn tauri::menu::IsMenuItem<tauri::Wry>>> = Vec::new();
        for p in profiles {
            // No single checkmark any more: several profiles can be in use at once,
            // so each row reports how many clients follow it instead of claiming to
            // be the one active choice.
            let bound = guard.clients_for_profile(&p.id).len();
            let label = if bound > 0 {
                format!("✓ {} ({bound} 个客户端)", p.name)
            } else {
                format!("   {}", p.name)
            };
            let item_id = format!("switch_profile:{}", p.id);
            let item = MenuItem::with_id(app, &item_id, &label, true, None::<&str>)?;
            sub_items.push(Box::new(item));
        }

        if sub_items.is_empty() {
            let empty_item =
                MenuItem::with_id(app, "no_profiles", "暂无配置方案", false, None::<&str>)?;
            sub_items.push(Box::new(empty_item));
        }

        let slice: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> =
            sub_items.iter().map(|b| b.as_ref()).collect();
        Submenu::with_items(app, "切换配置方案", true, &slice)?
    } else {
        Submenu::new(app, "切换配置方案", false)?
    };

    let sep2 = PredefinedMenuItem::separator(app)?;
    let restart_item = MenuItem::with_id(app, "restart", "重启 AI Deck", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出应用", true, None::<&str>)?;

    menu.append(&show_item)?;
    menu.append(&hide_item)?;
    menu.append(&sep1)?;
    menu.append(&profiles_submenu)?;
    menu.append(&sep2)?;
    menu.append(&restart_item)?;
    menu.append(&quit_item)?;

    Ok(menu)
}

pub fn handle_menu_event(app: &AppHandle, menu_id: &str) {
    match menu_id {
        "show" => {
            show_main_window(app);
        }
        "hide" => {
            hide_main_window(app);
        }
        "restart" => {
            app.restart();
        }
        "quit" => {
            app.exit(0);
        }
        s if s.starts_with("switch_profile:") => {
            let profile_id = s.trim_start_matches("switch_profile:").to_string();
            let app_handle = app.clone();
            tauri::async_runtime::spawn(async move {
                if let (Some(pm), Some(gw)) = (
                    app_handle.try_state::<ProfileState>(),
                    app_handle.try_state::<GatewayState>(),
                ) {
                    let mut pm_guard = pm.lock().await;
                    // The tray row means "put this profile's own clients on it",
                    // matching the activate button rather than offering a per-client
                    // choice there is no room to present.
                    if let Ok(result) = polydeck_core::profile_switch::activate_profile(
                        &mut pm_guard,
                        &profile_id,
                        None,
                    )
                    .await
                    {
                        if result.success {
                            if let Some(active) = pm_guard.get_profile(&profile_id) {
                                let mut gw_guard = gw.lock().await;
                                if let Some(server) = gw_guard.as_mut() {
                                    server.stop().await;
                                    *gw_guard = None;
                                }
                                if active.gateway_enabled {
                                    if let Some(primary) = active
                                        .providers
                                        .iter()
                                        .find(|p| p.is_primary)
                                        .or_else(|| active.providers.first())
                                    {
                                        let gw_config =
                                            crate::commands::gateway::build_gateway_config(
                                                &active.id, primary,
                                            );
                                        let mut server =
                                            polydeck_gateway::GatewayServer::new(gw_config);
                                        let _ = server.start().await;
                                        *gw_guard = Some(server);
                                    }
                                }
                            }
                        }
                    }
                }
            });
        }
        _ => {}
    }
}
