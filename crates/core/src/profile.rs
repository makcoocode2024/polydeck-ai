//! Profile management — multi-provider configuration sets.
//!
//! A Profile groups one or more provider configurations, client targets,
//! MCP servers, skills, and prompts into a switchable unit. Switching a
//! Profile atomically writes all client configs and syncs extensions.

use crate::binding::ClientBinding;
use crate::credentials;
use crate::error::{AppError, AppResult};
use crate::storage;
use crate::types::{CodexToolCompat, ProtocolKind, ReasoningConfidence, ThinkingSupport};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    // No `is_active`. Which clients follow this profile lives in
    // `StateDocument::bindings`, because a bool here cannot say "Codex follows me
    // but Claude Code does not" and would drift from the bindings that decide it.
    pub providers: Vec<ProviderConfig>,
    pub clients: Vec<String>,
    pub mcp_servers: Vec<McpServerConfig>,
    pub skills: Vec<String>,
    pub prompts: Vec<String>,
    pub gateway_enabled: bool,
    pub failover_enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_rpm")]
    pub rpm: u32,
    #[serde(default = "default_tpm")]
    pub tpm: u32,
    #[serde(default = "default_adaptive")]
    pub adaptive: bool,
}

fn default_rpm() -> u32 {
    60
}
fn default_tpm() -> u32 {
    100_000
}
fn default_adaptive() -> bool {
    true
}

