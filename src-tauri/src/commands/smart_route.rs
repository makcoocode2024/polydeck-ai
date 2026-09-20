//! Smart route IPC: read/write the routing config, simulate a decision,
//! read the audit trail. The config lives in `AppSettings`, so writing it and
//! calling [`super::gateway::refresh_gateway`] is the whole persistence and
//! hot-reload story.

use crate::state::{FailoverState, GatewayState, ProfileState};
use polydeck_core::profile::ProfileManager;
use polydeck_core::smart_route::SmartRouteSettings;
use tauri::{command, State};

#[command]
pub async fn ad_get_route_config(
    pm: State<'_, ProfileState>,
) -> Result<SmartRouteSettings, String> {
    let pm = pm.lock().await;
    Ok(pm.settings().smart_route)
}

#[command]
pub async fn ad_update_route_config(
    config: SmartRouteSettings,
    pm: State<'_, ProfileState>,
    gw: State<'_, GatewayState>,
    failover: State<'_, FailoverState>,
) -> Result<serde_json::Value, String> {
    // Surface bad regexes before saving: the engine refuses those rules at
    // compile time, and the UI needs to tell the user which rule is dead.
    let warnings = config.validate();
    {
        let mut pm = pm.lock().await;
        let mut settings = pm.settings();
        settings.smart_route = config.clone();
        pm.update_settings(settings).map_err(|e| e.to_string())?;
    }
    // Hot swap the routing table so the new rules serve the next request,
    // with no listener restart and no dropped in-flight requests.
    super::gateway::refresh_gateway(&gw, &pm, &failover).await?;
    Ok(serde_json::json!({ "config": config, "warnings": warnings }))
}

/// The audit state is the same Arc the gateway writes to; see
/// [`super::gateway::audit_log`]. Managing it separately from the gateway
/// lifetime keeps audit reads working while the listener is stopped.
pub struct RouteAuditState(pub std::sync::Arc<polydeck_gateway::router::smart_route::AuditLog>);

#[command]
pub async fn ad_get_route_audit(
    request_id: Option<String>,
    limit: Option<u32>,
    audit: State<'_, RouteAuditState>,
) -> Result<Vec<polydeck_gateway::router::smart_route::RouteAuditRecord>, String> {
    let limit = limit.unwrap_or(100).min(1_000) as usize;
    Ok(audit.0.snapshot(request_id.as_deref(), limit))
}

/// Simulate a routing decision without sending anything anywhere. The engine
/// is built from the same stored settings the gateway compiles, on a
/// synthetic Messages body, with the union of all bound gateway providers'
/// model lists playing the validation role.
#[command]
pub async fn ad_simulate_route(
    model: String,
    prompt: String,
    effort: Option<String>,
    pm: State<'_, ProfileState>,
) -> Result<Option<polydeck_gateway::router::smart_route::RouteDecision>, String> {
    let (settings, provider_models) = {
        let pm = pm.lock().await;
        let settings = pm.settings().smart_route;
        let models = bound_gateway_models(&pm);
        (settings, models)
    };
    polydeck_gateway::router::smart_route::simulate(
        &settings,
        &model,
        &prompt,
        effort.as_deref(),
        &provider_models,
    )
}

/// Models of the primary provider of every bound, gateway-enabled profile.
/// Empty when the gateway serves nothing — simulation then skips validation.
fn bound_gateway_models(pm: &ProfileManager) -> Vec<String> {
    let mut models: Vec<String> = Vec::new();
    for binding in pm.bindings() {
        let Some(profile) = pm.get_profile(&binding.profile_id) else {
            continue;
        };
        if !profile.gateway_enabled {
            continue;
        }
        let Some(primary) = profile
            .providers
            .iter()
            .find(|p| p.is_primary)
            .or_else(|| profile.providers.first())
        else {
            continue;
        };
        for m in &primary.models {
            if !models.contains(m) {
                models.push(m.clone());
            }
        }
    }
    models
}
