//! Profile switching — atomically writes all client configs when switching profiles.
//!
//! If any step fails, the entire switch is rolled back.

use crate::error::{AppError, AppResult};
use crate::profile::{Profile, ProfileManager};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use ts_rs::TS;

pub const CLAUDE_CODE_SONNET_ALIASES: &[&str] = &["sonnet", "default", "claude-sonnet"];
pub const CLAUDE_CODE_OPUS_ALIASES: &[&str] = &["opus", "opusplan", "claude-opus"];
pub const CLAUDE_CODE_HAIKU_ALIASES: &[&str] = &["haiku", "claude-haiku"];

// Names Claude Code is shown for each tier when the provider does not override
// them. They have to be current built-in Anthropic IDs: Claude Code decides a
// model's context window, price and feature set from the name, and falls back to
// a 200K unknown-model profile for anything it does not recognise.
//
// Bump these when Anthropic ships a new generation. Fable and Mythos are absent
// on purpose — Claude Code has no alias tier for them.
pub const DEFAULT_OPUS_DISPLAY_NAME: &str = "claude-opus-5";
pub const DEFAULT_SONNET_DISPLAY_NAME: &str = "claude-sonnet-5";
/// Haiku 4.5 caps at 200K, so this tier has no `[1m]` form.
pub const DEFAULT_HAIKU_DISPLAY_NAME: &str = "claude-haiku-4-5";

/// Loopback port the built-in gateway listens on.
const GATEWAY_PORT: u16 = 18888;

/// Point every `keys` entry at `wire` in a `modelOverrides` map.
///
/// A `[1m]` key keeps its suffix only when `tier_supports_1m`; otherwise it
/// collapses onto the plain wire name, since asking for a context window the
/// upstream cannot serve fails the request outright.
fn insert_tier(
    overrides: &mut serde_json::Map<String, serde_json::Value>,
    keys: &[&str],
    wire: &str,
    tier_supports_1m: bool,
) {
    let wire_1m = format!("{wire}[1m]");
    for key in keys {
        let value = if key.ends_with("[1m]") && tier_supports_1m && !wire.ends_with("[1m]") {
            wire_1m.as_str()
        } else {
            wire
        };
        overrides.insert(
            (*key).to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
}

/// Trimmed `value`, or `fallback` when it is absent or blank.
fn trimmed_or<'a>(value: Option<&'a str>, fallback: &'a str) -> &'a str {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
}

/// The `(opus, sonnet, haiku)` names Claude Code is shown for this provider.
///
/// The gateway has to resolve the same names this module writes into
/// `~/.claude.json`, so both sides read them from here.
pub fn claude_display_names(provider: &crate::profile::ProviderConfig) -> (&str, &str, &str) {
    (
        trimmed_or(
            provider.opus_display_name.as_deref(),
            DEFAULT_OPUS_DISPLAY_NAME,
        ),
        trimmed_or(
            provider.sonnet_display_name.as_deref(),
            DEFAULT_SONNET_DISPLAY_NAME,
        ),
        trimmed_or(
            provider.haiku_display_name.as_deref(),
            DEFAULT_HAIKU_DISPLAY_NAME,
        ),
    )
}

/// The model a client should default to for this provider.
///
/// Shared so the tier resolution below and `write_claude_config` cannot drift
/// apart on which model counts as the default.
fn claude_default_model(provider: &crate::profile::ProviderConfig) -> &str {
    if !provider.default_model.trim().is_empty() {
        provider.default_model.trim()
    } else if let Some(first) = provider.models.first().filter(|s| !s.trim().is_empty()) {
        first.trim()
    } else {
        // Last-resort fallback: use the generic "sonnet" alias rather than a
        // pinned retired ID like claude-3-7-sonnet-latest, which would make
        // Claude Code show a retirement banner.
        "sonnet"
    }
}

/// Pick the model that best serves a tier among those matching it.
///
/// Relays decorate names freely (`claude-opus-5-A`, `Claude-5-opus-preview`,
/// `Claude-Opus-5-thinking`), so a catalog often matches one tier several times.
/// Taking the first match made the answer depend on catalog order, which then
/// decided whether the tier could keep its canonical display name — the same two
/// models in the other order produced a different picker label.
///
/// So prefer, in order: the tier's canonical name exactly, then the least
/// decorated match (shortest, ties by catalog order). The plain `claude-opus-5`
/// wins over `claude-opus-5-A` however the relay lists them, and a decorated
/// name is still picked when it is all there is.
fn pick_tier_model<'a>(
    models: &'a [String],
    canonical: &str,
    matches_tier: impl Fn(&str) -> bool,
) -> Option<&'a str> {
    let candidates: Vec<&'a str> = models
        .iter()
        .map(|m| m.trim())
        .filter(|m| !m.is_empty() && matches_tier(&m.to_ascii_lowercase()))
        .collect();

    candidates
        .iter()
        .find(|m| m.eq_ignore_ascii_case(canonical))
        .or_else(|| candidates.iter().min_by_key(|m| m.len()))
        .copied()
}

/// The `(opus, sonnet, haiku)` provider models that actually serve each Claude
/// Code tier: the explicit `*_model` override when set, else a name-based guess.
///
/// The guess is keyword-based and case-insensitive, so irregular relay spellings
/// (`Claude-5-opus`, `claude-opus-5-A`, `anthropic/claude-opus-5`) still land on
/// the right tier. A catalog whose names carry no tier word at all cannot be
/// guessed — every tier then falls back to one model, and the `*_model` fields
/// are the only way to spread the tiers out.
pub fn claude_tier_candidates(provider: &crate::profile::ProviderConfig) -> (&str, &str, &str) {
    let model_to_use = claude_default_model(provider);

    let sonnet = provider
        .sonnet_model
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            pick_tier_model(&provider.models, DEFAULT_SONNET_DISPLAY_NAME, |lower| {
                lower.contains("sonnet")
                    || lower.contains("claude-3-7")
                    || lower.contains("claude-3.7")
            })
            .unwrap_or_else(|| {
                if model_to_use.to_ascii_lowercase().contains("sonnet") {
                    model_to_use
                } else {
                    provider
                        .models
                        .first()
                        .map(|s| s.as_str())
                        .unwrap_or(model_to_use)
                }
            })
        });

    let opus = provider
        .opus_model
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            pick_tier_model(&provider.models, DEFAULT_OPUS_DISPLAY_NAME, |lower| {
                lower.contains("opus")
            })
            .unwrap_or(sonnet)
        });

    let haiku = provider
        .haiku_model
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            pick_tier_model(&provider.models, DEFAULT_HAIKU_DISPLAY_NAME, |lower| {
                lower.contains("haiku") || lower.contains("flash") || lower.contains("mini")
            })
            .unwrap_or(sonnet)
        });

    (opus, sonnet, haiku)
}

/// The `(opus, sonnet, haiku)` names that travel on the wire when the gateway is
/// in front, which are also the names Claude Code shows.
///
/// Normally this is the display name: it has to be a built-in Anthropic ID or
/// Claude Code cannot size or price the model, and the gateway maps it back to
/// the provider's real model.
///
/// A display name that collides with a *different* provider model is the one
/// case where that breaks down. Redirecting it would make the collided-with
/// model unreachable — nobody could address the real `claude-opus-5` once it
/// resolves to `claude-opus-5-max` — so this tier carries its upstream name
/// instead. That name is one the provider serves, so the gateway's own
/// passthrough rule routes it, the picker label matches what runs, and every
/// provider model stays independently addressable.
///
/// The rule is structural, not tied to any naming convention: it asks only
/// whether the shown name would shadow a different model, so `-max`, `-ultra`,
/// `:max` and names with no tier word behave the same way.
///
/// Note this also overrides an *explicitly configured* display name when that
/// name collides. Keeping it would strand the model it shadows, and a display
/// name pointing at another of the provider's own models is a misconfiguration
/// either way — but it does mean the setting is silently not honored.
///
/// The gateway has to resolve the same names this module writes, so both sides
/// read them from here.
pub fn claude_wire_names<'a>(
    provider: &'a crate::profile::ProviderConfig,
) -> (&'a str, &'a str, &'a str) {
    let (opus_display, sonnet_display, haiku_display) = claude_display_names(provider);
    let (opus_candidate, sonnet_candidate, haiku_candidate) = claude_tier_candidates(provider);
    let served: HashSet<&str> = provider.models.iter().map(|m| m.trim()).collect();

    let resolve = |display: &'a str, candidate: &'a str| -> &'a str {
        if display != candidate && served.contains(display) {
            candidate
        } else {
            display
        }
    };

    (
        resolve(opus_display, opus_candidate),
        resolve(sonnet_display, sonnet_candidate),
        resolve(haiku_display, haiku_candidate),
    )
}