impl Default for RateLimitSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            rpm: default_rpm(),
            tpm: default_tpm(),
            adaptive: default_adaptive(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub protocol: ProtocolKind,
    pub default_model: String,
    pub models: Vec<String>,
    pub is_primary: bool,
    pub codex_compat: CodexToolCompat,
    pub reasoning_confidence: ReasoningConfidence,
    /// Whether this upstream returns *signed* Anthropic thinking blocks. Gates
    /// thinking injection in the gateway; `reasoning_confidence` must not, since
    /// it only measures the OpenAI `reasoning_effort` path.
    #[serde(default)]
    pub thinking_support: ThinkingSupport,
    pub accept_invalid_certs: bool,
    pub max_price_per_request: Option<f64>,
    #[serde(default)]
    pub rate_limit: RateLimitSettings,
    #[serde(default)]
    pub supports_1m_context: Option<bool>,
    #[serde(default)]
    pub default_effort_level: Option<String>,
    #[serde(default)]
    pub opus_model: Option<String>,
    #[serde(default)]
    pub sonnet_model: Option<String>,
    #[serde(default)]
    pub haiku_model: Option<String>,
    /// Name Claude Code should *show* for each tier, written into `~/.claude.json`
    /// in place of the bare `opus`/`sonnet`/`haiku` aliases.
    ///
    /// Claude Code only grants a model its real context window, pricing and
    /// feature set when it recognises the name, and the bare aliases resolve
    /// inconsistently across its `/model` picker, `--model` flag and subagent
    /// frontmatter. Naming a current built-in model here makes all three agree.
    /// Only meaningful with the gateway enabled — it is what maps the display
    /// name back to the provider's real model. Empty falls back to the alias.
    #[serde(default)]
    pub opus_display_name: Option<String>,
    #[serde(default)]
    pub sonnet_display_name: Option<String>,
    #[serde(default)]
    pub haiku_display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ProfileCreate {
    pub name: String,
    pub providers: Vec<ProviderConfig>,
    pub clients: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ProfileUpdate {
    pub name: Option<String>,
    pub providers: Option<Vec<ProviderConfig>>,
    pub clients: Option<Vec<String>>,
    pub gateway_enabled: Option<bool>,
    pub failover_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningInfo {
    pub supported: bool,
    pub confidence: ReasoningConfidence,
    pub effort_levels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct InjectionSettings {
    pub enabled: bool,
    pub port_range_start: u16,
    pub port_range_end: u16,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StepwiseSettings {
    pub enabled: bool,
    pub model_override: Option<String>,
    pub suggestion_count: u8,
    pub temperature: f32,
    pub timeout_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub theme: String,
    pub auto_start: bool,
    pub minimize_to_tray: bool,
    pub check_updates: String,
    pub accept_invalid_certs: bool,
    pub generate_only: bool,
    pub injection: InjectionSettings,
    pub stepwise: StepwiseSettings,
    /// Write the forced-Chinese-output rule into each client's global
    /// instructions file. See `crate::client_rules`.
    ///
    /// `serde(default)` is load-bearing, not habit: a state document written
    /// before this field existed has to keep deserializing. Without it the whole
    /// document fails to parse, and `with_state_path`'s `unwrap_or_default()`
    /// would silently discard every profile the user has.
    #[serde(default)]
    pub force_chinese_output: bool,
    /// Write the tool-execution-truthfulness rule into each client's global
    /// instructions file. See `crate::client_rules`.
    ///
    /// `serde(default)` for the same reason as the field above.
    #[serde(default)]
    pub enforce_tool_truthfulness: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            auto_start: false,
            minimize_to_tray: true,
            check_updates: "weekly".into(),
            accept_invalid_certs: false,
            generate_only: true,
            injection: InjectionSettings {
                enabled: false,
                port_range_start: 9222,
                port_range_end: 9322,
                features: vec![],
            },
            stepwise: StepwiseSettings {
                enabled: false,
                model_override: None,
                suggestion_count: 3,
                temperature: 0.7,
                timeout_secs: 30,
            },
            force_chinese_output: false,
            enforce_tool_truthfulness: false,
        }
    }
}

/// Current on-disk schema version. Bumped when [`StateDocument::migrate`] gains a step.
pub const STATE_VERSION: u32 = 2;

/// Persistent state document saved to ~/.ai-deck/state.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateDocument {
    pub version: u32,
    pub profiles: Vec<Profile>,
    /// Which profile each client follows. Replaces the single `activeProfileId`.
    ///
    /// `serde(default)` is load-bearing, not habit — the same reason spelled out on
    /// [`AppSettings::force_chinese_output`]. Every state document written before
    /// this field existed has to keep deserializing; without it the whole document
    /// fails to parse and the `unwrap_or_default()` in both load paths silently
    /// discards every profile the user has, with the `.bak` copy no help because it
    /// has the same old shape.
    #[serde(default)]
    pub bindings: Vec<ClientBinding>,
    /// v1's single active profile, read once by [`StateDocument::migrate`].
    ///
    /// `skip_serializing` so it leaves the file on the first save after migrating;
    /// every later load then finds it absent and `migrate` is a no-op. Kept as a
    /// field rather than deleted outright because seeding bindings from it is the
    /// only way an upgrading user keeps a working setup.
    #[serde(default, rename = "activeProfileId", skip_serializing)]
    pub legacy_active_profile_id: Option<String>,
    pub settings: AppSettings,
}

impl Default for StateDocument {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            profiles: vec![],
            bindings: vec![],
            legacy_active_profile_id: None,
            settings: AppSettings::default(),
        }
    }
}

impl StateDocument {
    /// Bring an older document up to [`STATE_VERSION`]. Returns whether anything changed.
    ///
    /// v1 → v2 expands the one `activeProfileId` into a binding per client in that
    /// profile's `clients` list, which is the reading that keeps an upgrading user's
    /// setup working: those are exactly the clients whose config files that profile
    /// had written.
    ///
    /// Note what this cannot do. The migrated clients' config files still carry the
    /// old bearer, which the gateway will no longer accept, so a caller that sees
    /// `true` has to re-activate the affected profiles to re-issue tokens and rewrite
    /// those files. `ProfileManager::needs_reapply` exists for that.
    fn migrate(&mut self) -> bool {
        if self.version >= STATE_VERSION {
            // Still drop a stale key rather than carry it forever.
            return self.legacy_active_profile_id.take().is_some();
        }

        // Only seed when there is nothing to preserve; a document that already has
        // bindings has been through this.
        if self.bindings.is_empty() {
            if let Some(active_id) = self.legacy_active_profile_id.clone() {
                if let Some(profile) = self.profiles.iter().find(|p| p.id == active_id) {
                    let now = Utc::now().to_rfc3339();
                    self.bindings = crate::binding::normalize_client_ids(&profile.clients)
                        .into_iter()
                        .map(|client_id| ClientBinding {
                            client_id,
                            profile_id: active_id.clone(),
                            bound_at: now.clone(),
                        })
                        .collect();
                }
            }
        }

        self.legacy_active_profile_id = None;
        self.version = STATE_VERSION;
        true
    }
}

