use crate::state::{GatewayState, ProfileState};
use tauri::{command, State};

/// One client's route: which upstream its requests go to, under which token.
pub fn build_route_config(
    client_id: &str,
    profile_id: &str,
    primary: &polydeck_core::profile::ProviderConfig,
) -> Result<polydeck_gateway::RouteConfig, String> {
    let local_token =
        polydeck_core::credentials::ensure_client_token(client_id).map_err(|e| e.to_string())?;
    let inner = build_gateway_config(profile_id, primary);
    let mut route = inner
        .routes
        .into_iter()
        .next()
        .ok_or_else(|| "route config was built empty".to_string())?;
    route.client_id = client_id.to_string();
    route.upstream.local_token = local_token;
    Ok(route)
}

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
    let (opus_display, sonnet_display, haiku_display) =
        polydeck_core::profile_switch::claude_wire_names(primary);
    // Resolve the tiers here rather than letting the rewriter guess again. Its
    // own name-based guess is a fallback for callers that have not resolved the
    // tiers, and two guesses that disagree put the picker label and the routing
    // on different models.
    let (opus_model, sonnet_model, haiku_model) =
        polydeck_core::profile_switch::claude_tier_candidates(primary);

    let mut config = polydeck_gateway::GatewayConfig::single(
        polydeck_gateway::UpstreamConfig {
            provider_id: Some(primary.id.clone()),
            base_url: primary.base_url.clone(),
            api_key,
            protocol: primary.protocol.to_string(),
            // Overwritten per client by `build_route_config`. A route reaching the
            // gateway with this value would be unreachable, since no client
            // authenticates as it.
            local_token: String::new(),
            max_price_per_request: primary.max_price_per_request,
            responses_mode,
            rate_limit: primary.rate_limit.clone(),
            default_effort_level: primary.default_effort_level.clone(),
            thinking_support: primary.thinking_support,
        },
        polydeck_gateway::model_rewrite::generate_provider_model_rewrites_with_overrides(
            &primary.models,
            primary.supports_1m_context.unwrap_or(false),
            polydeck_gateway::model_rewrite::TierOverrides {
                sonnet_model: Some(sonnet_model),
                opus_model: Some(opus_model),
                haiku_model: Some(haiku_model),
                // Claude Code sends the display names the profile writer gave it,
                // so the gateway resolves the effective ones, not the raw fields.
                sonnet_display_name: Some(sonnet_display),
                opus_display_name: Some(opus_display),
                haiku_display_name: Some(haiku_display),
            },
        ),
    );
    config.listen_addr = Some(std::net::SocketAddr::from((
        [127, 0, 0, 1],
        polydeck_core::profile_switch::GATEWAY_PORT,
    )));
    config
}

/// Bring the gateway in line with the current bindings.
///
/// The single place that decides whether the listener runs and what it serves.
/// Every path that can change bindings — activating, deactivating, editing a
/// profile, the tray, startup — funnels here instead of repeating a stop/rebuild
/// block, which is how those copies drifted apart before.
///
/// Already running plus a changed route set means a hot swap, so binding one more
/// client does not drop in-flight requests belonging to the others.
pub async fn refresh_gateway(
    gw: &GatewayState,
    pm: &ProfileState,
) -> Result<Option<std::net::SocketAddr>, String> {
    let (routes, warnings) = {
        let pm_guard = pm.lock().await;
        collect_routes(&pm_guard)
    };
    for warning in warnings {
        tracing::warn!("{warning}");
    }

    let mut gw_guard = gw.lock().await;

    if routes.is_empty() {
        if let Some(server) = gw_guard.as_mut() {
            server.stop().await;
        }
        *gw_guard = None;
        return Ok(None);
    }

    if let Some(server) = gw_guard.as_mut() {
        if server.is_running() {
            server.apply_routes(routes).await?;
            return Ok(server.addr());
        }
    }

    let mut config = polydeck_gateway::GatewayConfig::single(
        polydeck_gateway::UpstreamConfig {
            provider_id: None,
            base_url: String::new(),
            api_key: String::new(),
            protocol: String::new(),
            local_token: String::new(),
            max_price_per_request: None,
            responses_mode: polydeck_gateway::ResponsesMode::Auto,
            rate_limit: Default::default(),
            default_effort_level: None,
            thinking_support: Default::default(),
        },
        vec![],
    );
    config.listen_addr = Some(std::net::SocketAddr::from((
        [127, 0, 0, 1],
        polydeck_core::profile_switch::GATEWAY_PORT,
    )));
    config.routes = routes;

    let mut server = polydeck_gateway::GatewayServer::new(config);
    let addr = server.start().await?;
    *gw_guard = Some(server);
    Ok(Some(addr))
}