/// Warnings worth surfacing when a profile is activated, about how its tier
/// wiring actually resolves.
///
/// Two silent failure modes get a message here:
/// - Two tiers landing on one provider model, at least one of them guessed.
///   A catalog with no tier words in its names hands every tier the same
///   fallback, and nothing in the UI says the tiers are not actually spread
///   out. Both explicitly pinned to one model is the user's own choice and
///   stays quiet.
/// - An explicit `*_display_name` that `claude_wire_names` had to override
///   because it collided with a different provider model. The setting is
///   silently not honored otherwise.
pub fn claude_tier_warnings(provider: &crate::profile::ProviderConfig) -> Vec<String> {
    let (opus, sonnet, haiku) = claude_tier_candidates(provider);
    let explicitly_set = |v: &Option<String>| {
        v.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some()
    };

    let mut warnings = Vec::new();

    // Collapsed tiers, one message per model they share.
    let mut by_model: Vec<(&str, Vec<(&str, bool)>)> = Vec::new();
    for (label, model, is_explicit) in [
        ("Opus", opus, explicitly_set(&provider.opus_model)),
        ("Sonnet", sonnet, explicitly_set(&provider.sonnet_model)),
        ("Haiku", haiku, explicitly_set(&provider.haiku_model)),
    ] {
        match by_model.iter_mut().find(|(m, _)| *m == model) {
            Some((_, labels)) => labels.push((label, is_explicit)),
            None => by_model.push((model, vec![(label, is_explicit)])),
        }
    }
    for (model, labels) in by_model {
        if labels.len() > 1 && labels.iter().any(|(_, is_explicit)| !is_explicit) {
            let names = labels
                .iter()
                .map(|(label, _)| *label)
                .collect::<Vec<_>>()
                .join("、");
            warnings.push(format!(
                "档位 {names} 都解析到模型 {model}，分档未生效；若目录里没有能区分档位的模型名，请在配置中分别指定 opus_model / sonnet_model / haiku_model"
            ));
        }
    }

    // Explicit display names the wire path had to give up because they collided
    // with another provider model.
    let (opus_display, sonnet_display, haiku_display) = claude_display_names(provider);
    let (opus_wire, sonnet_wire, haiku_wire) = claude_wire_names(provider);
    for (label, display, wire, is_explicit) in [
        (
            "Opus",
            opus_display,
            opus_wire,
            explicitly_set(&provider.opus_display_name),
        ),
        (
            "Sonnet",
            sonnet_display,
            sonnet_wire,
            explicitly_set(&provider.sonnet_display_name),
        ),
        (
            "Haiku",
            haiku_display,
            haiku_wire,
            explicitly_set(&provider.haiku_display_name),
        ),
    ] {
        if is_explicit && display != wire {
            warnings.push(format!(
                "{label} 档位的显示名 {display} 与提供方另一个模型重名，为避免它不可寻址，已改用上游名 {wire} 展示"
            ));
        }
    }

    warnings
}

/// Strip a trailing `/v1` (and any trailing slashes) from an Anthropic base URL.
///
/// Claude Code talks to Anthropic-shaped endpoints through the official SDK,
/// which appends the `/v1/...` path segment on its own. Keeping a `/v1` suffix
/// in `ANTHROPIC_BASE_URL` makes it request `/v1/v1/messages`, which 404s.
fn strip_anthropic_version_suffix(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    match trimmed.strip_suffix("/v1") {
        Some(stripped) => stripped.trim_end_matches('/').to_string(),
        None => trimmed.to_string(),
    }
}

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
        // Tier wiring is decided per-provider before any config lands on disk;
        // surface collapses and overridden display names early so a profile that
        // looks set up is not silently not set up.
        let primary = profile
            .providers
            .iter()
            .find(|p| p.is_primary)
            .or_else(|| profile.providers.first());
        if let Some(primary) = primary {
            warnings.extend(claude_tier_warnings(primary));
        }

        let mut target_set = HashSet::new();
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

        // Writing the endpoint puts Desktop into third-party mode, which takes it
        // off the user's Claude account until a profile without it is activated.
        // Neither fact is visible from the app, and the config is only read at
        // startup.
        if clients_written.iter().any(|c| c.contains("desktop")) {
            warnings.push(
                "Claude Desktop 已切到第三方模式，需要重启它才生效；期间它不再走你的 Claude 账号登录。切到未勾选它的方案会自动恢复官方登录".into(),
            );
        }
    }

    // Hand Desktop back to its own account login when this profile does not claim
    // it. Keyed on the profile's own client list rather than `clients_written`, so
    // a failed write does not then tear down a setup that was working. Sits
    // outside the providers check above because a provider-less profile still has
    // to release Desktop.
    let targets_desktop = profile.clients.iter().any(|client| {
        let clean = client.trim().to_ascii_lowercase();
        clean == "claude-desktop" || clean.contains("desktop")
    });
    if !targets_desktop {
        if let Err(e) = crate::claude_desktop::restore() {
            warnings.push(format!("恢复 Claude Desktop 官方登录时提示：{e}"));
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

/// The conservative effort ladder handed to models no rule recognises.
///
/// An empty `supported_reasoning_levels` leaves Codex's effort submenu with
/// nothing to select, and the menu then cannot be committed or dismissed except
/// with Esc. Third-party relay catalogs are full of names no pattern here
/// matches (`deepseek-v4-pro-0813`, `qwen3.8-max`), so they get low/medium/high
/// — the subset essentially every reasoning-capable upstream accepts.
fn fallback_reasoning_levels() -> serde_json::Value {
    serde_json::json!([
        { "effort": "low", "description": "快速轻度推理 (Fast responses with lighter reasoning)" },
        { "effort": "medium", "description": "平衡推理模式 (Balances speed and reasoning depth)" },
        { "effort": "high", "description": "深度复杂推理 (Greater reasoning depth for complex problems)" }
    ])
}

/// A model's documented token limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModelWindow {
    /// Total window, input plus output.
    context: u64,
    /// Ceiling on a single response.
    max_output: u64,
}

impl ModelWindow {
    /// The whole-session window to hand Claude Code.
    ///
    /// Auto-compact fires in the low-90s percent of whatever number it is given,
    /// so passing the full context leaves less than one max-length response
    /// between the compaction point and the upstream's hard ceiling. Reserving
    /// the output budget moves compaction early enough that a full-length reply
    /// still fits.
    fn claude_budget(self) -> u64 {
        self.context.saturating_sub(self.max_output)
    }
}

/// A model's published limits, or `None` when no rule here covers the name.
///
/// Only names with a documented figure belong here. A guess would be worse than
/// the fallback: it travels into `CLAUDE_CODE_MAX_CONTEXT_TOKENS`, and one that
/// reads high lets a session run past what the upstream accepts.
fn model_window(slug: &str) -> Option<ModelWindow> {
    let lower = slug.to_ascii_lowercase();

    // Agnes AI. The two families differ, so `supports_1m_context` cannot express
    // this catalogue with one flag: the Pro pair is 1M, the Flash pair 512K.
    if lower.starts_with("agnes-") {
        if lower.contains("-pro") {
            return Some(ModelWindow {
                context: 1_000_000,
                max_output: 65_536,
            });
        }
        if lower.contains("-flash") {
            return Some(ModelWindow {
                context: 512_000,
                max_output: 65_536,
            });
        }
    }

    None
}

/// The context window to advertise to Codex for `slug`.
///
/// Codex applies its own `effective_context_window_percent`, so it gets the full
/// context rather than the reserved budget Claude Code needs.
fn codex_context_window(slug: &str, supports_1m: bool) -> i64 {
    let fallback = if supports_1m { 1_000_000 } else { 200_000 };
    model_window(slug).map(|w| w.context).unwrap_or(fallback) as i64
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
        // Still hand Codex one selectable entry: `supported_reasoning_levels: []`
        // leaves its effort submenu empty and the menu stops responding to
        // anything but Esc. `none` is the honest level for these models.
        let levels = serde_json::json!([
            { "effort": "none", "description": "关闭推理思考 (No reasoning)" }
        ]);
        return (serde_json::json!("none"), levels, false);
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

    // 6. Claude 3.7+ / Claude 4+ / Claude 5+ / Opus / Sonnet / Extended Thinking models (支持 none, low, medium, high, xhigh, max)
    let is_claude_thinking = lower.contains("claude-3-7")
        || lower.contains("claude-3.7")
        || lower.contains("claude-4")
        || lower.contains("claude-5")
        || lower.contains("claude-opus")
        || lower.contains("claude-sonnet")
        || lower.contains("sonnet-3-7")
        || lower.contains("sonnet-3.7")
        || lower.contains("sonnet-4")
        || lower.contains("sonnet-5")
        || lower.contains("opus-4")
        || lower.contains("opus-5")
        || lower.contains("model-s")
        || lower.contains("model-o");

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
        // Unrecognised name: assume a reasoning model with the safe three-level
        // ladder rather than declaring no levels, which hangs Codex's menu.
        (
            serde_json::json!("medium"),
            fallback_reasoning_levels(),
            true,
        )
    }
}

pub fn build_codex_catalog(
    provider_name: &str,
    default_model: &str,
    models: &[String],
) -> serde_json::Value {
    build_codex_catalog_with_1m(provider_name, default_model, models, false)
}