/// Profile manager — owns the state document and provides CRUD operations.
pub struct ProfileManager {
    state: StateDocument,
    state_path: PathBuf,
    /// Set when this load converted an older document. See [`ProfileManager::needs_reapply`].
    migrated: bool,
}

impl ProfileManager {
    pub fn with_state_path(state_path: PathBuf) -> Self {
        let mut state: StateDocument = if state_path.exists() {
            let data = storage::read_with_fallback(&state_path).unwrap_or_default();
            serde_json::from_slice(&data).unwrap_or_default()
        } else {
            StateDocument::default()
        };
        let migrated = state.migrate();
        Self {
            state,
            state_path,
            migrated,
        }
    }

    /// Load or initialize the profile manager.
    pub fn load() -> AppResult<Self> {
        let data_dir = data_directory()?;
        std::fs::create_dir_all(&data_dir)?;
        let state_path = data_dir.join("state.json");

        let mut state: StateDocument = if state_path.exists() {
            let data = storage::read_with_fallback(&state_path)?;
            serde_json::from_slice(&data).unwrap_or_default()
        } else {
            StateDocument::default()
        };

        let migrated = state.migrate();
        let manager = Self {
            state,
            state_path,
            migrated,
        };
        // Persist immediately so the legacy key leaves the file even if the caller
        // never writes anything else this run.
        if migrated {
            manager.save()?;
        }
        Ok(manager)
    }

    /// Whether this load converted an older document and the bound clients' config
    /// files are therefore stale.
    ///
    /// Their files still carry the pre-binding bearer, which the gateway no longer
    /// accepts, so a caller seeing `true` must re-activate each bound profile before
    /// the gateway comes up. Skipping that leaves every migrated client on a 401.
    pub fn needs_reapply(&self) -> bool {
        self.migrated && !self.state.bindings.is_empty()
    }

    /// Persist the current state atomically.
    pub fn save(&self) -> AppResult<()> {
        let data = serde_json::to_vec_pretty(&self.state)
            .map_err(|e| AppError::Storage(format!("序列化状态失败：{e}")))?;
        storage::atomic_replace(&self.state_path, &data)
    }

    // --- Profile CRUD ---

    pub fn list_profiles(&self) -> Vec<Profile> {
        self.state.profiles.clone()
    }

    pub fn get_profile(&self, id: &str) -> Option<Profile> {
        self.state.profiles.iter().find(|p| p.id == id).cloned()
    }

    // --- Client bindings ---

    /// Every binding, sorted by client id.
    pub fn bindings(&self) -> Vec<ClientBinding> {
        let mut out = self.state.bindings.clone();
        out.sort_by(|a, b| a.client_id.cmp(&b.client_id));
        out
    }

    /// The profile `client_id` follows, if any.
    pub fn profile_for_client(&self, client_id: &str) -> Option<Profile> {
        let clean = crate::binding::normalize_client_id(client_id);
        self.state
            .bindings
            .iter()
            .find(|b| b.client_id == clean)
            .and_then(|b| self.get_profile(&b.profile_id))
    }

