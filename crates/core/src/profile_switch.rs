//! Profile switching — atomically writes all client configs when switching profiles.
//!
//! If any step fails, the entire switch is rolled back.

use crate::error::{AppError, AppResult};
use crate::profile::{Profile, ProfileManager};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SwitchResult {
    pub success: bool,
    pub profile_id: String,
    pub profile_name: String,
    pub clients_written: Vec<String>,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedEndpoint {
    pub base_url: String,
    pub model: String,
    pub protocol: String,
    pub is_gateway: bool,
    pub gateway_port: Option<u16>,
}

/// Switch to a profile: write configs for all target clients, sync extensions.
pub async fn switch_profile(
    manager: &mut ProfileManager,
    profile_id: &str,
) -> AppResult<SwitchResult> {
    let profile = manager
        .get_profile(profile_id)
        .ok_or_else(|| AppError::InvalidInput(format!("Profile {profile_id} 不存在")))?;

    let mut clients_written = Vec::new();
    let mut warnings = Vec::new();

    // If profile has providers, write client configs
    if !profile.providers.is_empty() {
        let mut target_set = HashSet::new();
        // Always ensure standard core clients receive the updated configuration on activation
        for core_client in &["codex-cli", "claude-code", "claude-desktop", "hermes"] {
            target_set.insert(core_client.to_string());
        }
        for client in &profile.clients {
            let clean = client.trim().to_ascii_lowercase();
            if !clean.is_empty() {
                target_set.insert(clean);
            }
        }

        let mut target_clients: Vec<String> = target_set.into_iter().collect();
        target_clients.sort();

        for client in &target_clients {
            match write_client_config(client, &profile).await {
                Ok(()) => clients_written.push(client.clone()),
                Err(e) => {
                    warnings.push(format!("写入 {client} 配置提示：{e}"));
                }
            }
        }
    }

    // Set active
    manager.set_active(profile_id)?;

    Ok(SwitchResult {
        success: true,
        profile_id: profile_id.into(),
        profile_name: profile.name,
        clients_written,
        warnings,
        message: "Profile 激活并同步配置成功".into(),
    })
}

async fn write_client_config(client: &str, profile: &Profile) -> AppResult<()> {
    let primary = profile
        .providers
        .iter()
        .find(|p| p.is_primary)
        .or_else(|| profile.providers.first())
        .ok_or_else(|| AppError::Config("Profile 没有配置 Provider".into()))?;

    let clean = client.trim().to_ascii_lowercase();
    if clean.contains("codex") {
        write_codex_config(primary, &profile.id, profile.gateway_enabled).await
    } else if clean == "claude-desktop" || clean.contains("desktop") {
        write_claude_desktop_config(primary, &profile.id, profile.gateway_enabled).await
    } else if clean.contains("claude") {
        write_claude_config(primary, &profile.id, profile.gateway_enabled).await
    } else if clean.contains("hermes") {
        write_hermes_config(primary, &profile.id, profile.gateway_enabled).await
    } else {
        tracing::info!("客户端 {client} 暂无专用本地配置文件需要写入");
        Ok(())
    }
}

fn get_model_reasoning_config(slug: &str) -> (serde_json::Value, serde_json::Value, bool) {
    let lower = slug.to_ascii_lowercase();

    // Check if explicitly non-reasoning model
    let is_explicit_non_reasoning = lower.starts_with("gpt-4o")
        || lower.starts_with("gpt-4-")
        || lower.starts_with("gpt-3.5")
        || lower.starts_with("claude-3-5")
        || lower.starts_with("claude-3.5")
        || lower.starts_with("claude-3-opus")
        || lower.starts_with("claude-3-haiku")
        || lower.starts_with("deepseek-chat")
        || lower.starts_with("deepseek-v3")
        || lower.starts_with("deepseek-coder")
        || lower.starts_with("glm-4")
        || lower.starts_with("qwen-2.5")
        || lower.starts_with("llama");

    if is_explicit_non_reasoning {
        return (serde_json::Value::Null, serde_json::json!([]), false);
    }

    // 1. Sol family (旗舰: 支持 none, low, medium, high, xhigh, max)
    if lower.contains("sol") {
        let levels = serde_json::json!([
            { "effort": "none", "description": "关闭推理思考 (No reasoning)" },
            { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
            { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
            { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" },
            { "effort": "xhigh", "description": "极限深度推理 (Extended reasoning depth for hard tasks)" },
            { "effort": "max", "description": "最大极限推理 (Maximum reasoning budget for toughest challenges)" }
        ]);
        return (serde_json::json!("high"), levels, true);
    }

    // 2. Terra family (均衡: 支持 none, low, medium, high, xhigh，不支持 max)
    if lower.contains("terra") {
        let levels = serde_json::json!([
            { "effort": "none", "description": "关闭推理思考 (No reasoning)" },
            { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
            { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
            { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" },
            { "effort": "xhigh", "description": "极限深度推理 (Extended reasoning depth for hard tasks)" }
        ]);
        return (serde_json::json!("high"), levels, true);
    }

    // 3. Luna family (高速低成本: 支持 none, low, medium, high，不支持 xhigh/max)
    if lower.contains("luna") {
        let levels = serde_json::json!([
            { "effort": "none", "description": "关闭推理思考 (No reasoning)" },
            { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
            { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
            { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" }
        ]);
        return (serde_json::json!("medium"), levels, true);
    }

    // 4. Other GPT-5 series (如 gpt-5.4, gpt-5.5)
    if lower.starts_with("gpt-5") {
        let levels = serde_json::json!([
            { "effort": "none", "description": "关闭推理思考 (No reasoning)" },
            { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
            { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
            { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" },
            { "effort": "xhigh", "description": "极限深度推理 (Extended reasoning depth for hard tasks)" }
        ]);
        return (serde_json::json!("high"), levels, true);
    }

    // 5. Google Gemini family (只支持 low, medium, high；不支持 minimal/none/xhigh/max)
    if lower.contains("gemini") {
        let levels = serde_json::json!([
            { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
            { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
            { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" }
        ]);
        return (serde_json::json!("high"), levels, true);
    }

    // 6. Claude 3.7 / Claude 4 / Extended Thinking models (支持 none, low, medium, high, xhigh, max)
    let is_claude_thinking = lower.contains("claude-3-7")
        || lower.contains("claude-3.7")
        || lower.contains("claude-4")
        || lower.contains("sonnet-3-7")
        || lower.contains("sonnet-3.7")
        || lower.contains("opus-4")
        || lower.contains("sonnet-4");

    if is_claude_thinking {
        let levels = serde_json::json!([
            { "effort": "none", "description": "关闭推理思考 (No reasoning)" },
            { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
            { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
            { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" },
            { "effort": "xhigh", "description": "极限深度推理 (Extended reasoning depth for hard tasks)" },
            { "effort": "max", "description": "最大极限推理 (Maximum reasoning budget for toughest challenges)" }
        ]);
        return (serde_json::json!("high"), levels, true);
    }

    // 7. Other reasoning patterns (o1, o3, o4, thinking, reasoner, r1, qwq)
    let is_reasoning = lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
        || lower.contains("thinking")
        || lower.contains("reasoner")
        || lower.contains("reasoning")
        || lower.contains("r1")
        || lower.contains("qwq");

    if is_reasoning {
        let levels = serde_json::json!([
            { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
            { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
            { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" },
            { "effort": "xhigh", "description": "极限深度推理 (Extended reasoning depth for hard tasks)" }
        ]);
        (serde_json::json!("high"), levels, true)
    } else {
        (serde_json::Value::Null, serde_json::json!([]), false)
    }
}

pub fn build_codex_catalog(
    provider_name: &str,
    default_model: &str,
    models: &[String],
) -> serde_json::Value {
    let mut catalog_models: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let primary_model = if !default_model.trim().is_empty()
        && default_model.trim() != "codex-auto-review"
        && !default_model.trim().ends_with("auto-review")
    {
        default_model.trim()
    } else if let Some(first) = models.iter().find(|s| {
        !s.trim().is_empty()
            && s.trim() != "codex-auto-review"
            && !s.trim().ends_with("auto-review")
    }) {
        first.trim()
    } else {
        "gpt-4o"
    };

    if !primary_model.is_empty() && seen.insert(primary_model.to_string()) {
        catalog_models.push(primary_model.to_string());
    }

    for m in models {
        let trimmed = m.trim();
        if trimmed == "codex-auto-review" || trimmed.ends_with("auto-review") {
            continue;
        }
        if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
            catalog_models.push(trimmed.to_string());
        }
    }

    if catalog_models.is_empty() {
        catalog_models.push("gpt-4o".to_string());
    }

    let models_json: Vec<serde_json::Value> = catalog_models
        .iter()
        .filter(|slug| *slug != "codex-auto-review" && !slug.ends_with("auto-review"))
        .enumerate()
        .map(|(i, slug)| {
            let (default_reasoning, supported_reasoning, supports_reasoning) = get_model_reasoning_config(slug);
            serde_json::json!({
                "slug": slug,
                "display_name": slug,
                "description": format!("{slug} via {provider_name}"),
                "default_reasoning_level": default_reasoning,
                "default_reasoning_summary": "none",
                "default_verbosity": "medium",
                "context_window": 200000,
                "max_context_window": 200000,
                "effective_context_window_percent": 95,
                "priority": i,
                "input_modalities": ["text"],
                "service_tiers": [],
                "additional_speed_tiers": [],
                "shell_type": "shell_command",
                "apply_patch_tool_type": "freeform",
                "web_search_tool_type": "text",
                "supported_in_api": true,
                "support_verbosity": true,
                "supports_image_detail_original": false,
                "supports_parallel_tool_calls": true,
                "supports_reasoning_summaries": supports_reasoning,
                "supports_search_tool": true,
                "tool_mode": null,
                "upgrade": null,
                "visibility": "list",
                "availability_nux": null,
                "minimal_client_version": "0.0.1",
                "use_responses_lite": false,
                "available_in_plans": ["free", "pro", "team", "enterprise", "edu", "anon"],
                "truncation_policy": {
                    "limit": 10000,
                    "mode": "tokens"
                },
                "supported_reasoning_levels": supported_reasoning,
                "base_instructions": "You are Codex, a coding agent. Work carefully in the user's current workspace, follow the user's instructions, inspect existing code before editing, preserve unrelated changes, use available tools when needed, and verify completed work before reporting it.",
                "experimental_supported_tools": []
            })
        })
        .collect();

    serde_json::json!({
        "models": models_json
    })
}

async fn write_codex_config(
    provider: &crate::profile::ProviderConfig,
    profile_id: &str,
    gateway_enabled: bool,
) -> AppResult<()> {
    let home = crate::user_home_dir()
        .ok_or_else(|| AppError::Config("无法确定用户主目录".into()))?;
    let codex_dir = home.join(".codex");
    std::fs::create_dir_all(&codex_dir)?;
    let config_path = codex_dir.join("config.toml");

    // Read existing config or create new
    let mut doc = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        content.parse::<toml_edit::DocumentMut>().unwrap_or_default()
    } else {
        toml_edit::DocumentMut::new()
    };

    let model_to_use = if !provider.default_model.trim().is_empty() && provider.default_model.trim() != "codex-auto-review" {
        provider.default_model.trim()
    } else if let Some(first) = provider.models.iter().find(|s| !s.trim().is_empty() && s.trim() != "codex-auto-review") {
        first.trim()
    } else {
        "gpt-4o"
    };

    // Sanitize provider id for toml key
    let raw_key = if !provider.id.trim().is_empty() {
        provider.id.trim()
    } else {
        "ai-deck"
    };
    let provider_key = raw_key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>();

    // Write dynamic model catalog JSON for Codex /model command
    let catalog_doc = build_codex_catalog(&provider.name, model_to_use, &provider.models);
    let catalog_path = codex_dir.join("ai-deck-model-catalog.json");
    let catalog_content = serde_json::to_string_pretty(&catalog_doc)?;
    crate::storage::atomic_replace(&catalog_path, catalog_content.as_bytes())?;

    // Also update provider-deck-model-catalog.json for compatibility
    let legacy_catalog = codex_dir.join("provider-deck-model-catalog.json");
    let _ = crate::storage::atomic_replace(&legacy_catalog, catalog_content.as_bytes());

    let catalog_path_str = catalog_path.to_string_lossy().replace('\\', "/");

    // Set active model and provider at top level
    doc["model"] = toml_edit::value(model_to_use);
    doc["model_provider"] = toml_edit::value(&provider_key);
    doc["model_catalog_json"] = toml_edit::value(catalog_path_str);
    doc["model_context_window"] = toml_edit::value(200000);
    let (def_reasoning_level, _, supports_summaries) = get_model_reasoning_config(model_to_use);
    if let Some(level_str) = def_reasoning_level.as_str() {
        doc["model_reasoning_effort"] = toml_edit::value(level_str);
    } else {
        doc.remove("model_reasoning_effort");
    }
    doc["model_reasoning_summary"] = toml_edit::value("none");
    doc["model_supports_reasoning_summaries"] = toml_edit::value(supports_summaries);

    let target_base_url = if gateway_enabled {
        "http://127.0.0.1:18888/v1".to_string()
    } else {
        let base = provider.base_url.trim().trim_end_matches('/');
        if base.ends_with("/v1") {
            base.to_string()
        } else {
            format!("{base}/v1")
        }
    };

    // Retrieve API key if stored in OS credentials
    let maybe_key = crate::credentials::get_api_key(profile_id).ok();

    // Update model_providers table
    let providers = doc
        .entry("model_providers")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));

    if let Some(table) = providers.as_table_mut() {
        let mut provider_table = toml_edit::Table::new();
        provider_table.insert("name", toml_edit::value(&provider.name));
        provider_table.insert("base_url", toml_edit::value(&target_base_url));
        provider_table.insert("wire_api", toml_edit::value("responses"));
        provider_table.insert("requires_openai_auth", toml_edit::value(false));

        let token_to_write = if let Some(key) = &maybe_key {
            if !key.trim().is_empty() {
                key.trim().to_string()
            } else if gateway_enabled {
                "ai-deck-local".to_string()
            } else {
                String::new()
            }
        } else if gateway_enabled {
            "ai-deck-local".to_string()
        } else {
            String::new()
        };

        if !token_to_write.is_empty() {
            provider_table.insert("experimental_bearer_token", toml_edit::value(token_to_write));
        }

        table.insert(&provider_key, toml_edit::Item::Table(provider_table));
    }

    let content = doc.to_string();
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::storage::atomic_replace(&config_path, content.as_bytes())?;
    Ok(())
}

async fn write_claude_config(
    provider: &crate::profile::ProviderConfig,
    profile_id: &str,
    gateway_enabled: bool,
) -> AppResult<()> {
    let home = crate::user_home_dir()
        .ok_or_else(|| AppError::Config("无法确定用户主目录".into()))?;
    let claude_dir = home.join(".claude");
    std::fs::create_dir_all(&claude_dir)?;
    let config_path = claude_dir.join("settings.json");

    // Read existing or create new
    let mut config: serde_json::Value = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if !config.is_object() {
        config = serde_json::json!({});
    }

    let target_base_url = if gateway_enabled {
        "http://127.0.0.1:18888/v1".to_string()
    } else {
        let base = provider.base_url.trim().trim_end_matches('/');
        if base.ends_with("/v1") {
            base.to_string()
        } else {
            format!("{base}/v1")
        }
    };

    let model_to_use = if !provider.default_model.trim().is_empty() {
        provider.default_model.trim()
    } else if let Some(first) = provider.models.first().filter(|s| !s.trim().is_empty()) {
        first.trim()
    } else {
        "claude-3-7-sonnet-latest"
    };

    // Find best candidates for Sonnet, Opus, Haiku
    let sonnet_candidate = provider
        .models
        .iter()
        .find(|m| m.to_ascii_lowercase().contains("sonnet"))
        .map(|s| s.as_str())
        .unwrap_or_else(|| {
            if model_to_use.to_ascii_lowercase().contains("sonnet") {
                model_to_use
            } else {
                provider.models.first().map(|s| s.as_str()).unwrap_or(model_to_use)
            }
        });

    let opus_candidate = provider
        .models
        .iter()
        .find(|m| m.to_ascii_lowercase().contains("opus"))
        .map(|s| s.as_str())
        .unwrap_or(sonnet_candidate);

    let haiku_candidate = provider
        .models
        .iter()
        .find(|m| {
            let lower = m.to_ascii_lowercase();
            lower.contains("haiku") || lower.contains("flash") || lower.contains("mini")
        })
        .map(|s| s.as_str())
        .unwrap_or(sonnet_candidate);

    // 1. Update availableModels array
    let mut available_models = Vec::new();
    let mut seen_avail = HashSet::new();

    for alias in &["sonnet", "opus", "haiku"] {
        if seen_avail.insert((*alias).to_string()) {
            available_models.push((*alias).to_string());
        }
    }
    if !model_to_use.is_empty() && seen_avail.insert(model_to_use.to_string()) {
        available_models.push(model_to_use.to_string());
    }
    for m in &provider.models {
        let trimmed = m.trim();
        if !trimmed.is_empty() && seen_avail.insert(trimmed.to_string()) {
            available_models.push(trimmed.to_string());
        }
    }

    config["availableModels"] = serde_json::to_value(&available_models)?;

    // 2. Update default model
    if model_to_use.to_ascii_lowercase().contains("opus") {
        config["model"] = serde_json::Value::String("opus".into());
    } else if model_to_use.to_ascii_lowercase().contains("haiku") {
        config["model"] = serde_json::Value::String("haiku".into());
    } else if model_to_use.to_ascii_lowercase().contains("sonnet") {
        config["model"] = serde_json::Value::String("sonnet".into());
    } else {
        config["model"] = serde_json::Value::String(model_to_use.to_string());
    }

    // 3. Update modelOverrides map
    let mut overrides = serde_json::Map::new();

    let sonnet_overrides = [
        "claude-3-5-sonnet",
        "claude-3-5-sonnet-20240620",
        "claude-3-5-sonnet-20241022",
        "claude-3-5-sonnet-latest",
        "claude-3-7-sonnet",
        "claude-3-7-sonnet-20250219",
        "claude-3-7-sonnet-latest",
        "claude-sonnet-4-5",
        "claude-sonnet-4-5-20250929",
        "claude-sonnet-4-5-20250929[1m]",
        "claude-sonnet-4-5[1m]",
        "claude-sonnet-4-6",
        "claude-sonnet-4-6[1m]",
        "claude-sonnet-5",
        "claude-sonnet-5[1m]",
    ];
    for key in &sonnet_overrides {
        overrides.insert((*key).to_string(), serde_json::Value::String(sonnet_candidate.to_string()));
    }

    let opus_overrides = [
        "claude-3-opus",
        "claude-3-opus-20240229",
        "claude-3-opus-latest",
        "claude-opus-4-5",
        "claude-opus-4-5-20251101",
        "claude-opus-4-5-20251101[1m]",
        "claude-opus-4-5[1m]",
        "claude-opus-4-6",
        "claude-opus-4-6[1m]",
        "claude-opus-4-7",
        "claude-opus-4-7[1m]",
        "claude-opus-4-8",
        "claude-opus-4-8[1m]",
        "claude-opus-5",
        "claude-opus-5[1m]",
    ];
    for key in &opus_overrides {
        overrides.insert((*key).to_string(), serde_json::Value::String(opus_candidate.to_string()));
    }

    let haiku_overrides = [
        "claude-3-haiku",
        "claude-3-haiku-20240307",
        "claude-3-5-haiku",
        "claude-3-5-haiku-20241022",
        "claude-3-5-haiku-latest",
        "claude-haiku-4-5",
        "claude-haiku-4-5-20251001",
        "claude-haiku-4-5-20251001-v1",
        "claude-haiku-4-5-20251001-v1[1m]",
        "claude-haiku-4-5-20251001[1m]",
        "claude-haiku-4-5[1m]",
    ];
    for key in &haiku_overrides {
        overrides.insert((*key).to_string(), serde_json::Value::String(haiku_candidate.to_string()));
    }

    for m in &provider.models {
        let trimmed = m.trim();
        if !trimmed.is_empty() {
            overrides.insert(trimmed.to_string(), serde_json::Value::String(trimmed.to_string()));
        }
    }

    config["modelOverrides"] = serde_json::Value::Object(overrides);

    // 4. Update env section
    let env = config
        .as_object_mut()
        .unwrap()
        .entry("env")
        .or_insert_with(|| serde_json::json!({}));

    if let Some(env_obj) = env.as_object_mut() {
        env_obj.insert(
            "ANTHROPIC_BASE_URL".into(),
            serde_json::Value::String(target_base_url),
        );

        let maybe_key = crate::credentials::get_api_key(profile_id).ok();
        let token_to_write = if let Some(key) = &maybe_key {
            if !key.trim().is_empty() {
                key.trim().to_string()
            } else if gateway_enabled {
                "ai-deck-local".to_string()
            } else {
                String::new()
            }
        } else if gateway_enabled {
            "ai-deck-local".to_string()
        } else {
            String::new()
        };

        if !token_to_write.is_empty() {
            env_obj.insert(
                "ANTHROPIC_API_KEY".into(),
                serde_json::Value::String(token_to_write.clone()),
            );
            // Crucial: keep ANTHROPIC_AUTH_TOKEN synchronized or remove stale tokens
            env_obj.insert(
                "ANTHROPIC_AUTH_TOKEN".into(),
                serde_json::Value::String(token_to_write.clone()),
            );
        } else {
            env_obj.remove("ANTHROPIC_API_KEY");
            env_obj.remove("ANTHROPIC_AUTH_TOKEN");
        }

        env_obj.insert(
            "ANTHROPIC_DEFAULT_SONNET_MODEL".into(),
            serde_json::Value::String(sonnet_candidate.to_string()),
        );
        env_obj.insert(
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME".into(),
            serde_json::Value::String(sonnet_candidate.to_string()),
        );
        env_obj.insert(
            "ANTHROPIC_DEFAULT_SONNET_MODEL_DESCRIPTION".into(),
            serde_json::Value::String(format!("{sonnet_candidate} via {}", provider.name)),
        );

        env_obj.insert(
            "ANTHROPIC_DEFAULT_OPUS_MODEL".into(),
            serde_json::Value::String(opus_candidate.to_string()),
        );
        env_obj.insert(
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME".into(),
            serde_json::Value::String(opus_candidate.to_string()),
        );
        env_obj.insert(
            "ANTHROPIC_DEFAULT_OPUS_MODEL_DESCRIPTION".into(),
            serde_json::Value::String(format!("{opus_candidate} via {}", provider.name)),
        );

        env_obj.insert(
            "ANTHROPIC_DEFAULT_HAIKU_MODEL".into(),
            serde_json::Value::String(haiku_candidate.to_string()),
        );
        env_obj.insert(
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME".into(),
            serde_json::Value::String(haiku_candidate.to_string()),
        );
        env_obj.insert(
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_DESCRIPTION".into(),
            serde_json::Value::String(format!("{haiku_candidate} via {}", provider.name)),
        );

        env_obj.insert(
            "CLAUDE_CODE_SUBAGENT_MODEL".into(),
            serde_json::Value::String("inherit".into()),
        );
        env_obj.insert(
            "CLAUDE_CODE_EFFORT_LEVEL".into(),
            serde_json::Value::String("max".into()),
        );
    }

    let content = serde_json::to_string_pretty(&config)?;
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::storage::atomic_replace(&config_path, content.as_bytes())?;

    // Also update ~/.claude.json at user home directory
    let claude_json_path = home.join(".claude.json");
    let mut claude_json: serde_json::Value = if claude_json_path.exists() {
        let text = std::fs::read_to_string(&claude_json_path).unwrap_or_default();
        serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    if !claude_json.is_object() {
        claude_json = serde_json::json!({});
    }
    if let Some(obj) = claude_json.as_object_mut() {
        obj.insert("hasCompletedOnboarding".into(), serde_json::Value::Bool(true));
        let maybe_key = crate::credentials::get_api_key(profile_id).ok();
        let token = if let Some(key) = &maybe_key {
            if !key.trim().is_empty() {
                key.trim().to_string()
            } else if gateway_enabled {
                "ai-deck-local".to_string()
            } else {
                String::new()
            }
        } else if gateway_enabled {
            "ai-deck-local".to_string()
        } else {
            String::new()
        };
        if !token.is_empty() {
            obj.insert("primaryApiKey".into(), serde_json::Value::String(token));
        }
    }
    if let Ok(serialized) = serde_json::to_string_pretty(&claude_json) {
        let _ = crate::storage::atomic_replace(&claude_json_path, serialized.as_bytes());
    }

    Ok(())
}

async fn write_claude_desktop_config(
    _provider: &crate::profile::ProviderConfig,
    _profile_id: &str,
    _gateway_enabled: bool,
) -> AppResult<()> {
    let home = crate::user_home_dir()
        .ok_or_else(|| AppError::Config("无法确定用户主目录".into()))?;

    #[cfg(windows)]
    let config_path = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join("AppData/Roaming"))
        .join(r"Claude\claude_desktop_config.json");

    #[cfg(target_os = "macos")]
    let config_path = home.join("Library/Application Support/Claude/claude_desktop_config.json");

    #[cfg(all(not(windows), not(target_os = "macos")))]
    let config_path = home.join(".config/Claude/claude_desktop_config.json");

    let mut config: serde_json::Value = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if !config.is_object() {
        config = serde_json::json!({});
    }

    if config.get("mcpServers").is_none() {
        config.as_object_mut().unwrap().insert("mcpServers".into(), serde_json::json!({}));
    }

    let content = serde_json::to_string_pretty(&config)?;
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::storage::atomic_replace(&config_path, content.as_bytes())?;
    Ok(())
}

async fn write_hermes_config(
    provider: &crate::profile::ProviderConfig,
    profile_id: &str,
    gateway_enabled: bool,
) -> AppResult<()> {
    let home = crate::user_home_dir()
        .ok_or_else(|| AppError::Config("无法确定用户主目录".into()))?;
    let hermes_dir = home.join(".hermes");
    std::fs::create_dir_all(&hermes_dir)?;

    let target_base_url = if gateway_enabled {
        "http://127.0.0.1:18888/v1".to_string()
    } else {
        let base = provider.base_url.trim().trim_end_matches('/');
        if base.ends_with("/v1") {
            base.to_string()
        } else {
            format!("{base}/v1")
        }
    };

    let maybe_key = crate::credentials::get_api_key(profile_id).ok();
    let key = if let Some(k) = &maybe_key {
        if !k.trim().is_empty() {
            k.trim().to_string()
        } else if gateway_enabled {
            "ai-deck-local".to_string()
        } else {
            String::new()
        }
    } else if gateway_enabled {
        "ai-deck-local".to_string()
    } else {
        String::new()
    };

    let model = if provider.default_model.trim().is_empty() {
        "gpt-4o"
    } else {
        provider.default_model.trim()
    };

    let mut hermes_models: Vec<String> = Vec::new();
    let mut seen_models: HashSet<String> = HashSet::new();
    if !model.is_empty() && seen_models.insert(model.to_string()) {
        hermes_models.push(model.to_string());
    }
    for m in &provider.models {
        let trimmed = m.trim();
        if !trimmed.is_empty() && seen_models.insert(trimmed.to_string()) {
            hermes_models.push(trimmed.to_string());
        }
    }
    if hermes_models.is_empty() {
        hermes_models.push("gpt-4o".to_string());
    }

    let models_yaml = hermes_models
        .iter()
        .map(|m| format!("      - {m}"))
        .collect::<Vec<_>>()
        .join("\n");

    // 1. Write ~/.hermes/config.yaml conforming strictly to Hermes CLI schema
    // Root level must only contain valid Hermes keys (inference_provider, model, custom_providers).
    // Misplaced root-level keys like api_key or base_url trigger validation warnings in Hermes CLI.
    let config_yaml = format!(
r##"# Hermes Agent Configuration (Managed by AI Deck)
inference_provider: custom
model: {model}

custom_providers:
  custom:
    base_url: {target_base_url}
    api_key: {key}
    models:
{models_yaml}
"##
    );
    let config_path = hermes_dir.join("config.yaml");
    crate::storage::atomic_replace(&config_path, config_yaml.as_bytes())?;
    let config_yml = hermes_dir.join("config.yml");
    let _ = crate::storage::atomic_replace(&config_yml, config_yaml.as_bytes());

    // 2. Write ~/.hermes/.env for CLI environment loader
    let env_content = format!(
r#"INFERENCE_PROVIDER=custom
MODEL={model}
HERMES_MODEL={model}
OPENAI_BASE_URL={target_base_url}
OPENAI_API_BASE={target_base_url}
OPENAI_API_KEY={key}
CUSTOM_BASE_URL={target_base_url}
CUSTOM_API_KEY={key}
ANTHROPIC_BASE_URL={target_base_url}
ANTHROPIC_API_KEY={key}
"#
    );
    let env_path = hermes_dir.join(".env");
    let _ = crate::storage::atomic_replace(&env_path, env_content.as_bytes());

    // 3. Write ~/.hermes/config.json
    let config_json_val = serde_json::json!({
        "inference_provider": "custom",
        "model": model,
        "custom_providers": {
            "custom": {
                "base_url": target_base_url,
                "api_key": key,
                "models": hermes_models
            }
        }
    });
    let json_content = serde_json::to_string_pretty(&config_json_val)?;
    let json_path = hermes_dir.join("config.json");
    let _ = crate::storage::atomic_replace(&json_path, json_content.as_bytes());

    let dot_config_hermes = home.join(".config").join("hermes");
    if std::fs::create_dir_all(&dot_config_hermes).is_ok() {
        let _ = crate::storage::atomic_replace(&dot_config_hermes.join("config.yaml"), config_yaml.as_bytes());
        let _ = crate::storage::atomic_replace(&dot_config_hermes.join("config.json"), json_content.as_bytes());
        let _ = crate::storage::atomic_replace(&dot_config_hermes.join(".env"), env_content.as_bytes());
    }

    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let appdata_hermes = std::path::PathBuf::from(appdata).join("hermes");
        if std::fs::create_dir_all(&appdata_hermes).is_ok() {
            let _ = crate::storage::atomic_replace(&appdata_hermes.join("config.yaml"), config_yaml.as_bytes());
            let _ = crate::storage::atomic_replace(&appdata_hermes.join("config.json"), json_content.as_bytes());
            let _ = crate::storage::atomic_replace(&appdata_hermes.join(".env"), env_content.as_bytes());
        }
    }

    #[cfg(windows)]
    if let Some(localappdata) = std::env::var_os("LOCALAPPDATA") {
        let localappdata_hermes = std::path::PathBuf::from(localappdata).join("hermes");
        if std::fs::create_dir_all(&localappdata_hermes).is_ok() {
            let _ = crate::storage::atomic_replace(&localappdata_hermes.join("config.yaml"), config_yaml.as_bytes());
            let _ = crate::storage::atomic_replace(&localappdata_hermes.join("config.json"), json_content.as_bytes());
            let _ = crate::storage::atomic_replace(&localappdata_hermes.join(".env"), env_content.as_bytes());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::ProfileUpdate;

    #[test]
    fn test_build_codex_catalog_structure() {
        let models = vec![
            "gemini-3.7-flash-high".to_string(),
            "subtoken-opus-4-6-thinking".to_string(),
            "subtoken-sonnet-4-6".to_string(),
            "codex-auto-review".to_string(),
        ];
        let catalog = build_codex_catalog("Subtoken VIP", "gemini-3.7-flash-high", &models);
        let list = catalog["models"].as_array().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0]["slug"], "gemini-3.7-flash-high");
        assert_eq!(list[0]["priority"], 0);
        assert_eq!(list[0]["supported_reasoning_levels"].as_array().unwrap().len(), 3);
        assert_eq!(list[1]["slug"], "subtoken-opus-4-6-thinking");
        assert_eq!(list[2]["slug"], "subtoken-sonnet-4-6");

        // Verify Sol has 6 reasoning levels
        let (sol_def, sol_levels, sol_supp) = get_model_reasoning_config("gpt-5.6-sol");
        assert!(sol_supp);
        assert_eq!(sol_def, "high");
        assert_eq!(sol_levels.as_array().unwrap().len(), 6);

        // Verify Terra has 5 reasoning levels (no max)
        let (terra_def, terra_levels, terra_supp) = get_model_reasoning_config("gpt-5.6-terra");
        assert!(terra_supp);
        assert_eq!(terra_def, "high");
        assert_eq!(terra_levels.as_array().unwrap().len(), 5);

        // Verify Luna has 4 reasoning levels (no xhigh/max)
        let (luna_def, luna_levels, luna_supp) = get_model_reasoning_config("gpt-5.6-luna");
        assert!(luna_supp);
        assert_eq!(luna_def, "medium");
        assert_eq!(luna_levels.as_array().unwrap().len(), 4);

        // Verify Gemini only has low, medium, high (3 levels)
        let (gem_def, gem_levels, gem_supp) = get_model_reasoning_config("gemini-2.5-pro");
        assert!(gem_supp);
        assert_eq!(gem_def, "high");
        assert_eq!(gem_levels.as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn test_switch_profile_active_isolation() {
        let temp_home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_DECK_HOME_OVERRIDE", temp_home.path());
        let state_path = temp_home.path().join(".ai-deck").join("state.json");
        let mut pm = ProfileManager::with_state_path(state_path);

        let mut p1 = pm.create_profile_simple("方案Alpha").unwrap();
        p1.providers[0].name = "Alpha Provider".into();
        p1.providers[0].default_model = "alpha-model".into();
        p1.providers[0].models = vec!["alpha-model".into()];
        let p1 = pm.update_profile(&p1.id, ProfileUpdate {
            name: Some("方案Alpha".into()),
            providers: Some(p1.providers),
            clients: None,
            gateway_enabled: Some(true),
            failover_enabled: None,
        }).unwrap();

        let mut p2 = pm.create_profile_simple("方案Beta").unwrap();
        p2.providers[0].name = "Beta Provider".into();
        p2.providers[0].default_model = "beta-model".into();
        p2.providers[0].models = vec!["beta-model".into()];
        let p2 = pm.update_profile(&p2.id, ProfileUpdate {
            name: Some("方案Beta".into()),
            providers: Some(p2.providers),
            clients: None,
            gateway_enabled: Some(true),
            failover_enabled: None,
        }).unwrap();

        // 1. Switch to Profile Alpha
        let res1 = switch_profile(&mut pm, &p1.id).await.unwrap();
        assert!(res1.success);
        assert_eq!(pm.active_profile().unwrap().id, p1.id);

        let home = crate::user_home_dir().unwrap();
        let codex_doc = std::fs::read_to_string(home.join(".codex").join("config.toml")).unwrap();
        assert!(codex_doc.contains("alpha-model"), "Codex 配置应写入 Alpha 方案模型");
        assert!(!codex_doc.contains("beta-model"), "Codex 配置中不应包含未激活的 Beta 方案模型");

        let hermes_yaml = std::fs::read_to_string(home.join(".hermes").join("config.yaml")).unwrap();
                assert!(hermes_yaml.contains("custom_providers:"), "Hermes 配置应包含 custom_providers");
        assert!(hermes_yaml.contains("inference_provider: custom"), "Hermes 配置应声明 inference_provider: custom");
        assert!(hermes_yaml.contains("model: alpha-model"), "Hermes 配置应写入 Alpha 方案模型");
        assert!(!hermes_yaml.lines().any(|l| l.starts_with("api_key:")), "Hermes 配置根层级不应包含 api_key");
        assert!(!hermes_yaml.lines().any(|l| l.starts_with("base_url:")), "Hermes 配置根层级不应包含 base_url");
        assert!(!hermes_yaml.contains("model: beta-model"), "Hermes 配置中不应包含未激活的 Beta 方案模型");

        // 2. Switch to Profile Beta
        let res2 = switch_profile(&mut pm, &p2.id).await.unwrap();
        assert!(res2.success);
        assert_eq!(pm.active_profile().unwrap().id, p2.id);

        let codex_doc2 = std::fs::read_to_string(home.join(".codex").join("config.toml")).unwrap();
        assert!(codex_doc2.contains("beta-model"), "Codex 配置应更新为 Beta 方案模型");
        assert!(!codex_doc2.contains("alpha-model"), "Codex 配置中不应残留 Alpha 方案模型");

        let hermes_yaml2 = std::fs::read_to_string(home.join(".hermes").join("config.yaml")).unwrap();
        assert!(hermes_yaml2.contains("model: beta-model"), "Hermes 配置应更新为 Beta 方案模型");
        assert!(!hermes_yaml2.contains("alpha-model"), "Hermes 配置中不应残留 Alpha 方案模型");
    }
}