pub fn build_codex_catalog_with_1m(
    provider_name: &str,
    default_model: &str,
    models: &[String],
    supports_1m: bool,
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
            let context_window = codex_context_window(slug, supports_1m);
            let (default_reasoning, supported_reasoning, supports_reasoning) = get_model_reasoning_config(slug);
            serde_json::json!({
                "slug": slug,
                "display_name": slug,
                "description": format!("{slug} via {provider_name}"),
                "default_reasoning_level": default_reasoning,
                "default_reasoning_summary": "none",
                "default_verbosity": "medium",
                "context_window": context_window,
                "max_context_window": context_window,
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
    let home =
        crate::user_home_dir().ok_or_else(|| AppError::Config("无法确定用户主目录".into()))?;
    let codex_dir = home.join(".codex");
    std::fs::create_dir_all(&codex_dir)?;
    let config_path = codex_dir.join("config.toml");

    // Read existing config or create new
    let mut doc = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        content
            .parse::<toml_edit::DocumentMut>()
            .unwrap_or_default()
    } else {
        toml_edit::DocumentMut::new()
    };

    let model_to_use = if !provider.default_model.trim().is_empty()
        && provider.default_model.trim() != "codex-auto-review"
    {
        provider.default_model.trim()
    } else if let Some(first) = provider
        .models
        .iter()
        .find(|s| !s.trim().is_empty() && s.trim() != "codex-auto-review")
    {
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
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();

    // Write dynamic model catalog JSON for Codex /model command
    let supports_1m = provider.supports_1m_context.unwrap_or(false);
    let catalog_doc =
        build_codex_catalog_with_1m(&provider.name, model_to_use, &provider.models, supports_1m);
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
    doc["model_context_window"] = toml_edit::value(codex_context_window(model_to_use, supports_1m));
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
            provider_table.insert(
                "experimental_bearer_token",
                toml_edit::value(token_to_write),
            );
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
    let home =
        crate::user_home_dir().ok_or_else(|| AppError::Config("无法确定用户主目录".into()))?;
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

    // Claude Code uses the Anthropic SDK, which appends "/v1/messages" to
    // ANTHROPIC_BASE_URL itself. A trailing "/v1" here would produce
    // "/v1/v1/messages" -> 404, which Claude Code surfaces as the misleading
    // "issue with the selected model" error, so the base URL must stay bare.
    let target_base_url = if gateway_enabled {
        format!("http://127.0.0.1:{GATEWAY_PORT}")
    } else {
        strip_anthropic_version_suffix(&provider.base_url)
    };

    let model_to_use = claude_default_model(provider);

    // Which provider model serves each tier (explicit override, else guessed).
    let (opus_candidate, sonnet_candidate, haiku_candidate) = claude_tier_candidates(provider);

    let supports_1m = provider.supports_1m_context.unwrap_or(false);

    // The name that travels on the wire for each tier, which is also the name
    // Claude Code shows. Only the gateway can translate a display name back to
    // the provider's real model, so without it the wire has to carry the
    // upstream names and no display name is possible.
    let (sonnet_wire, opus_wire, haiku_wire) = if gateway_enabled {
        let (opus, sonnet, haiku) = claude_wire_names(provider);
        (sonnet, opus, haiku)
    } else {
        (sonnet_candidate, opus_candidate, haiku_candidate)
    };

    // 1. Update availableModels array
    let mut available_models = Vec::new();
    let mut seen_avail = HashSet::new();
    let mut push_avail = |value: &str| {
        if !value.is_empty() && seen_avail.insert(value.to_string()) {
            available_models.push(value.to_string());
        }
    };

    // Display names lead the picker; they are the entries Claude Code can size
    // and price correctly.
    for wire in [opus_wire, sonnet_wire, haiku_wire] {
        push_avail(wire);
    }
    // Haiku 4.5 has no 1M form, so only the two larger tiers get a `[1m]` entry.
    if supports_1m {
        for wire in [opus_wire, sonnet_wire] {
            if !wire.ends_with("[1m]") {
                push_avail(&format!("{wire}[1m]"));
            }
        }
    }
    // Bare aliases stay available: they are what `--model opus` and subagent
    // frontmatter pass, and some users type them from habit.
    for alias in ["opus", "sonnet", "haiku"] {
        push_avail(alias);
    }
    push_avail(model_to_use);
    for m in &provider.models {
        push_avail(m.trim());
    }

    config["availableModels"] = serde_json::to_value(&available_models)?;

    // 2. Update default model
    let lower_default = model_to_use.to_ascii_lowercase();
    let default_wire = if lower_default.contains("opus") {
        opus_wire
    } else if lower_default.contains("haiku") {
        haiku_wire
    } else if lower_default.contains("sonnet") {
        sonnet_wire
    } else {
        // A provider-specific name with no tier word: pass it through as-is
        // rather than guessing a tier and silently changing which model runs.
        model_to_use
    };
    config["model"] = serde_json::Value::String(default_wire.to_string());

    // 3. Update modelOverrides map
    //
    // Every key resolves to its tier's wire name, so whichever Claude Code name
    // a user or subagent asks for, one recognised name reaches the gateway.
    //
    // Retired model IDs (claude-3-opus*, claude-3-7-sonnet*, claude-3-5-haiku*)
    // are deliberately omitted: Claude Code prints a "was retired on ..." banner
    // for any key it finds here, even though the gateway already rewrites those
    // names upstream. See gateway::model_rewrite for the actual remapping.
    let mut overrides = serde_json::Map::new();

    let sonnet_overrides = [
        "claude-3-5-sonnet",
        "claude-3-5-sonnet-20240620",
        "claude-3-5-sonnet-20241022",
        "claude-3-5-sonnet-latest",
        "claude-sonnet-4-5",
        "claude-sonnet-4-5-20250929",
        "claude-sonnet-4-5-20250929[1m]",
        "claude-sonnet-4-5[1m]",
        "claude-sonnet-4-6",
        "claude-sonnet-4-6[1m]",
        "claude-sonnet-5",
        "claude-sonnet-5[1m]",
    ];
    insert_tier(&mut overrides, &sonnet_overrides, sonnet_wire, supports_1m);

    let opus_overrides = [
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
    insert_tier(&mut overrides, &opus_overrides, opus_wire, supports_1m);

    let haiku_overrides = [
        "claude-3-haiku",
        "claude-3-haiku-20240307",
        "claude-haiku-4-5",
        "claude-haiku-4-5-20251001",
        "claude-haiku-4-5-20251001-v1",
        "claude-haiku-4-5-20251001-v1[1m]",
        "claude-haiku-4-5-20251001[1m]",
        "claude-haiku-4-5[1m]",
    ];
    // Haiku 4.5 tops out at 200K, so its `[1m]` keys always fall back.
    insert_tier(&mut overrides, &haiku_overrides, haiku_wire, false);

    for m in &provider.models {
        let trimmed = m.trim();
        if !trimmed.is_empty() {
            overrides.insert(
                trimmed.to_string(),
                serde_json::Value::String(trimmed.to_string()),
            );
        }
    }

    // Short aliases mapping MUST be added after self-mapping loop to ensure
    // aliases like "opus", "sonnet", "haiku", "default", "opusplan" correctly
    // route to the target wire name.
    insert_tier(
        &mut overrides,
        CLAUDE_CODE_SONNET_ALIASES,
        sonnet_wire,
        false,
    );
    insert_tier(&mut overrides, CLAUDE_CODE_OPUS_ALIASES, opus_wire, false);
    insert_tier(&mut overrides, CLAUDE_CODE_HAIKU_ALIASES, haiku_wire, false);
    // A custom display name needs a self-map, or Claude Code would leave it
    // untouched only by luck of not being listed here.
    for wire in [opus_wire, sonnet_wire, haiku_wire] {
        overrides.insert(
            wire.to_string(),
            serde_json::Value::String(wire.to_string()),
        );
        if supports_1m && !wire.ends_with("[1m]") && wire != haiku_wire {
            let wire_1m = format!("{wire}[1m]");
            overrides.insert(wire_1m.clone(), serde_json::Value::String(wire_1m));
        }
    }
    if !model_to_use.is_empty() && !overrides.contains_key(model_to_use) {
        overrides.insert(
            model_to_use.to_string(),
            serde_json::Value::String(model_to_use.to_string()),
        );
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
            // Write only ANTHROPIC_AUTH_TOKEN to avoid mutual exclusivity warnings in Claude Code
            env_obj.insert(
                "ANTHROPIC_AUTH_TOKEN".into(),
                serde_json::Value::String(token_to_write.clone()),
            );
            env_obj.remove("ANTHROPIC_API_KEY");
        } else {
            env_obj.remove("ANTHROPIC_API_KEY");
            env_obj.remove("ANTHROPIC_AUTH_TOKEN");
        }

        // `*_MODEL` is what actually goes on the wire when Claude Code resolves
        // an alias itself, so it carries the wire name. `*_MODEL_NAME` and
        // `*_MODEL_DESCRIPTION` are labels only — the `/model` picker ignores
        // them — but the description is a useful place to show the real upstream
        // model behind a display name.
        for (tier, wire, upstream) in [
            ("SONNET", sonnet_wire, sonnet_candidate),
            ("OPUS", opus_wire, opus_candidate),
            ("HAIKU", haiku_wire, haiku_candidate),
        ] {
            let description = if wire == upstream {
                format!("{wire} via {}", provider.name)
            } else {
                format!("{wire} -> {upstream} via {}", provider.name)
            };
            env_obj.insert(
                format!("ANTHROPIC_DEFAULT_{tier}_MODEL"),
                serde_json::Value::String(wire.to_string()),
            );
            env_obj.insert(
                format!("ANTHROPIC_DEFAULT_{tier}_MODEL_NAME"),
                serde_json::Value::String(wire.to_string()),
            );
            env_obj.insert(
                format!("ANTHROPIC_DEFAULT_{tier}_MODEL_DESCRIPTION"),
                serde_json::Value::String(description),
            );
        }

        env_obj.insert(
            "CLAUDE_CODE_SUBAGENT_MODEL".into(),
            serde_json::Value::String("inherit".into()),
        );

        // Without this the `/model` picker only ever offers the three tier slots
        // written above, no matter how many models the provider serves. The
        // gateway answers `/v1/models` with the capability metadata the picker
        // needs (see gateway::router::synthesize_models_response), so turn the
        // discovery call on to let every provider model reach the picker — each
        // with its own effort control, which a bare tier slot never gets.
        //
        // Only meaningful behind the gateway: pointed straight at a provider,
        // the discovery call would hit an upstream that owes us nothing.
        //
        // Written on both branches rather than only when enabled. This file is
        // merged, not replaced, so skipping the write leaves whatever the last
        // profile put here — a gateway-less profile inheriting "1" sends the
        // picker at an upstream that never answers it, and a gateway profile
        // inheriting "0" silently falls back to Claude Code's built-in model
        // list, which is what made the picker label tiers with stale names.
        env_obj.insert(
            "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY".into(),
            serde_json::Value::String(if gateway_enabled { "1" } else { "0" }.into()),
        );
        let effort_level = if let Some(eff) = &provider.default_effort_level {
            if !eff.trim().is_empty() {
                eff.trim().to_string()
            } else {
                let (def_reasoning, _, _) = get_model_reasoning_config(model_to_use);
                def_reasoning.as_str().unwrap_or("high").to_string()
            }
        } else {
            let (def_reasoning, _, _) = get_model_reasoning_config(model_to_use);
            def_reasoning.as_str().unwrap_or("high").to_string()
        };

        env_obj.insert(
            "CLAUDE_CODE_EFFORT_LEVEL".into(),
            serde_json::Value::String(effort_level),
        );

        // Claude Code sizes a session from the model *name*, against a catalogue
        // compiled into its binary. A name it does not know gets a flat 200K
        // assumption — auto-compact then starts squeezing at 200K no matter how
        // much window the upstream really serves. Model discovery does not help:
        // its `/v1/models` reply carries no context field, and the catalogue is
        // never filled from the network.
        //
        // This variable is the override, but it applies only to names that do
        // not start with `claude-`, so it is dead weight for the synthetic tier
        // names written above and useful for the provider's own model ids.
        //
        // One variable covers the whole session while the user can switch models
        // within it, so it carries the *smallest* budget among the models they
        // could select — too small only wastes window, too large overruns the
        // upstream. An unknown name in the list means no floor can be trusted
        // and nothing is written; Claude Code's own 200K assumption stands.
        //
        // Removed rather than skipped when unknown: this file is merged, so a
        // leftover value from the previous profile would otherwise size a
        // session against a model that is no longer configured.
        let budget = std::iter::once(model_to_use)
            .chain(provider.models.iter().map(|m| m.trim()))
            .filter(|name| !name.is_empty())
            .try_fold(u64::MAX, |floor, name| {
                model_window(name).map(|w| floor.min(w.claude_budget()))
            });
        match budget {
            Some(tokens) if tokens > 0 => {
                env_obj.insert(
                    "CLAUDE_CODE_MAX_CONTEXT_TOKENS".into(),
                    serde_json::Value::String(tokens.to_string()),
                );
            }
            _ => {
                env_obj.remove("CLAUDE_CODE_MAX_CONTEXT_TOKENS");
            }
        }
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
        obj.insert(
            "hasCompletedOnboarding".into(),
            serde_json::Value::Bool(true),
        );
        // Remove primaryApiKey and oauthAccount from ~/.claude.json so Claude Code uses
        // ANTHROPIC_AUTH_TOKEN and ANTHROPIC_BASE_URL configured in settings.json without
        // triggering "/login managed key" conflicts.
        obj.remove("primaryApiKey");
        obj.remove("oauthAccount");
    }
    if let Ok(serialized) = serde_json::to_string_pretty(&claude_json) {
        let _ = crate::storage::atomic_replace(&claude_json_path, serialized.as_bytes());
    }

    Ok(())
}

/// Point Claude Desktop at the profile's endpoint.
///
/// Two separate surfaces, in two separate directory trees: the MCP servers below,
/// and the third-party endpoint in `Claude-3p`, which
/// [`crate::claude_desktop::apply`] owns.
async fn write_claude_desktop_config(
    provider: &crate::profile::ProviderConfig,
    profile_id: &str,
    gateway_enabled: bool,
) -> AppResult<()> {
    let home =
        crate::user_home_dir().ok_or_else(|| AppError::Config("无法确定用户主目录".into()))?;

    #[cfg(windows)]
    let config_path = crate::roaming_app_data_dir()
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
        config
            .as_object_mut()
            .unwrap()
            .insert("mcpServers".into(), serde_json::json!({}));
    }

    let content = serde_json::to_string_pretty(&config)?;
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::storage::atomic_replace(&config_path, content.as_bytes())?;

    crate::claude_desktop::apply(&desktop_endpoint_spec(
        provider,
        profile_id,
        gateway_enabled,
    ))
}

/// Describe the endpoint Claude Desktop should use for this provider.
fn desktop_endpoint_spec(
    provider: &crate::profile::ProviderConfig,
    profile_id: &str,
    gateway_enabled: bool,
) -> crate::claude_desktop::EndpointSpec {
    // Desktop appends the `/v1/...` path itself, so a `/v1` suffix here would
    // make it request `/v1/v1/messages` and 404.
    let base_url = if gateway_enabled {
        format!("http://127.0.0.1:{GATEWAY_PORT}")
    } else {
        strip_anthropic_version_suffix(&provider.base_url)
    };

    let api_key = crate::credentials::get_api_key(profile_id)
        .ok()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
        .unwrap_or_else(|| {
            if gateway_enabled {
                "ai-deck-local".to_string()
            } else {
                String::new()
            }
        });

    crate::claude_desktop::EndpointSpec {
        base_url,
        api_key,
        models: desktop_inference_models(provider, gateway_enabled),
    }
}

/// The three tiers as Desktop's model menu should list them.
///
/// Without the gateway there is nothing to translate a display name into the
/// provider's real model, so the menu has to carry the upstream names verbatim.
fn desktop_inference_models(
    provider: &crate::profile::ProviderConfig,
    gateway_enabled: bool,
) -> Vec<crate::claude_desktop::InferenceModel> {
    let (opus_wire, sonnet_wire, haiku_wire) = if gateway_enabled {
        claude_wire_names(provider)
    } else {
        claude_tier_candidates(provider)
    };
    let (opus_display, sonnet_display, haiku_display) = claude_display_names(provider);
    let supports_1m = provider.supports_1m_context.unwrap_or(false);

    let mut models = Vec::new();
    let mut seen = HashSet::new();
    // Haiku 4.5 caps at 200K, so only the two larger tiers claim 1M.
    for (wire, display, tier_supports_1m) in [
        (opus_wire, opus_display, supports_1m),
        (sonnet_wire, sonnet_display, supports_1m),
        (haiku_wire, haiku_display, false),
    ] {
        if wire.is_empty() || !seen.insert(wire.to_string()) {
            continue;
        }
        models.push(crate::claude_desktop::InferenceModel {
            name: wire.to_string(),
            // Only meaningful when the gateway is there to map it back.
            label_override: (gateway_enabled && display != wire).then(|| display.to_string()),
            supports_1m: tier_supports_1m,
        });
    }

    let default_model = claude_default_model(provider);
    if !default_model.is_empty() && seen.insert(default_model.to_string()) {
        models.push(crate::claude_desktop::InferenceModel {
            name: default_model.to_string(),
            label_override: None,
            supports_1m: false,
        });
    }

    models
}

async fn write_hermes_config(
    provider: &crate::profile::ProviderConfig,
    profile_id: &str,
    gateway_enabled: bool,
) -> AppResult<()> {
    let home =
        crate::user_home_dir().ok_or_else(|| AppError::Config("无法确定用户主目录".into()))?;
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
        let _ = crate::storage::atomic_replace(
            &dot_config_hermes.join("config.yaml"),
            config_yaml.as_bytes(),
        );
        let _ = crate::storage::atomic_replace(
            &dot_config_hermes.join("config.json"),
            json_content.as_bytes(),
        );
        let _ =
            crate::storage::atomic_replace(&dot_config_hermes.join(".env"), env_content.as_bytes());
    }

    // Hermes reads whichever of these it finds first, so both get the same
    // content. Resolved through the app-data helpers rather than the environment
    // so tests cannot reach real user data.
    #[cfg(windows)]
    for dir in [crate::roaming_app_data_dir(), crate::local_app_data_dir()]
        .into_iter()
        .flatten()
    {
        let hermes_dir = dir.join("hermes");
        if std::fs::create_dir_all(&hermes_dir).is_ok() {
            let _ = crate::storage::atomic_replace(
                &hermes_dir.join("config.yaml"),
                config_yaml.as_bytes(),
            );
            let _ = crate::storage::atomic_replace(
                &hermes_dir.join("config.json"),
                json_content.as_bytes(),
            );
            let _ =
                crate::storage::atomic_replace(&hermes_dir.join(".env"), env_content.as_bytes());
        }
    }

    Ok(())
}

