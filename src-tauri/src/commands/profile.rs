use crate::state::{GatewayState, ProfileState};
use polydeck_core::profile::Profile;
use tauri::{command, State};

#[command]
pub async fn ad_list_profiles(pm: State<'_, ProfileState>) -> Result<Vec<Profile>, String> {
    let pm = pm.lock().await;
    Ok(pm.list_profiles())
}

#[command]
pub async fn ad_get_active_profile(pm: State<'_, ProfileState>) -> Result<Option<Profile>, String> {
    let pm = pm.lock().await;
    Ok(pm.active_profile())
}

#[command]
pub async fn ad_create_profile(
    pm: State<'_, ProfileState>,
    name: String,
) -> Result<Profile, String> {
    let mut pm = pm.lock().await;
    pm.create_profile_simple(&name).map_err(|e| e.to_string())
}

#[command]
pub async fn ad_update_profile(
    pm: State<'_, ProfileState>,
    gw: State<'_, GatewayState>,
    id: String,
    update: polydeck_core::profile::ProfileUpdate,
) -> Result<Profile, String> {
    let (updated, is_active) = {
        let mut pm_guard = pm.lock().await;
        let updated = pm_guard
            .update_profile(&id, update)
            .map_err(|e| e.to_string())?;
        let is_active = pm_guard
            .active_profile()
            .map(|p| p.id == id)
            .unwrap_or(false);
        if is_active {
            let _ = polydeck_core::profile_switch::switch_profile(&mut pm_guard, &id).await;
        }
        (updated, is_active)
    };

    if is_active {
        let mut gw_guard = gw.lock().await;
        if let Some(server) = gw_guard.as_mut() {
            server.stop().await;
            *gw_guard = None;
        }
        if updated.gateway_enabled {
            if let Some(primary) = updated
                .providers
                .iter()
                .find(|p| p.is_primary)
                .or_else(|| updated.providers.first())
            {
                let gw_config =
                    crate::commands::gateway::build_gateway_config(&updated.id, primary);
                let mut server = polydeck_gateway::GatewayServer::new(gw_config);
                let _ = server.start().await;
                *gw_guard = Some(server);
            }
        }
    }

    Ok(updated)
}

#[command]
pub async fn ad_duplicate_profile(
    pm: State<'_, ProfileState>,
    id: String,
) -> Result<Profile, String> {
    let mut pm = pm.lock().await;
    pm.duplicate_profile(&id).map_err(|e| e.to_string())
}

#[command]
pub async fn ad_delete_profile(pm: State<'_, ProfileState>, id: String) -> Result<(), String> {
    let mut pm = pm.lock().await;
    pm.delete_profile(&id).map_err(|e| e.to_string())
}

#[command]
pub async fn ad_switch_profile(
    pm: State<'_, ProfileState>,
    gw: State<'_, GatewayState>,
    id: String,
) -> Result<polydeck_core::profile_switch::SwitchResult, String> {
    let (result, active_opt) = {
        let mut pm_guard = pm.lock().await;
        let result = polydeck_core::profile_switch::switch_profile(&mut pm_guard, &id)
            .await
            .map_err(|e| e.to_string())?;

        if !result.success {
            return Err(result.message);
        }
        let active = pm_guard.get_profile(&id);
        (result, active)
    };

    if let Some(active) = active_opt {
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
                let gw_config = crate::commands::gateway::build_gateway_config(&active.id, primary);
                let mut server = polydeck_gateway::GatewayServer::new(gw_config);
                let _ = server.start().await;
                *gw_guard = Some(server);
            }
        }
    }

    Ok(result)
}

#[command]
pub async fn ad_get_profile_templates(
) -> Result<Vec<polydeck_core::profile_templates::ProfileTemplate>, String> {
    Ok(polydeck_core::profile_templates::builtin_templates())
}

#[command]
pub async fn ad_probe_provider(
    base_url: String,
    api_key: String,
    accept_invalid_certs: Option<bool>,
) -> Result<polydeck_core::protocol::ProbeResult, String> {
    polydeck_core::protocol::probe(&base_url, &api_key, accept_invalid_certs.unwrap_or(false))
        .await
        .map_err(|e| e.to_string())
}

#[command]
pub async fn ad_test_provider_chat(
    base_url: String,
    api_key: String,
    model: String,
    protocol: Option<polydeck_core::types::ProtocolKind>,
    accept_invalid_certs: Option<bool>,
    prompt: Option<String>,
) -> Result<polydeck_core::protocol::ChatTestResult, String> {
    polydeck_core::protocol::test_chat(
        &base_url,
        &api_key,
        &model,
        protocol,
        accept_invalid_certs.unwrap_or(false),
        prompt.as_deref(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[command]
pub async fn ad_set_profile_api_key(profile_id: String, api_key: String) -> Result<(), String> {
    polydeck_core::credentials::set_api_key(&profile_id, &api_key).map_err(|e| e.to_string())
}

#[command]
pub async fn ad_get_profile_api_key(profile_id: String) -> Result<Option<String>, String> {
    match polydeck_core::credentials::get_api_key(&profile_id) {
        Ok(k) => Ok(Some(k)),
        Err(_) => Ok(None),
    }
}

#[command]
pub async fn ad_probe_rate_limits(
    base_url: String,
    api_key: String,
    model: Option<String>,
    accept_invalid_certs: Option<bool>,
) -> Result<polydeck_core::protocol::RateLimitRecommendation, String> {
    polydeck_core::protocol::probe_rate_limits(
        &base_url,
        &api_key,
        model.as_deref(),
        accept_invalid_certs.unwrap_or(false),
    )
    .await
    .map_err(|e| e.to_string())
}

/// Re-probe whether a provider returns *signed* thinking blocks, and persist it.
///
/// Persisting is the point: the gateway reads `thinking_support` when it builds
/// its config, so an answer that only reached the UI would leave injection gated
/// on a stale value.
#[command]
pub async fn ad_probe_thinking_support(
    pm: State<'_, ProfileState>,
    profile_id: String,
    provider_id: String,
) -> Result<polydeck_core::types::ThinkingSupport, String> {
    let (base_url, model, accept_invalid_certs) = {
        let pm = pm.lock().await;
        let profile = pm
            .list_profiles()
            .into_iter()
            .find(|p| p.id == profile_id)
            .ok_or_else(|| format!("profile {profile_id} not found"))?;
        let provider = profile
            .providers
            .iter()
            .find(|p| p.id == provider_id)
            .ok_or_else(|| format!("provider {provider_id} not found"))?;
        (
            provider.base_url.clone(),
            provider.default_model.clone(),
            provider.accept_invalid_certs,
        )
    };
    let api_key =
        polydeck_core::credentials::get_api_key(&profile_id).map_err(|e| e.to_string())?;

    let support = polydeck_core::reasoning_verification::probe_anthropic_thinking(
        &base_url,
        &api_key,
        &model,
        accept_invalid_certs,
    )
    .await
    .map_err(|e| e.to_string())?;

    let mut pm = pm.lock().await;
    let mut providers = pm
        .list_profiles()
        .into_iter()
        .find(|p| p.id == profile_id)
        .ok_or_else(|| format!("profile {profile_id} not found"))?
        .providers;
    for provider in providers.iter_mut() {
        if provider.id == provider_id {
            provider.thinking_support = support;
        }
    }
    pm.update_profile(
        &profile_id,
        polydeck_core::profile::ProfileUpdate {
            name: None,
            providers: Some(providers),
            clients: None,
            gateway_enabled: None,
            failover_enabled: None,
        },
    )
    .map_err(|e| e.to_string())?;

    Ok(support)
}
