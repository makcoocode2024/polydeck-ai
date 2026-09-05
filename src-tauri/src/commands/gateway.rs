use crate::state::{FailoverState, GatewayState, ProfileState};
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
    failover: &FailoverState,
) -> Result<Option<std::net::SocketAddr>, String> {
    let (routes, warnings) = {
        let pm_guard = pm.lock().await;
        collect_routes(&pm_guard)
    };
    for warning in warnings {
        tracing::warn!("{warning}");
    }

    // Rebuild the failover manager from the current bindings before the listener
    // is touched, so the slot the router reads is never stale mid-swap. Replacing
    // the slot's contents stops the previous monitor loop.
    let manager = {
        let pm_guard = pm.lock().await;
        build_failover_manager(&pm_guard)
    };
    if let Some(previous) = failover.replace(manager.clone()).await {
        previous.stop().await;
    }
    if let Some(manager) = &manager {
        manager.start().await;
    }

    let mut gw_guard = gw.lock().await;

    if routes.is_empty() {
        if let Some(server) = gw_guard.as_mut() {
            server.stop().await;
        }
        *gw_guard = None;
        if let Some(previous) = failover.replace(None).await {
            previous.stop().await;
        }
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

    let mut server =
        polydeck_gateway::GatewayServer::new(config).with_failover_slot((**failover).clone());
    let addr = server.start().await?;
    *gw_guard = Some(server);
    Ok(Some(addr))
}

/// Bound profiles that have both the gateway and failover switched on, in binding
/// order and without duplicates.
///
/// Separated from [`build_failover_manager`] so the selection rules can be tested
/// without a keyring: the construction step reads the stored API key, which a unit
/// test must not touch.
fn failover_candidates(
    pm: &polydeck_core::profile::ProfileManager,
) -> Vec<polydeck_core::profile::Profile> {
    let mut candidates: Vec<polydeck_core::profile::Profile> = Vec::new();
    for binding in pm.bindings() {
        let Some(profile) = pm.get_profile(&binding.profile_id) else {
            continue;
        };
        // A direct-connect profile's clients never reach the gateway, so probing
        // its chain would monitor an upstream nothing is routed through.
        if !profile.gateway_enabled || !profile.failover_enabled {
            continue;
        }
        if candidates.iter().any(|p| p.id == profile.id) {
            continue;
        }
        candidates.push(profile);
    }
    candidates
}

/// A failover manager for the bound profile that asks for one, or `None`.
///
/// Failover is per-upstream-chain, and the manager owns a single primary plus an
/// ordered backup list, so it is built for one profile rather than merged across
/// them. When several bound profiles enable it, the first is used and the rest are
/// reported: silently monitoring one profile's chain while the UI implies all of
/// them are covered is worse than saying so.
///
/// Returns `None` when no bound profile enables failover, when the chosen profile
/// has fewer than two providers (a chain of one has nowhere to fail over to), or
/// when the keyring holds no key for it.
fn build_failover_manager(
    pm: &polydeck_core::profile::ProfileManager,
) -> Option<std::sync::Arc<polydeck_gateway::FailoverManager>> {
    let candidates = failover_candidates(pm);
    let profile = candidates.first()?;
    if candidates.len() > 1 {
        let others: Vec<&str> = candidates[1..].iter().map(|p| p.name.as_str()).collect();
        tracing::warn!(
            "多个方案启用了故障转移，当前只监控「{}」；未监控：{}",
            profile.name,
            others.join("、")
        );
    }

    let api_key = polydeck_core::credentials::get_api_key(&profile.id).unwrap_or_default();
    if api_key.is_empty() {
        tracing::warn!(
            "方案「{}」启用了故障转移，但凭据库中没有 API Key，健康探测无法进行",
            profile.name
        );
        return None;
    }

    let to_config =
        |p: &polydeck_core::profile::ProviderConfig| polydeck_gateway::failover::ProviderConfig {
            id: p.id.clone(),
            name: p.name.clone(),
            base_url: p.base_url.clone(),
            api_key: api_key.clone(),
            default_model: p.default_model.clone(),
        };

    let primary = profile
        .providers
        .iter()
        .find(|p| p.is_primary)
        .or_else(|| profile.providers.first())?;
    let backups: Vec<_> = profile
        .providers
        .iter()
        .filter(|p| p.id != primary.id)
        .map(to_config)
        .collect();

    if backups.is_empty() {
        tracing::warn!(
            "方案「{}」启用了故障转移，但只有一个 Provider 节点，没有可切换的备用上游",
            profile.name
        );
        return None;
    }

    match polydeck_gateway::FailoverManager::new(
        profile.id.clone(),
        to_config(primary),
        backups,
        polydeck_gateway::FailoverOptions::default(),
    ) {
        Ok(manager) => Some(manager),
        Err(e) => {
            tracing::warn!("方案「{}」的故障转移初始化失败：{e}", profile.name);
            None
        }
    }
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
    failover: State<'_, FailoverState>,
) -> Result<String, String> {
    match refresh_gateway(&gw, &pm, &failover).await? {
        Some(addr) => Ok(addr.to_string()),
        None => Err(
            "没有客户端绑定到启用了网关的方案，网关无事可做。请先在方案里绑定客户端".to_string(),
        ),
    }
}

#[command]
pub async fn ad_gateway_stop(
    gw: State<'_, GatewayState>,
    failover: State<'_, FailoverState>,
) -> Result<(), String> {
    let mut gw = gw.lock().await;
    if let Some(server) = gw.as_mut() {
        server.stop().await;
        *gw = None;
    }
    // Otherwise the health-probe loop keeps calling upstreams after the gateway
    // the user just stopped is gone.
    if let Some(previous) = failover.replace(None).await {
        previous.stop().await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use polydeck_core::profile::{ProfileManager, ProfileUpdate};

    fn manager() -> (ProfileManager, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("临时目录创建失败");
        let pm = ProfileManager::with_state_path(dir.path().join("state.json"));
        (pm, dir)
    }

    fn set_flags(pm: &mut ProfileManager, id: &str, gateway: bool, failover: bool) {
        pm.update_profile(
            id,
            ProfileUpdate {
                name: None,
                providers: None,
                clients: None,
                gateway_enabled: Some(gateway),
                failover_enabled: Some(failover),
            },
        )
        .expect("更新方案失败");
    }

    /// Only a profile with *both* switches on is monitored. A direct-connect
    /// profile's clients never reach the gateway, so probing its chain would watch
    /// an upstream nothing routes through.
    #[test]
    fn selects_only_gateway_and_failover_enabled_profiles() {
        let (mut pm, _dir) = manager();

        let plain = pm.create_profile_simple("仅网关").expect("创建失败");
        let direct = pm
            .create_profile_simple("直连带故障转移")
            .expect("创建失败");
        let both = pm
            .create_profile_simple("网关带故障转移")
            .expect("创建失败");

        set_flags(&mut pm, &plain.id, true, false);
        set_flags(&mut pm, &direct.id, false, true);
        set_flags(&mut pm, &both.id, true, true);

        pm.bind_clients(&plain.id, &["codex-cli".into()])
            .expect("绑定失败");
        pm.bind_clients(&direct.id, &["claude-code".into()])
            .expect("绑定失败");
        pm.bind_clients(&both.id, &["claude-desktop".into()])
            .expect("绑定失败");

        let picked = failover_candidates(&pm);
        assert_eq!(
            picked.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            vec!["网关带故障转移"],
            "只有同时启用网关与故障转移的方案才应被监控"
        );
    }

    /// An unbound profile is not monitored however its flags are set: nothing
    /// routes to it, so its chain is not in the request path.
    #[test]
    fn ignores_unbound_profiles() {
        let (mut pm, _dir) = manager();
        let orphan = pm.create_profile_simple("未绑定").expect("创建失败");
        set_flags(&mut pm, &orphan.id, true, true);

        assert!(
            failover_candidates(&pm).is_empty(),
            "未绑定任何客户端的方案不应参与故障转移监控"
        );
    }

    /// Two clients on one profile must not yield it twice; `FailoverManager::new`
    /// rejects duplicate provider ids, so a repeated profile would fail to build.
    #[test]
    fn deduplicates_a_profile_bound_to_several_clients() {
        let (mut pm, _dir) = manager();
        let shared = pm.create_profile_simple("共用方案").expect("创建失败");
        set_flags(&mut pm, &shared.id, true, true);
        pm.bind_clients(&shared.id, &["codex-cli".into(), "claude-code".into()])
            .expect("绑定失败");

        assert_eq!(
            failover_candidates(&pm).len(),
            1,
            "同一方案绑定多个客户端时只应出现一次"
        );
    }
}