#[cfg(test)]
// The HOME guard below is deliberately held across the `.await`s in these
// tests. Serializing the whole test body is the point: `AI_DECK_HOME_OVERRIDE`
// is process-global, and the config writers under test are async, so releasing
// the guard at an await would let a concurrent test repoint HOME mid-write.
// These are single-threaded-by-design tests, not a runtime deadlock risk.
#[allow(clippy::await_holding_lock)]
mod tests {
    use super::*;
    use crate::profile::ProfileUpdate;

    /// The crate-wide guard, not a local one: `AI_DECK_HOME_OVERRIDE` is
    /// process-global, so a mutex private to this module cannot exclude a test in
    /// another module that touches the same variable.
    use crate::lock_home_env;

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
        assert_eq!(
            list[0]["supported_reasoning_levels"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
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

    /// Codex's effort submenu hangs on an empty `supported_reasoning_levels`:
    /// nothing is selectable, so the menu can neither be committed nor closed
    /// except with Esc. No model may ever advertise zero levels.
    #[test]
    fn every_model_advertises_at_least_one_reasoning_level() {
        let names = [
            // Third-party relay names that match no pattern in the table.
            "deepseek-v4-pro-0813",
            "deepseek-v4-flash-0731",
            "qwen3.8-max",
            "some-unknown-model",
            // Explicitly non-reasoning models.
            "gpt-4o",
            "gpt-4-turbo",
            "gpt-3.5-turbo",
            "deepseek-chat",
            "deepseek-v3",
            "glm-4.6",
            "llama-3.3-70b",
            "claude-3-5-sonnet",
            // Recognised reasoning families.
            "gpt-5.6-sol",
            "gpt-5.6-luna",
            "gemini-2.5-pro",
            "claude-opus-5",
            "deepseek-reasoner",
            "qwen-qwq-32b",
        ];
        for name in names {
            let (default, levels, _supports) = get_model_reasoning_config(name);
            let levels = levels.as_array().expect("levels is an array");
            assert!(!levels.is_empty(), "{name} advertises no reasoning levels");
            // The default must be one of the advertised levels, or Codex has
            // nothing valid preselected.
            let default = default.as_str().expect("default level is a string");
            assert!(
                levels.iter().any(|l| l["effort"] == default),
                "{name}: default {default} is not among its levels",
            );
        }
    }

    #[test]
    fn unknown_third_party_models_get_the_safe_three_level_ladder() {
        for name in ["deepseek-v4-pro-0813", "qwen3.8-max", "brand-new-model"] {
            let (default, levels, supports) = get_model_reasoning_config(name);
            assert!(supports, "{name}");
            assert_eq!(default, "medium", "{name}");
            let efforts: Vec<&str> = levels
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l["effort"].as_str().unwrap())
                .collect();
            assert_eq!(efforts, vec!["low", "medium", "high"], "{name}");
        }
    }

    #[test]
    fn non_reasoning_models_offer_none_rather_than_nothing() {
        let (default, levels, supports) = get_model_reasoning_config("gpt-4o");
        assert!(!supports, "gpt-4o must not claim reasoning summaries");
        assert_eq!(default, "none");
        let efforts: Vec<&str> = levels
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["effort"].as_str().unwrap())
            .collect();
        assert_eq!(efforts, vec!["none"]);
    }

