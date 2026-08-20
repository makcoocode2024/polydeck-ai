use crate::state::{GatewayState, ProfileState};
use tauri::{command, State};

pub fn build_gateway_config(
    profile_id: &str,
    primary: &polydeck_core::profile::ProviderConfig,
) -> polydeck_gateway::GatewayConfig {
    let api_key = polydeck_core::credentials::get_api_key(profile_id).unwrap_or_default();
    let responses_mode = match primary.protocol {
        polydeck_core::types::ProtocolKind::Responses => polydeck_gateway::ResponsesMode::Native,
        polydeck_core::types::ProtocolKind::OpenAI => {
            if primary.codex_compat == polydeck_core::types::CodexToolCompat::ResponsesCustom
                || primary.codex_compat == polydeck_core::types::CodexToolCompat::ResponsesFunction
            {
                polydeck_gateway::ResponsesMode::Native
            } else if primary.codex_compat == polydeck_core::types::CodexToolCompat::ChatFunction {
                polydeck_gateway::ResponsesMode::Bridge
            } else {
                polydeck_gateway::ResponsesMode::Auto
            }
        }
        _ => polydeck_gateway::ResponsesMode::Auto,
    };

    polydeck_gateway::GatewayConfig {
        listen_addr: Some(std::net::SocketAddr::from(([127, 0, 0, 1], 18888))),
        upstream: polydeck_gateway::UpstreamConfig {
            provider_id: Some(primary.id.clone()),
            base_url: primary.base_url.clone(),
            api_key,
            protocol: primary.protocol.to_string(),
            local_token: "ai-deck-local".into(),
            max_price_per_request: primary.max_price_per_request,
            responses_mode,
            rate_limit: primary.rate_limit.clone(),
        },
        model_rewrites: vec![],
        timeout: std::time::Duration::from_secs(120),
        max_retries: 3,
    }
}

#[command]
pub async fn ad_gateway_start(
    gw: State<'_, GatewayState>,
    pm: State<'_, ProfileState>,
) -> Result<String, String> {
    let mut gw_guard = gw.lock().await;

    if let Some(server) = gw_guard.as_mut() {
        server.stop().await;
        *gw_guard = None;
    }

    let (active_profile, primary) = {
        let pm_guard = pm.lock().await;
        let active_profile = pm_guard
            .active_profile()
            .or_else(|| pm_guard.list_profiles().into_iter().next())
            .ok_or_else(|| "û�п��õ����÷�������������".to_string())?;

        let primary = active_profile
            .providers
            .iter()
            .find(|p| p.is_primary)
            .or_else(|| active_profile.providers.first())
            .cloned()
            .ok_or_else(|| "���÷�����δ���� Provider �ڵ�".to_string())?;

        (active_profile, primary)
    };

    let gw_config = build_gateway_config(&active_profile.id, &primary);
    let mut server = polydeck_gateway::GatewayServer::new(gw_config);
    let addr = server.start().await?;
    *gw_guard = Some(server);

    Ok(addr.to_string())
}

#[command]
pub async fn ad_gateway_stop(gw: State<'_, GatewayState>) -> Result<(), String> {
    let mut gw = gw.lock().await;
    if let Some(server) = gw.as_mut() {
        server.stop().await;
        *gw = None;
    }
    Ok(())
}

#[command]
pub async fn ad_gateway_status(gw: State<'_, GatewayState>) -> Result<serde_json::Value, String> {
    let gw = gw.lock().await;
    match gw.as_ref() {
        Some(server) => Ok(serde_json::json!({
            "running": server.is_running(),
            "port": server.port(),
        })),
        None => Ok(serde_json::json!({"running": false, "port": null})),
    }
}