    /// Which clients follow `profile_id`, sorted.
    pub fn clients_for_profile(&self, profile_id: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .state
            .bindings
            .iter()
            .filter(|b| b.profile_id == profile_id)
            .map(|b| b.client_id.clone())
            .collect();
        out.sort();
        out
    }

    /// Whether any binding points at `profile_id`.
    pub fn is_bound(&self, profile_id: &str) -> bool {
        self.state
            .bindings
            .iter()
            .any(|b| b.profile_id == profile_id)
    }

    /// Point `client_ids` at `profile_id`, returning the bindings as stored.
    ///
    /// Each id is first removed from whatever profile held it. That is the map
    /// invariant: one client, one profile. Appending without removing would leave a
    /// client bound twice, and which of the two won would depend on iteration order.
    pub fn bind_clients(
        &mut self,
        profile_id: &str,
        client_ids: &[String],
    ) -> AppResult<Vec<ClientBinding>> {
        if !self.state.profiles.iter().any(|p| p.id == profile_id) {
            return Err(AppError::InvalidInput(format!(
                "Profile {profile_id} 不存在"
            )));
        }
        let clean = crate::binding::normalize_client_ids(client_ids);
        if clean.is_empty() {
            return Ok(vec![]);
        }

        self.state
            .bindings
            .retain(|b| !clean.contains(&b.client_id));
        let now = Utc::now().to_rfc3339();
        let fresh: Vec<ClientBinding> = clean
            .into_iter()
            .map(|client_id| ClientBinding {
                client_id,
                profile_id: profile_id.to_string(),
                bound_at: now.clone(),
            })
            .collect();
        self.state.bindings.extend(fresh.iter().cloned());
        self.save()?;
        Ok(fresh)
    }

    /// Release `client_ids`, returning the ids that had been bound.
    ///
    /// A released client's config file is deliberately left as written. It then
    /// authenticates with a token the gateway no longer knows and gets a 401, which
    /// is loud and fixed by rebinding — quieter than letting it keep reaching a
    /// profile the user thinks it has stopped using.
    pub fn unbind_clients(&mut self, client_ids: &[String]) -> AppResult<Vec<String>> {
        let clean = crate::binding::normalize_client_ids(client_ids);
        let removed: Vec<String> = self
            .state
            .bindings
            .iter()
            .filter(|b| clean.contains(&b.client_id))
            .map(|b| b.client_id.clone())
            .collect();
        if removed.is_empty() {
            return Ok(vec![]);
        }
        self.state
            .bindings
            .retain(|b| !clean.contains(&b.client_id));
        self.save()?;
        Ok(removed)
    }

    /// Whether any bound client is Claude Desktop.
    ///
    /// Drives the teardown decision in `profile_switch`: Desktop goes back to the
    /// user's own Claude account only when *no* binding claims it, not merely when
    /// the profile being activated does not.
    pub fn any_binding_claims_desktop(&self) -> bool {
        self.state
            .bindings
            .iter()
            .any(|b| crate::binding::is_claude_desktop(&b.client_id))
    }

    pub fn duplicate_profile(&mut self, source_id: &str) -> AppResult<Profile> {
        let source = self
            .get_profile(source_id)
            .ok_or_else(|| AppError::InvalidInput(format!("Profile {source_id} 不存在")))?;

        let new_id = Uuid::new_v4().to_string();
        let new_name = format!("{} (副本)", source.name);

        // The copy inherits `clients` as the set it *would* bind, but no binding:
        // those clients still follow the source, and one client cannot follow both.
        let mut duplicated = source.clone();
        duplicated.id = new_id.clone();
        duplicated.name = new_name;
        duplicated.created_at = Utc::now().to_rfc3339();
        duplicated.updated_at = Utc::now().to_rfc3339();

        // Copy API key from credentials if one exists
        if let Ok(key) = credentials::get_api_key(source_id) {
            if !key.trim().is_empty() {
                let _ = credentials::set_api_key(&new_id, &key);
            }
        }

        self.state.profiles.push(duplicated.clone());
        self.save()?;
        Ok(duplicated)
    }