    #[tokio::test]
    async fn test_switch_profile_active_isolation() {
        let _home_guard = lock_home_env();
        let temp_home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_DECK_HOME_OVERRIDE", temp_home.path());
        let state_path = temp_home.path().join(".ai-deck").join("state.json");
        let mut pm = ProfileManager::with_state_path(state_path);

        let mut p1 = pm.create_profile_simple("方案Alpha").unwrap();
        p1.providers[0].name = "Alpha Provider".into();
        p1.providers[0].default_model = "alpha-model".into();
        p1.providers[0].models = vec!["alpha-model".into()];
        let p1 = pm
            .update_profile(
                &p1.id,
                ProfileUpdate {
                    name: Some("方案Alpha".into()),
                    providers: Some(p1.providers),
                    clients: Some(vec![
                        "codex-cli".into(),
                        "claude-code".into(),
                        "hermes".into(),
                        "claude-desktop".into(),
                    ]),
                    gateway_enabled: Some(true),
                    failover_enabled: None,
                },
            )
            .unwrap();

        let mut p2 = pm.create_profile_simple("方案Beta").unwrap();
        p2.providers[0].name = "Beta Provider".into();
        p2.providers[0].default_model = "beta-model".into();
        p2.providers[0].models = vec!["beta-model".into()];
        let p2 = pm
            .update_profile(
                &p2.id,
                ProfileUpdate {
                    name: Some("方案Beta".into()),
                    providers: Some(p2.providers),
                    clients: Some(vec![
                        "codex-cli".into(),
                        "claude-code".into(),
                        "hermes".into(),
                        "claude-desktop".into(),
                    ]),
                    gateway_enabled: Some(true),
                    failover_enabled: None,
                },
            )
            .unwrap();

        // 1. Switch to Profile Alpha
        let res1 = switch_profile(&mut pm, &p1.id).await.unwrap();
        assert!(res1.success);
        assert_eq!(pm.active_profile().unwrap().id, p1.id);

        let home = crate::user_home_dir().unwrap();
        let codex_doc = std::fs::read_to_string(home.join(".codex").join("config.toml")).unwrap();
        assert!(
            codex_doc.contains("alpha-model"),
            "Codex 配置应写入 Alpha 方案模型"
        );
        assert!(
            !codex_doc.contains("beta-model"),
            "Codex 配置中不应包含未激活的 Beta 方案模型"
        );

        let hermes_yaml =
            std::fs::read_to_string(home.join(".hermes").join("config.yaml")).unwrap();
        assert!(
            hermes_yaml.contains("custom_providers:"),
            "Hermes 配置应包含 custom_providers"
        );
        assert!(
            hermes_yaml.contains("inference_provider: custom"),
            "Hermes 配置应声明 inference_provider: custom"
        );
        assert!(
            hermes_yaml.contains("model: alpha-model"),
            "Hermes 配置应写入 Alpha 方案模型"
        );
        assert!(
            !hermes_yaml.lines().any(|l| l.starts_with("api_key:")),
            "Hermes 配置根层级不应包含 api_key"
        );
        assert!(
            !hermes_yaml.lines().any(|l| l.starts_with("base_url:")),
            "Hermes 配置根层级不应包含 base_url"
        );
        assert!(
            !hermes_yaml.contains("model: beta-model"),
            "Hermes 配置中不应包含未激活的 Beta 方案模型"
        );

        // 2. Switch to Profile Beta
        let res2 = switch_profile(&mut pm, &p2.id).await.unwrap();
        assert!(res2.success);
        assert_eq!(pm.active_profile().unwrap().id, p2.id);

        let codex_doc2 = std::fs::read_to_string(home.join(".codex").join("config.toml")).unwrap();
        assert!(
            codex_doc2.contains("beta-model"),
            "Codex 配置应更新为 Beta 方案模型"
        );
        assert!(
            !codex_doc2.contains("alpha-model"),
            "Codex 配置中不应残留 Alpha 方案模型"
        );

        let hermes_yaml2 =
            std::fs::read_to_string(home.join(".hermes").join("config.yaml")).unwrap();
        assert!(
            hermes_yaml2.contains("model: beta-model"),
            "Hermes 配置应更新为 Beta 方案模型"
        );
        assert!(
            !hermes_yaml2.contains("alpha-model"),
            "Hermes 配置中不应残留 Alpha 方案模型"
        );
    }