/// A route per bound client on a gateway-enabled profile, plus anything worth saying
/// about the combination.
fn collect_routes(
    pm: &polydeck_core::profile::ProfileManager,
) -> (Vec<polydeck_gateway::RouteConfig>, Vec<String>) {
    let mut routes = Vec::new();
    let mut warnings = Vec::new();
    // provider id → (profile name, rate limit), to catch two profiles sharing an
    // upstream with different limits.
    let mut seen_providers: std::collections::HashMap<
        String,
        (String, polydeck_core::profile::RateLimitSettings),
    > = std::collections::HashMap::new();

    for binding in pm.bindings() {
        let Some(profile) = pm.get_profile(&binding.profile_id) else {
            continue;
        };
        // A direct-connect profile writes the provider's own URL into its clients'
        // configs, so those clients never reach the gateway and need no route.
        if !profile.gateway_enabled {
            continue;
        }
        let Some(primary) = profile
            .providers
            .iter()
            .find(|p| p.is_primary)
            .or_else(|| profile.providers.first())
        else {
            warnings.push(format!(
                "方案「{}」没有 Provider 节点，{} 无法通过网关路由",
                profile.name, binding.client_id
            ));
            continue;
        };

        // The rate limiter is keyed by provider id, which is right for a quota — one
        // upstream, one bucket. But two profiles pointing at the same provider with
        // different limits then overwrite each other's settings on every request, so
        // say which profiles disagree rather than silently letting the last one win.
        if let Some((other_name, other_limit)) = seen_providers.get(&primary.id) {
            if other_name != &profile.name && other_limit != &primary.rate_limit {
                warnings.push(format!(
                    "方案「{}」与「{}」共用 Provider {} 但限流设置不同，两者会互相冲刷；配额本身是按上游共享的",
                    profile.name, other_name, primary.id
                ));
            }
        } else {
            seen_providers.insert(
                primary.id.clone(),
                (profile.name.clone(), primary.rate_limit.clone()),
            );
        }

        match build_route_config(&binding.client_id, &profile.id, primary) {
            Ok(route) => routes.push(route),
            Err(e) => warnings.push(format!("{} 的网关路由构建失败：{e}", binding.client_id)),
        }
    }

    (routes, warnings)
}

#[command]
pub async fn ad_gateway_start(
    gw: State<'_, GatewayState>,
    pm: State<'_, ProfileState>,
) -> Result<String, String> {
    match refresh_gateway(&gw, &pm).await? {
        Some(addr) => Ok(addr.to_string()),
        None => Err(
            "没有客户端绑定到启用了网关的方案，网关无事可做。请先在方案里绑定客户端".to_string(),
        ),
    }
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
pub async fn ad_gateway_status(
    gw: State<'_, GatewayState>,
    pm: State<'_, ProfileState>,
) -> Result<serde_json::Value, String> {
    // What the gateway is serving, not just that it is up: with several profiles
    // behind one port, "running" alone does not say whether *your* client is routed.
    let served: Vec<serde_json::Value> = {
        let pm_guard = pm.lock().await;
        pm_guard
            .bindings()
            .into_iter()
            .filter_map(|b| {
                let profile = pm_guard.get_profile(&b.profile_id)?;
                profile.gateway_enabled.then(|| {
                    serde_json::json!({
                        "clientId": b.client_id,
                        "profileId": profile.id,
                        "profileName": profile.name,
                    })
                })
            })
            .collect()
    };

    let gw = gw.lock().await;
    match gw.as_ref() {
        Some(server) => Ok(serde_json::json!({
            "running": server.is_running(),
            "port": server.port(),
            "routes": served,
        })),
        None => Ok(serde_json::json!({"running": false, "port": null, "routes": served})),
    }
}