    pub fn create_profile(&mut self, create: ProfileCreate) -> AppResult<Profile> {
        let profile = Profile {
            id: Uuid::new_v4().to_string(),
            name: create.name,
            providers: create.providers,
            clients: create.clients,
            mcp_servers: vec![],
            skills: vec![],
            prompts: vec![],
            gateway_enabled: true,
            failover_enabled: false,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        };
        self.state.profiles.push(profile.clone());
        self.save()?;
        Ok(profile)
    }

    pub fn update_profile(&mut self, id: &str, update: ProfileUpdate) -> AppResult<Profile> {
        let profile = self
            .state
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| AppError::InvalidInput(format!("Profile {id} 不存在")))?;

        if let Some(name) = update.name {
            profile.name = name;
        }
        if let Some(providers) = update.providers {
            profile.providers = providers;
        }
        if let Some(clients) = update.clients {
            profile.clients = clients;
        }
        if let Some(gw) = update.gateway_enabled {
            profile.gateway_enabled = gw;
        }
        if let Some(fo) = update.failover_enabled {
            profile.failover_enabled = fo;
        }
        profile.updated_at = Utc::now().to_rfc3339();

        let result = profile.clone();
        self.save()?;
        Ok(result)
    }

    pub fn delete_profile(&mut self, id: &str) -> AppResult<()> {
        // Deleting a profile a client still follows would leave that client's config
        // file pointing at a profile that no longer exists, so name the clients and
        // let the caller unbind them first.
        let bound = self.clients_for_profile(id);
        if !bound.is_empty() {
            return Err(AppError::InvalidInput(format!(
                "该方案仍绑定着 {}，请先解绑再删除。",
                bound.join("、")
            )));
        }
        self.state.profiles.retain(|p| p.id != id);
        // Clean up credentials
        let _ = credentials::delete_api_key(id);
        self.save()?;
        Ok(())
    }

    // --- Settings ---

    pub fn settings(&self) -> AppSettings {
        self.state.settings.clone()
    }

    pub fn update_settings(&mut self, settings: AppSettings) -> AppResult<()> {
        self.state.settings = settings;
        self.save()?;
        Ok(())
    }

    // --- Export / Import ---

    pub fn export_profile(&self, id: &str) -> AppResult<serde_json::Value> {
        let profile = self
            .get_profile(id)
            .ok_or_else(|| AppError::InvalidInput(format!("Profile {id} 不存在")))?;
        // Nothing to strip: bindings are machine-local and live outside the profile,
        // so an export carries no activation state to begin with.
        serde_json::to_value(&profile).map_err(|e| AppError::Storage(e.to_string()))
    }
}

impl Default for ProfileManager {
    fn default() -> Self {
        let data_dir = data_directory().unwrap_or_else(|_| PathBuf::from(".ai-deck"));
        Self {
            state: StateDocument::default(),
            state_path: data_dir.join("state.json"),
            migrated: false,
        }
    }
}

impl ProfileManager {
    /// Convenience: create a profile with a default starter provider.
    pub fn create_profile_simple(&mut self, name: &str) -> AppResult<Profile> {
        let default_provider = ProviderConfig {
            id: format!("prov_{}", Uuid::new_v4().simple()),
            name: format!("{name} 节点"),
            base_url: "https://api.openai.com/v1".into(),
            protocol: ProtocolKind::OpenAI,
            default_model: "gpt-4o".into(),
            models: vec!["gpt-4o".into()],
            is_primary: true,
            codex_compat: CodexToolCompat::ResponsesCustom,
            reasoning_confidence: ReasoningConfidence::Validated,
            thinking_support: ThinkingSupport::Unprobed,
            accept_invalid_certs: false,
            max_price_per_request: None,
            rate_limit: RateLimitSettings::default(),
            supports_1m_context: None,
            default_effort_level: None,
            opus_model: None,
            sonnet_model: None,
            haiku_model: None,
            opus_display_name: None,
            sonnet_display_name: None,
            haiku_display_name: None,
        };
        self.create_profile(ProfileCreate {
            name: name.to_string(),
            providers: vec![default_provider],
            clients: vec!["codex-cli".into(), "claude-code".into()],
        })
    }
}