    #[tokio::test]
    async fn test_claude_config_aliases_and_invariant() {
        let _home_guard = lock_home_env();
        let temp_home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_DECK_HOME_OVERRIDE", temp_home.path());
        let claude_dir = temp_home.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();

        let provider = crate::profile::ProviderConfig {
            id: "test-provider".into(),
            name: "Test Provider".into(),
            base_url: "http://127.0.0.1:18888/v1".into(),
            protocol: crate::types::ProtocolKind::Anthropic,
            is_primary: true,
            codex_compat: crate::types::CodexToolCompat::ResponsesFunction,
            reasoning_confidence: crate::types::ReasoningConfidence::Unknown,
            thinking_support: crate::types::ThinkingSupport::Unprobed,
            models: vec!["model-S".into(), "model-O".into(), "claude-opus-5".into()],
            default_model: "opus".into(),
            accept_invalid_certs: false,
            max_price_per_request: None,
            rate_limit: crate::profile::RateLimitSettings::default(),
            supports_1m_context: Some(false),
            default_effort_level: Some("high".into()),
            opus_model: None,
            sonnet_model: None,
            haiku_model: None,
            opus_display_name: None,
            sonnet_display_name: None,
            haiku_display_name: None,
        };

        let res = write_claude_config(&provider, "test-profile", true).await;
        assert!(res.is_ok());

        let settings_path = claude_dir.join("settings.json");
        let content = std::fs::read_to_string(&settings_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        // 1. Check auth: only ANTHROPIC_AUTH_TOKEN, no ANTHROPIC_API_KEY
        let env = parsed.get("env").and_then(|v| v.as_object()).unwrap();
        assert!(
            env.contains_key("ANTHROPIC_AUTH_TOKEN"),
            "env 应包含 ANTHROPIC_AUTH_TOKEN"
        );
        assert!(
            !env.contains_key("ANTHROPIC_API_KEY"),
            "env 不应包含 ANTHROPIC_API_KEY 以免互斥告警"
        );

        // 2. Every alias resolves to its tier's display name. With the gateway on
        //    the wire carries display names, not provider models — the gateway is
        //    what turns them back into provider models.
        let overrides = parsed
            .get("modelOverrides")
            .and_then(|v| v.as_object())
            .unwrap();
        for alias in CLAUDE_CODE_OPUS_ALIASES {
            assert_eq!(
                overrides.get(*alias).and_then(|v| v.as_str()),
                Some(DEFAULT_OPUS_DISPLAY_NAME)
            );
        }
        for alias in CLAUDE_CODE_SONNET_ALIASES {
            assert_eq!(
                overrides.get(*alias).and_then(|v| v.as_str()),
                Some(DEFAULT_SONNET_DISPLAY_NAME)
            );
        }
        for alias in CLAUDE_CODE_HAIKU_ALIASES {
            assert_eq!(
                overrides.get(*alias).and_then(|v| v.as_str()),
                Some(DEFAULT_HAIKU_DISPLAY_NAME)
            );
        }

        // 3. Invariant: every availableModels entry resolves to something the
        //    gateway can route — a provider model or a tier display name.
        let available_models = parsed
            .get("availableModels")
            .and_then(|v| v.as_array())
            .unwrap();
        let mut routable: HashSet<String> = provider.models.iter().cloned().collect();
        routable.extend(
            [
                DEFAULT_OPUS_DISPLAY_NAME,
                DEFAULT_SONNET_DISPLAY_NAME,
                DEFAULT_HAIKU_DISPLAY_NAME,
            ]
            .map(str::to_string),
        );

        for m_val in available_models {
            let m_str = m_val.as_str().unwrap();
            let resolved = overrides
                .get(m_str)
                .and_then(|v| v.as_str())
                .unwrap_or(m_str);
            assert!(
                routable.contains(resolved),
                "availableModels 项 '{m_str}' 经 modelOverrides 解析为 '{resolved}'，既不在 provider.models {:?} 中，也不是显示名",
                provider.models
            );
        }

        // 4. Test custom candidate overrides (opus -> claude-opus-5-max, sonnet -> claude-opus-5-xhigh, haiku -> model-S)
        let custom_provider = crate::profile::ProviderConfig {
            id: "custom-provider".into(),
            name: "Custom Provider".into(),
            base_url: "http://127.0.0.1:18888/v1".into(),
            protocol: crate::types::ProtocolKind::Anthropic,
            is_primary: true,
            codex_compat: crate::types::CodexToolCompat::ResponsesFunction,
            reasoning_confidence: crate::types::ReasoningConfidence::Unknown,
            thinking_support: crate::types::ThinkingSupport::Unprobed,
            models: vec![
                "model-S".into(),
                "model-O".into(),
                "claude-opus-5".into(),
                "claude-opus-5-max".into(),
                "claude-opus-5-xhigh".into(),
            ],
            default_model: "opus".into(),
            accept_invalid_certs: false,
            max_price_per_request: None,
            rate_limit: crate::profile::RateLimitSettings::default(),
            supports_1m_context: Some(false),
            default_effort_level: Some("high".into()),
            opus_model: Some("claude-opus-5-max".into()),
            sonnet_model: Some("claude-opus-5-xhigh".into()),
            haiku_model: Some("model-S".into()),
            opus_display_name: None,
            sonnet_display_name: None,
            haiku_display_name: None,
        };

        let res2 = write_claude_config(&custom_provider, "custom-profile", true).await;
        assert!(res2.is_ok());
        let content2 = std::fs::read_to_string(&settings_path).unwrap();
        let parsed2: serde_json::Value = serde_json::from_str(&content2).unwrap();
        let overrides2 = parsed2
            .get("modelOverrides")
            .and_then(|v| v.as_object())
            .unwrap();
        // A custom upstream model normally leaves the shown name alone — except
        // where the tier's display name is another model this provider serves.
        // `claude-opus-5` is such a name here, so the Opus tier carries its
        // upstream name instead and the literal `claude-opus-5` stays reachable.
        assert_eq!(
            overrides2.get("opus").and_then(|v| v.as_str()),
            Some("claude-opus-5-max")
        );
        assert_eq!(
            overrides2.get("sonnet").and_then(|v| v.as_str()),
            Some(DEFAULT_SONNET_DISPLAY_NAME)
        );
        assert_eq!(
            overrides2.get("haiku").and_then(|v| v.as_str()),
            Some(DEFAULT_HAIKU_DISPLAY_NAME)
        );
        // `claude-opus-5` must still address the provider's own model, not the tier.
        assert_eq!(
            overrides2.get("claude-opus-5").and_then(|v| v.as_str()),
            Some("claude-opus-5")
        );
        let env2 = parsed2.get("env").and_then(|v| v.as_object()).unwrap();
        assert_eq!(
            env2.get("ANTHROPIC_DEFAULT_OPUS_MODEL")
                .and_then(|v| v.as_str()),
            Some("claude-opus-5-max")
        );
        // Wire name and upstream now agree, so the label states one model.
        assert_eq!(
            env2.get("ANTHROPIC_DEFAULT_OPUS_MODEL_DESCRIPTION")
                .and_then(|v| v.as_str()),
            Some("claude-opus-5-max via Custom Provider")
        );
        // Sonnet did not collide, so it keeps the display-name indirection.
        assert_eq!(
            env2.get("ANTHROPIC_DEFAULT_SONNET_MODEL_DESCRIPTION")
                .and_then(|v| v.as_str()),
            Some("claude-sonnet-5 -> claude-opus-5-xhigh via Custom Provider")
        );
        // Discovery has to be on, or the picker only ever offers the three slots.
        assert_eq!(
            env2.get("CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY")
                .and_then(|v| v.as_str()),
            Some("1")
        );

        // 5. Retired model IDs must never be emitted: Claude Code renders a
        //    "was retired on ..." banner for any such key present here.
        for retired in [
            "claude-3-opus",
            "claude-3-opus-20240229",
            "claude-3-opus-latest",
            "claude-3-7-sonnet",
            "claude-3-7-sonnet-20250219",
            "claude-3-7-sonnet-latest",
            "claude-3-5-haiku",
            "claude-3-5-haiku-20241022",
            "claude-3-5-haiku-latest",
        ] {
            assert!(
                !overrides2.contains_key(retired),
                "modelOverrides 不应包含已退役模型 {retired}，否则 Claude Code 会显示退役警告"
            );
        }
    }

    /// Build a provider carrying just the fields tier resolution reads.
    fn tier_provider(
        models: &[&str],
        opus: Option<&str>,
        sonnet: Option<&str>,
        haiku: Option<&str>,
    ) -> crate::profile::ProviderConfig {
        crate::profile::ProviderConfig {
            id: "p".into(),
            name: "P".into(),
            base_url: "http://127.0.0.1:18888".into(),
            protocol: crate::types::ProtocolKind::Anthropic,
            is_primary: true,
            codex_compat: crate::types::CodexToolCompat::ResponsesFunction,
            reasoning_confidence: crate::types::ReasoningConfidence::Unknown,
            thinking_support: crate::types::ThinkingSupport::Unprobed,
            models: models.iter().map(|s| s.to_string()).collect(),
            default_model: models.first().unwrap_or(&"sonnet").to_string(),
            accept_invalid_certs: false,
            max_price_per_request: None,
            rate_limit: crate::profile::RateLimitSettings::default(),
            supports_1m_context: Some(false),
            default_effort_level: None,
            opus_model: opus.map(str::to_string),
            sonnet_model: sonnet.map(str::to_string),
            haiku_model: haiku.map(str::to_string),
            opus_display_name: None,
            sonnet_display_name: None,
            haiku_display_name: None,
        }
    }

    #[test]
    fn tier_guess_reads_irregular_relay_spellings() {
        // Relays rename freely. The guess is keyword-based and case-insensitive,
        // so a reordered, decorated or vendor-prefixed name still lands.
        for (catalog, expected_opus) in [
            (vec!["Claude-5-opus", "Claude-5-sonnet"], "Claude-5-opus"),
            (
                vec!["claude-opus-5-A", "claude-sonnet-5-A"],
                "claude-opus-5-A",
            ),
            (vec!["Claude.Opus.5", "Claude.Sonnet.4.5"], "Claude.Opus.5"),
            (vec!["Claude-Opus-5-thinking"], "Claude-Opus-5-thinking"),
            (vec!["anthropic/claude-opus-5"], "anthropic/claude-opus-5"),
            (vec!["CLAUDE-5-OPUS"], "CLAUDE-5-OPUS"),
        ] {
            let provider = tier_provider(&catalog, None, None, None);
            let (opus, _, _) = claude_tier_candidates(&provider);
            assert_eq!(opus, expected_opus, "catalog {catalog:?}");
        }
    }

    #[test]
    fn tier_guess_prefers_the_canonical_name_over_catalog_order() {
        // Both orders must agree, or the same two models would produce different
        // picker labels depending on how the relay happened to list them.
        for catalog in [
            vec!["claude-opus-5", "claude-opus-5-A"],
            vec!["claude-opus-5-A", "claude-opus-5"],
        ] {
            let provider = tier_provider(&catalog, None, None, None);
            let (opus, _, _) = claude_tier_candidates(&provider);
            assert_eq!(opus, "claude-opus-5", "catalog {catalog:?}");
            // And the tier keeps its canonical label, which is what lets Claude
            // Code size and price it.
            let (opus_wire, _, _) = claude_wire_names(&provider);
            assert_eq!(opus_wire, DEFAULT_OPUS_DISPLAY_NAME);
        }
    }

    #[test]
    fn tier_guess_falls_back_to_least_decorated_match() {
        // No canonical name present, so the plainest match wins either order.
        for catalog in [
            vec!["Claude-5-opus-preview", "Claude-5-opus"],
            vec!["Claude-5-opus", "Claude-5-opus-preview"],
        ] {
            let provider = tier_provider(&catalog, None, None, None);
            let (opus, _, _) = claude_tier_candidates(&provider);
            assert_eq!(opus, "Claude-5-opus", "catalog {catalog:?}");
        }
    }

    #[test]
    fn wire_name_falls_back_to_upstream_only_on_collision() {
        // `claude-opus-5` is both the default Opus display name and a model this
        // provider serves, while the tier is pinned to `-max`. Redirecting the
        // display name would strand the literal model, so the tier takes the
        // upstream name. Sonnet's display name collides with nothing and keeps
        // the indirection that lets Claude Code size and price it.
        let provider = tier_provider(
            &[
                "model-S",
                "claude-opus-5",
                "claude-opus-5-max",
                "claude-opus-5-xhigh",
                "model-T",
            ],
            Some("claude-opus-5-max"),
            Some("claude-opus-5-xhigh"),
            Some("model-T"),
        );
        let (opus, sonnet, haiku) = claude_wire_names(&provider);
        assert_eq!(opus, "claude-opus-5-max");
        assert_eq!(sonnet, DEFAULT_SONNET_DISPLAY_NAME);
        assert_eq!(haiku, DEFAULT_HAIKU_DISPLAY_NAME);
    }

    #[test]
    fn wire_names_keep_display_names_for_unrelated_providers() {
        // The ordinary case: a GLM provider serves no Claude names, so all three
        // tiers keep their display names and the gateway maps them back.
        let provider = tier_provider(&["glm-4.6", "glm-4.5-air"], None, None, Some("glm-4.5-air"));
        let (opus, sonnet, haiku) = claude_wire_names(&provider);
        assert_eq!(opus, DEFAULT_OPUS_DISPLAY_NAME);
        assert_eq!(sonnet, DEFAULT_SONNET_DISPLAY_NAME);
        assert_eq!(haiku, DEFAULT_HAIKU_DISPLAY_NAME);
    }

    #[test]
    fn wire_name_keeps_display_name_when_it_is_the_tier_model() {
        // Serving `claude-opus-5` *as* the Opus tier is not a collision; the name
        // means the same thing on both sides.
        let provider = tier_provider(&["claude-opus-5", "model-S"], None, None, None);
        let (opus, _, _) = claude_wire_names(&provider);
        assert_eq!(opus, "claude-opus-5");
    }

    #[tokio::test]
    async fn discovery_flag_tracks_the_gateway_and_never_inherits() {
        // settings.json is merged, not replaced. A gateway profile that wrote "1"
        // must not leave it behind for a gateway-less profile, and vice versa —
        // an inherited "0" is what made the picker fall back to Claude Code's
        // built-in model list and label tiers with stale names.
        let _home_guard = lock_home_env();
        let temp_home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_DECK_HOME_OVERRIDE", temp_home.path());
        let claude_dir = temp_home.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        let settings_path = claude_dir.join("settings.json");

        let provider = tier_provider(&["claude-opus-5-max", "claude-sonnet-5"], None, None, None);
        let read_flag = |path: &std::path::Path| -> Option<String> {
            let parsed: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            parsed
                .pointer("/env/CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        };

        write_claude_config(&provider, "p", true).await.unwrap();
        assert_eq!(read_flag(&settings_path).as_deref(), Some("1"));

        // Same file, gateway now off: the flag has to flip, not linger.
        write_claude_config(&provider, "p", false).await.unwrap();
        assert_eq!(read_flag(&settings_path).as_deref(), Some("0"));

        // And back on again.
        write_claude_config(&provider, "p", true).await.unwrap();
        assert_eq!(read_flag(&settings_path).as_deref(), Some("1"));
    }

    #[test]
    fn tier_warnings_flag_collapsed_tiers() {
        // A catalog with no tier words hands every tier the same fallback; the
        // switch must say so instead of letting the collapse pass silently.
        let provider = tier_provider(
            &["model-S", "model-O", "model-A", "model-T"],
            None,
            None,
            None,
        );
        let warnings = claude_tier_warnings(&provider);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("model-S"), "{warnings:?}");
        assert!(warnings[0].contains("Opus"), "{warnings:?}");
        assert!(warnings[0].contains("Haiku"), "{warnings:?}");
    }

    #[test]
    fn tier_warnings_stay_quiet_when_every_tier_is_pinned() {
        // All three pinned to one model is deliberate and must not warn.
        let provider = tier_provider(
            &["big", "mid", "small"],
            Some("big"),
            Some("big"),
            Some("big"),
        );
        assert!(claude_tier_warnings(&provider).is_empty());
    }

    #[test]
    fn tier_warnings_flag_overridden_explicit_display_names() {
        // The display name collides with a different provider model, so the wire
        // name overrides it; the user set it explicitly and needs to know.
        let mut provider = tier_provider(
            &["claude-opus-5", "claude-opus-5-max", "model-S"],
            Some("claude-opus-5-max"),
            None,
            None,
        );
        provider.opus_display_name = Some("claude-opus-5".into());
        let warnings = claude_tier_warnings(&provider);
        let hit = warnings
            .iter()
            .find(|w| w.contains("claude-opus-5-max") && w.contains("claude-opus-5"))
            .expect("override warning missing: {warnings:?}");
        assert!(hit.contains("显示名"), "{hit}");
    }

    #[test]
    fn tier_warnings_quiet_when_the_display_name_is_its_own_model() {
        // Each tier serves a distinct model and the display names equal them, so
        // neither collapse nor override warnings apply.
        let provider = tier_provider(
            &["claude-opus-5", "claude-sonnet-5", "claude-haiku-4-5"],
            None,
            None,
            None,
        );
        assert!(claude_tier_warnings(&provider).is_empty());
    }

    #[tokio::test]
    async fn test_claude_config_display_names() {
        let _home_guard = lock_home_env();
        let temp_home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_DECK_HOME_OVERRIDE", temp_home.path());
        let claude_dir = temp_home.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();

        let base = crate::profile::ProviderConfig {
            id: "p".into(),
            name: "Zhipu".into(),
            base_url: "https://open.bigmodel.cn/api/anthropic".into(),
            protocol: crate::types::ProtocolKind::Anthropic,
            is_primary: true,
            codex_compat: crate::types::CodexToolCompat::ResponsesFunction,
            reasoning_confidence: crate::types::ReasoningConfidence::Unknown,
            thinking_support: crate::types::ThinkingSupport::Unprobed,
            models: vec!["glm-4.6".into(), "glm-4.5-air".into()],
            default_model: "glm-4.6".into(),
            accept_invalid_certs: false,
            max_price_per_request: None,
            rate_limit: crate::profile::RateLimitSettings::default(),
            supports_1m_context: Some(true),
            default_effort_level: None,
            opus_model: None,
            sonnet_model: None,
            haiku_model: None,
            opus_display_name: Some("claude-opus-4-8".into()),
            sonnet_display_name: None,
            haiku_display_name: Some("   ".into()),
        };

        let settings_path = claude_dir.join("settings.json");
        let read_config = || -> serde_json::Value {
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap()
        };

        write_claude_config(&base, "p", true).await.unwrap();
        let parsed = read_config();
        let overrides = parsed
            .get("modelOverrides")
            .and_then(|v| v.as_object())
            .unwrap();
        let available: Vec<&str> = parsed["availableModels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();

        // A custom name is honoured; a blank one falls back to the default.
        assert_eq!(
            overrides.get("opus").and_then(|v| v.as_str()),
            Some("claude-opus-4-8")
        );
        assert_eq!(
            overrides.get("haiku").and_then(|v| v.as_str()),
            Some(DEFAULT_HAIKU_DISPLAY_NAME)
        );
        assert_eq!(
            overrides.get("sonnet").and_then(|v| v.as_str()),
            Some(DEFAULT_SONNET_DISPLAY_NAME)
        );
        // A custom display name self-maps, so Claude Code leaves it alone.
        assert_eq!(
            overrides.get("claude-opus-4-8").and_then(|v| v.as_str()),
            Some("claude-opus-4-8")
        );

        // Display names lead the picker and the bare aliases survive alongside.
        assert_eq!(available[0], "claude-opus-4-8");
        for alias in ["opus", "sonnet", "haiku"] {
            assert!(
                available.contains(&alias),
                "别名 {alias} 应保留在 availableModels 中"
            );
        }

        // 1M-capable tiers offer a `[1m]` entry that keeps its suffix; Haiku,
        // capped at 200K, never does.
        assert!(available.contains(&"claude-opus-4-8[1m]"));
        assert!(available.contains(&"claude-sonnet-5[1m]"));
        assert!(!available
            .iter()
            .any(|m| m.starts_with(DEFAULT_HAIKU_DISPLAY_NAME) && m.ends_with("[1m]")));
        assert_eq!(
            overrides.get("claude-opus-5[1m]").and_then(|v| v.as_str()),
            Some("claude-opus-4-8[1m]")
        );
        assert_eq!(
            overrides
                .get("claude-haiku-4-5[1m]")
                .and_then(|v| v.as_str()),
            Some(DEFAULT_HAIKU_DISPLAY_NAME)
        );

        // Without the gateway nothing can translate a display name, so the wire
        // has to carry the provider's own models.
        let no_1m = crate::profile::ProviderConfig {
            supports_1m_context: Some(false),
            ..base.clone()
        };
        write_claude_config(&no_1m, "p", false).await.unwrap();
        let parsed = read_config();
        let overrides = parsed
            .get("modelOverrides")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(
            overrides.get("opus").and_then(|v| v.as_str()),
            Some("glm-4.6")
        );
        assert_eq!(
            overrides.get("sonnet").and_then(|v| v.as_str()),
            Some("glm-4.6")
        );
        assert!(!parsed["availableModels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m == "claude-opus-4-8"));
        // A `[1m]` request must not survive to an upstream that cannot serve it.
        assert_eq!(
            overrides.get("claude-opus-5[1m]").and_then(|v| v.as_str()),
            Some("glm-4.6")
        );
    }

    #[test]
    fn test_strip_anthropic_version_suffix() {
        // A trailing /v1 must be removed: the Anthropic SDK appends it itself.
        assert_eq!(
            strip_anthropic_version_suffix("http://127.0.0.1:18888/v1"),
            "http://127.0.0.1:18888"
        );
        // Trailing slashes must not defeat the check.
        assert_eq!(
            strip_anthropic_version_suffix("http://127.0.0.1:18888/v1/"),
            "http://127.0.0.1:18888"
        );
        // Already bare URLs stay untouched (minus trailing slashes).
        assert_eq!(
            strip_anthropic_version_suffix("https://api.example.com/"),
            "https://api.example.com"
        );
        // "/v1" must only be stripped as a whole path segment.
        assert_eq!(
            strip_anthropic_version_suffix("https://api.example.com/openai/v1"),
            "https://api.example.com/openai"
        );
        // A host ending in something like "apiv1" is not a /v1 suffix.
        assert_eq!(
            strip_anthropic_version_suffix("https://apiv1.example.com"),
            "https://apiv1.example.com"
        );
    }

    #[tokio::test]
    async fn test_claude_config_base_url_has_no_v1_suffix() {
        let _home_guard = lock_home_env();
        let temp_home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_DECK_HOME_OVERRIDE", temp_home.path());
        let claude_dir = temp_home.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();

        // Seed ~/.claude.json with a leftover managed key to prove it gets cleaned.
        let claude_json_path = temp_home.path().join(".claude.json");
        std::fs::write(
            &claude_json_path,
            r#"{"primaryApiKey":"sk-ant-leftover","oauthAccount":{"emailAddress":"a@b.c"}}"#,
        )
        .unwrap();

        let mut provider = crate::profile::ProviderConfig {
            id: "p".into(),
            name: "P".into(),
            // Deliberately configured WITH a /v1 suffix.
            base_url: "https://relay.example.com/v1".into(),
            protocol: crate::types::ProtocolKind::Anthropic,
            is_primary: true,
            codex_compat: crate::types::CodexToolCompat::ResponsesFunction,
            reasoning_confidence: crate::types::ReasoningConfidence::Unknown,
            thinking_support: crate::types::ThinkingSupport::Unprobed,
            models: vec!["claude-opus-5".into()],
            default_model: "claude-opus-5".into(),
            accept_invalid_certs: false,
            max_price_per_request: None,
            rate_limit: crate::profile::RateLimitSettings::default(),
            supports_1m_context: Some(false),
            default_effort_level: Some("high".into()),
            opus_model: None,
            sonnet_model: None,
            haiku_model: None,
            opus_display_name: None,
            sonnet_display_name: None,
            haiku_display_name: None,
        };

        let read_base_url = |dir: &std::path::Path| -> String {
            let content = std::fs::read_to_string(dir.join("settings.json")).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
            parsed["env"]["ANTHROPIC_BASE_URL"]
                .as_str()
                .unwrap()
                .to_string()
        };

        // Gateway mode: must point at the bare loopback origin.
        write_claude_config(&provider, "prof", true).await.unwrap();
        let gw_url = read_base_url(&claude_dir);
        assert_eq!(gw_url, format!("http://127.0.0.1:{GATEWAY_PORT}"));
        assert!(
            !gw_url.ends_with("/v1"),
            "ANTHROPIC_BASE_URL 不能以 /v1 结尾，否则 Claude Code 会请求 /v1/v1/messages 而 404"
        );

        // Direct mode: the provider /v1 suffix must be stripped, not appended to.
        write_claude_config(&provider, "prof", false).await.unwrap();
        let direct_url = read_base_url(&claude_dir);
        assert_eq!(direct_url, "https://relay.example.com");
        assert!(
            !direct_url.ends_with("/v1"),
            "直连模式同样不能保留 /v1 后缀"
        );

        // Direct mode without a /v1 suffix must stay unchanged.
        provider.base_url = "https://relay.example.com".into();
        write_claude_config(&provider, "prof", false).await.unwrap();
        assert_eq!(read_base_url(&claude_dir), "https://relay.example.com");

        // The stale /login managed key must be gone, otherwise Claude Code warns
        // "Both ANTHROPIC_AUTH_TOKEN and /login managed key set".
        let claude_json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&claude_json_path).unwrap()).unwrap();
        assert!(
            claude_json.get("primaryApiKey").is_none(),
            "~/.claude.json 不应残留 primaryApiKey"
        );
        assert!(
            claude_json.get("oauthAccount").is_none(),
            "~/.claude.json 不应残留 oauthAccount"
        );
    }

    /// Targeting Claude Desktop must write its 3P endpoint, and switching away
    /// must hand it back to its own account login.
    #[tokio::test]
    async fn test_desktop_target_writes_the_3p_endpoint() {
        let _home_guard = lock_home_env();
        let temp_home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_DECK_HOME_OVERRIDE", temp_home.path());
        let state_path = temp_home.path().join(".ai-deck").join("state.json");
        let mut pm = ProfileManager::with_state_path(state_path);

        let mut p = pm.create_profile_simple("方案").unwrap();
        p.providers[0].base_url = "https://relay.example.com/v1".into();
        p.providers[0].models = vec!["m".into()];
        p.providers[0].default_model = "m".into();
        let p = pm
            .update_profile(
                &p.id,
                ProfileUpdate {
                    name: Some("方案".into()),
                    providers: Some(p.providers.clone()),
                    clients: Some(vec!["claude-desktop".into()]),
                    gateway_enabled: Some(true),
                    failover_enabled: None,
                },
            )
            .unwrap();

        // A foreign entry stands in for a profile the user made in Desktop itself.
        let threep = temp_home
            .path()
            .join("AppData")
            .join("Local")
            .join("Claude-3p");
        let library = threep.join("configLibrary");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::write(
            library.join("_meta.json"),
            r#"{"appliedId":"foreign-id","entries":[{"id":"foreign-id","name":"111"}]}"#,
        )
        .unwrap();

        let res = switch_profile(&mut pm, &p.id).await.unwrap();
        assert!(res.clients_written.iter().any(|c| c == "claude-desktop"));

        let our_profile = library.join(format!("{}.json", crate::claude_desktop::PROFILE_UUID));
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&our_profile).unwrap()).unwrap();
        assert_eq!(
            written["inferenceGatewayBaseUrl"].as_str(),
            Some(format!("http://127.0.0.1:{GATEWAY_PORT}").as_str()),
            "网关模式应指向回环地址且不带 /v1"
        );

        let mode_of = |dir: &std::path::Path| -> String {
            let raw = std::fs::read_to_string(dir.join("claude_desktop_config.json")).unwrap();
            serde_json::from_str::<serde_json::Value>(&raw).unwrap()["deploymentMode"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        };
        let normal = temp_home
            .path()
            .join("AppData")
            .join("Local")
            .join("Claude");
        assert_eq!(mode_of(&threep), "3p");
        assert_eq!(mode_of(&normal), "3p");
        assert!(
            res.warnings.iter().any(|w| w.contains("重启")),
            "必须提示要重启 Desktop 才生效"
        );
        assert!(
            !res.warnings.iter().any(|w| w.contains("写不到本地文件")),
            "端点现在写得进去了，不该再声称写不到"
        );

        // Switching to a profile that does not claim Desktop restores it.
        let mut other = pm.create_profile_simple("仅 CLI").unwrap();
        other.providers[0].models = vec!["m".into()];
        other.providers[0].default_model = "m".into();
        let other_providers = other.providers.clone();
        let other = pm
            .update_profile(
                &other.id,
                ProfileUpdate {
                    name: Some("仅 CLI".into()),
                    providers: Some(other_providers),
                    clients: Some(vec!["claude-code".into()]),
                    gateway_enabled: Some(true),
                    failover_enabled: None,
                },
            )
            .unwrap();
        switch_profile(&mut pm, &other.id).await.unwrap();

        assert_eq!(mode_of(&threep), "1p");
        assert_eq!(mode_of(&normal), "1p");
        assert!(!our_profile.exists(), "恢复后应删掉 PolyDeck 那份 profile");
        let meta: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(library.join("_meta.json")).unwrap())
                .unwrap();
        assert_eq!(
            meta["appliedId"].as_str(),
            Some("foreign-id"),
            "appliedId 应回到用户自己的 profile"
        );
        assert_eq!(meta["entries"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_model_window_agnes_families_differ() {
        let flash = model_window("agnes-2.5-flash").unwrap();
        assert_eq!(flash.context, 512_000);
        assert_eq!(flash.max_output, 65_536);
        // 512K total minus one full-length reply.
        assert_eq!(flash.claude_budget(), 446_464);

        let pro = model_window("agnes-2.5-pro").unwrap();
        assert_eq!(pro.context, 1_000_000);
        assert_eq!(pro.claude_budget(), 934_464);

        assert_eq!(model_window("agnes-2.0-flash"), Some(flash));
        assert_eq!(model_window("agnes-2.5-pro-alpha"), Some(pro));

        // No documented figure, so no guess.
        assert_eq!(model_window("model-S"), None);
        assert_eq!(model_window("claude-opus-5"), None);
    }

    #[test]
    fn test_codex_context_window_falls_back_to_1m_flag() {
        // A documented name wins over the flag, in both directions.
        assert_eq!(codex_context_window("agnes-2.5-flash", true), 512_000);
        assert_eq!(codex_context_window("agnes-2.5-pro", false), 1_000_000);
        // An unknown name keeps the old flag-driven behaviour.
        assert_eq!(codex_context_window("model-S", true), 1_000_000);
        assert_eq!(codex_context_window("model-S", false), 200_000);
    }

    fn agnes_like_provider(
        models: Vec<String>,
        default_model: &str,
    ) -> crate::profile::ProviderConfig {
        crate::profile::ProviderConfig {
            id: "agnes-cn".into(),
            name: "Agnes AI".into(),
            base_url: "https://api.agnes-ai.cn/v1".into(),
            protocol: crate::types::ProtocolKind::OpenAI,
            is_primary: true,
            codex_compat: crate::types::CodexToolCompat::ChatFunction,
            reasoning_confidence: crate::types::ReasoningConfidence::Verified,
            thinking_support: crate::types::ThinkingSupport::Unsigned,
            models,
            default_model: default_model.into(),
            accept_invalid_certs: false,
            max_price_per_request: None,
            rate_limit: crate::profile::RateLimitSettings::default(),
            supports_1m_context: Some(false),
            default_effort_level: None,
            opus_model: Some(default_model.into()),
            sonnet_model: Some(default_model.into()),
            haiku_model: Some(default_model.into()),
            opus_display_name: None,
            sonnet_display_name: None,
            haiku_display_name: None,
        }
    }

    /// Reads `env.CLAUDE_CODE_MAX_CONTEXT_TOKENS` out of a freshly written
    /// `settings.json`, seeding a stale value first so removal is observable.
    async fn max_context_tokens_after_write(
        provider: &crate::profile::ProviderConfig,
    ) -> Option<String> {
        let temp_home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_DECK_HOME_OVERRIDE", temp_home.path());
        let claude_dir = temp_home.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("settings.json"),
            r#"{"env":{"CLAUDE_CODE_MAX_CONTEXT_TOKENS":"999999"}}"#,
        )
        .unwrap();

        write_claude_config(provider, "test-profile", false)
            .await
            .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(claude_dir.join("settings.json")).unwrap(),
        )
        .unwrap();
        parsed["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"]
            .as_str()
            .map(str::to_string)
    }

    #[tokio::test]
    async fn test_claude_config_writes_context_budget() {
        let _home_guard = lock_home_env();

        // Every model documented: the floor across them is written. Both Flash
        // and Pro are served here, so the Flash budget is the safe one.
        let mixed = agnes_like_provider(
            vec!["agnes-2.5-flash".into(), "agnes-2.5-pro".into()],
            "agnes-2.5-flash",
        );
        assert_eq!(
            max_context_tokens_after_write(&mixed).await.as_deref(),
            Some("446464"),
        );

        // Pro only: nothing forces the budget down to the Flash figure.
        let pro_only = agnes_like_provider(vec!["agnes-2.5-pro".into()], "agnes-2.5-pro");
        assert_eq!(
            max_context_tokens_after_write(&pro_only).await.as_deref(),
            Some("934464"),
        );

        // One undocumented model is enough to void the floor, and the stale
        // value from the previous profile must not survive.
        let partly_unknown = agnes_like_provider(
            vec!["agnes-2.5-flash".into(), "mystery-model-9".into()],
            "agnes-2.5-flash",
        );
        assert_eq!(
            max_context_tokens_after_write(&partly_unknown).await,
            None,
            "未知模型在列时不应写入窗口，且必须清掉上一个 profile 的残留值"
        );
    }
}