/// Get the application data directory (~/.ai-deck/).
pub fn data_directory() -> AppResult<PathBuf> {
    let home =
        crate::user_home_dir().ok_or_else(|| AppError::Config("无法确定用户主目录".into()))?;
    Ok(home.join(".ai-deck"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_pm() -> (ProfileManager, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let state_path = dir.path().join(".ai-deck").join("state.json");
        let pm = ProfileManager::with_state_path(state_path);
        (pm, dir)
    }

    #[test]
    fn test_duplicate_profile() {
        let (mut pm, _dir) = test_pm();
        let created = pm.create_profile_simple("主方案").unwrap();
        let dup = pm.duplicate_profile(&created.id).unwrap();

        assert_ne!(dup.id, created.id);
        assert_eq!(dup.name, "主方案 (副本)");
        assert_eq!(dup.providers.len(), created.providers.len());
        assert_eq!(dup.clients.len(), created.clients.len());
        // The copy inherits the target list but not the bindings: one client cannot
        // follow both, and silently moving them to the copy would repoint a working
        // setup at a profile the user has not configured yet.
        assert!(
            pm.clients_for_profile(&dup.id).is_empty(),
            "副本不应继承绑定"
        );
    }

    #[test]
    fn test_only_active_profile_takes_effect_and_no_interference() {
        let (mut pm, _dir) = test_pm();

        // 1. Create Profile A and Profile B
        let p_a = pm.create_profile_simple("方案A").unwrap();
        let p_b = pm.create_profile_simple("方案B").unwrap();
        let p_c = pm.create_profile_simple("方案C").unwrap();

        // 2. Bind both clients to Profile A
        pm.bind_clients(&p_a.id, &["codex-cli".into(), "claude-code".into()])
            .unwrap();
        assert_eq!(
            pm.profile_for_client("codex-cli").unwrap().id,
            p_a.id,
            "codex-cli 应跟随方案A"
        );
        assert_eq!(
            pm.clients_for_profile(&p_a.id),
            vec!["claude-code".to_string(), "codex-cli".to_string()]
        );
        assert!(
            pm.clients_for_profile(&p_b.id).is_empty(),
            "方案B 不应有绑定"
        );
        assert!(
            pm.clients_for_profile(&p_c.id).is_empty(),
            "方案C 不应有绑定"
        );

        // 3. Modifying inactive Profile B should NOT affect active Profile A
        let updated_b = pm
            .update_profile(
                &p_b.id,
                ProfileUpdate {
                    name: Some("方案B修改版".into()),
                    providers: Some(vec![ProviderConfig {
                        id: "prov_b_mod".into(),
                        name: "自定义节点B".into(),
                        base_url: "https://api.example-b.com/v1".into(),
                        protocol: ProtocolKind::OpenAI,
                        default_model: "custom-model-b".into(),
                        models: vec!["custom-model-b".into()],
                        is_primary: true,
                        codex_compat: CodexToolCompat::ResponsesCustom,
                        reasoning_confidence: ReasoningConfidence::Validated,
                        thinking_support: ThinkingSupport::Unprobed,
                        accept_invalid_certs: false,
                        max_price_per_request: None,
                        rate_limit: RateLimitSettings::default(),
                        supports_1m_context: None,
                        default_effort_level: None,
                        opus_model: None,
                        sonnet_model: None,
                        haiku_model: None,
                        opus_display_name: None,
                        sonnet_display_name: None,
                        haiku_display_name: None,
                    }]),
                    clients: None,
                    gateway_enabled: Some(false),
                    failover_enabled: None,
                },
            )
            .unwrap();

        assert_eq!(updated_b.name, "方案B修改版");
        assert!(
            pm.clients_for_profile(&p_b.id).is_empty(),
            "修改未绑定的方案B 不应让它获得绑定"
        );
        assert_eq!(
            pm.profile_for_client("codex-cli").unwrap().id,
            p_a.id,
            "codex-cli 仍应跟随方案A"
        );

        // 4. Duplicating B must not move any binding onto the copy
        let p_d = pm.duplicate_profile(&p_b.id).unwrap();
        assert!(pm.clients_for_profile(&p_d.id).is_empty(), "副本不应有绑定");
        assert_eq!(pm.profile_for_client("claude-code").unwrap().id, p_a.id);

        // 5. Deleting an unbound profile succeeds and disturbs nothing
        assert!(
            pm.delete_profile(&p_c.id).is_ok(),
            "删除未绑定方案C应当成功"
        );
        assert_eq!(pm.profile_for_client("codex-cli").unwrap().id, p_a.id);

        // 6. Deleting a profile clients still follow is refused, and says which
        let err = pm.delete_profile(&p_a.id).unwrap_err().to_string();
        assert!(err.contains("codex-cli"), "报错要点出还绑着谁，实际：{err}");
        assert!(err.contains("claude-code"), "报错要列出全部，实际：{err}");

        // 7. Move only codex-cli to B. This is the whole point of bindings: the two
        // clients now follow different profiles at the same time.
        pm.bind_clients(&p_b.id, &["codex-cli".into()]).unwrap();
        assert_eq!(pm.profile_for_client("codex-cli").unwrap().id, p_b.id);
        assert_eq!(
            pm.profile_for_client("claude-code").unwrap().id,
            p_a.id,
            "只移动 codex-cli 不应带走 claude-code"
        );
        assert_eq!(
            pm.clients_for_profile(&p_a.id),
            vec!["claude-code".to_string()]
        );
        assert_eq!(
            pm.clients_for_profile(&p_b.id),
            vec!["codex-cli".to_string()]
        );

        // 8. Rebinding moves rather than copies, so a client is never bound twice
        assert_eq!(
            pm.bindings()
                .iter()
                .filter(|b| b.client_id == "codex-cli")
                .count(),
            1,
            "一个客户端只能有一条绑定"
        );

        // 9. Releasing the last client frees the profile for deletion
        pm.unbind_clients(&["claude-code".into()]).unwrap();
        assert!(pm.profile_for_client("claude-code").is_none());
        assert!(pm.delete_profile(&p_a.id).is_ok(), "解绑后应可删除");
    }

    /// A v1 document's single active profile becomes one binding per client in that
    /// profile's list — those are exactly the clients whose configs it had written.
    #[test]
    fn migration_expands_the_active_profile_into_per_client_bindings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        // Ids deliberately mis-cased and padded, as a hand-edited file might be.
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "version": 1,
                "profiles": [{
                    "id": "prof_old",
                    "name": "旧方案",
                    "isActive": true,
                    "providers": [],
                    "clients": ["Codex-CLI", " claude-code"],
                    "mcpServers": [],
                    "skills": [],
                    "prompts": [],
                    "gatewayEnabled": true,
                    "failoverEnabled": false,
                    "createdAt": "2026-01-01T00:00:00Z",
                    "updatedAt": "2026-01-01T00:00:00Z",
                }],
                "activeProfileId": "prof_old",
                "settings": AppSettings::default(),
            }))
            .unwrap(),
        )
        .unwrap();

        let pm = ProfileManager::with_state_path(path.clone());
        assert_eq!(
            pm.clients_for_profile("prof_old"),
            vec!["claude-code".to_string(), "codex-cli".to_string()],
            "两个客户端都应迁成绑定，且 id 已归一"
        );
        assert!(
            pm.needs_reapply(),
            "迁移后必须提示重新下发，否则客户端配置里还是旧 bearer"
        );

        // The legacy key must leave the file, so a later load is a no-op.
        pm.save().unwrap();
        let raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(
            raw.get("activeProfileId").is_none(),
            "保存后磁盘上不应再有 activeProfileId"
        );
        assert_eq!(raw["version"], STATE_VERSION);
        let again = ProfileManager::with_state_path(path);
        assert!(!again.needs_reapply(), "第二次加载不该再报需要重新下发");
    }

    /// The single most dangerous case: both load paths use
    /// `serde_json::from_slice(...).unwrap_or_default()`, so a field without
    /// `serde(default)` turns every older document into an empty state.
    #[test]
    fn a_document_without_bindings_keeps_its_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "version": 2,
                "profiles": [{
                    "id": "prof_keep",
                    "name": "要保住的方案",
                    "providers": [],
                    "clients": [],
                    "mcpServers": [],
                    "skills": [],
                    "prompts": [],
                    "gatewayEnabled": true,
                    "failoverEnabled": false,
                    "createdAt": "2026-01-01T00:00:00Z",
                    "updatedAt": "2026-01-01T00:00:00Z",
                }],
                "settings": AppSettings::default(),
            }))
            .unwrap(),
        )
        .unwrap();

        let pm = ProfileManager::with_state_path(path);
        assert_eq!(pm.list_profiles().len(), 1, "缺 bindings 字段不能吃掉方案");
        assert_eq!(pm.list_profiles()[0].name, "要保住的方案");
        assert!(pm.bindings().is_empty());
    }

    /// A dangling `activeProfileId` is not an error — nothing was in effect, so
    /// there is nothing to preserve.
    #[test]
    fn migration_tolerates_an_active_id_with_no_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "version": 1,
                "profiles": [],
                "activeProfileId": "prof_deleted",
                "settings": AppSettings::default(),
            }))
            .unwrap(),
        )
        .unwrap();

        let pm = ProfileManager::with_state_path(path);
        assert!(pm.bindings().is_empty());
        assert!(!pm.needs_reapply(), "没有绑定就没有要重新下发的东西");
    }

    /// An active profile that selected no clients yields no bindings, so nothing
    /// routes for it. A real behavior change from v1, where it still counted as
    /// active, and the UI has to say so.
    #[test]
    fn migration_of_a_clientless_active_profile_binds_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "version": 1,
                "profiles": [{
                    "id": "prof_bare",
                    "name": "没选客户端",
                    "providers": [],
                    "clients": [],
                    "mcpServers": [],
                    "skills": [],
                    "prompts": [],
                    "gatewayEnabled": true,
                    "failoverEnabled": false,
                    "createdAt": "2026-01-01T00:00:00Z",
                    "updatedAt": "2026-01-01T00:00:00Z",
                }],
                "activeProfileId": "prof_bare",
                "settings": AppSettings::default(),
            }))
            .unwrap(),
        )
        .unwrap();

        let pm = ProfileManager::with_state_path(path);
        assert!(pm.bindings().is_empty());
        assert!(!pm.needs_reapply());
    }

    /// Desktop drives teardown, so the predicate has to see through a suffixed id.
    #[test]
    fn desktop_claim_is_visible_across_bindings() {
        let (mut pm, _dir) = test_pm();
        let p = pm.create_profile_simple("桌面方案").unwrap();
        assert!(!pm.any_binding_claims_desktop());

        pm.bind_clients(&p.id, &["claude-desktop".into()]).unwrap();
        assert!(pm.any_binding_claims_desktop());

        pm.unbind_clients(&["claude-desktop".into()]).unwrap();
        assert!(
            !pm.any_binding_claims_desktop(),
            "解绑后不应还认为有人占着 Desktop"
        );
    }

    #[test]
    fn binding_an_unknown_profile_is_refused() {
        let (mut pm, _dir) = test_pm();
        assert!(pm
            .bind_clients("prof_missing", &["codex-cli".into()])
            .is_err());
    }
}
