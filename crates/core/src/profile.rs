//! Profile management — multi-provider configuration sets.
//!
//! A Profile groups one or more provider configurations, client targets,
//! MCP servers, skills, and prompts into a switchable unit. Switching a
//! Profile atomically writes all client configs and syncs extensions.

use crate::credentials;
use crate::error::{AppError, AppResult};
use crate::storage;
use crate::types::{CodexToolCompat, ProtocolKind, ReasoningConfidence};
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
    pub is_active: bool,
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
        }
    }
}

/// Persistent state document saved to ~/.ai-deck/state.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateDocument {
    pub version: u32,
    pub profiles: Vec<Profile>,
    pub active_profile_id: Option<String>,
    pub settings: AppSettings,
}

impl Default for StateDocument {
    fn default() -> Self {
        Self {
            version: 1,
            profiles: vec![],
            active_profile_id: None,
            settings: AppSettings::default(),
        }
    }
}

/// Profile manager — owns the state document and provides CRUD operations.
pub struct ProfileManager {
    state: StateDocument,
    state_path: PathBuf,
}

impl ProfileManager {
    pub fn with_state_path(state_path: PathBuf) -> Self {
        let state = if state_path.exists() {
            let data = storage::read_with_fallback(&state_path).unwrap_or_default();
            serde_json::from_slice(&data).unwrap_or_default()
        } else {
            StateDocument::default()
        };
        Self { state, state_path }
    }

    /// Load or initialize the profile manager.
    pub fn load() -> AppResult<Self> {
        let data_dir = data_directory()?;
        std::fs::create_dir_all(&data_dir)?;
        let state_path = data_dir.join("state.json");

        let state = if state_path.exists() {
            let data = storage::read_with_fallback(&state_path)?;
            serde_json::from_slice(&data).unwrap_or_default()
        } else {
            StateDocument::default()
        };

        Ok(Self { state, state_path })
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

    pub fn active_profile(&self) -> Option<Profile> {
        self.state
            .active_profile_id
            .as_ref()
            .and_then(|id| self.get_profile(id))
    }

    pub fn duplicate_profile(&mut self, source_id: &str) -> AppResult<Profile> {
        let source = self
            .get_profile(source_id)
            .ok_or_else(|| AppError::InvalidInput(format!("Profile {source_id} 不存在")))?;

        let new_id = Uuid::new_v4().to_string();
        let new_name = format!("{} (副本)", source.name);

        let mut duplicated = source.clone();
        duplicated.id = new_id.clone();
        duplicated.name = new_name;
        duplicated.is_active = false;
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
            is_active: false,
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
        // Don't allow deleting the active profile
        if self.state.active_profile_id.as_deref() == Some(id) {
            return Err(AppError::InvalidInput(
                "不能删除当前生效的 Profile。请先切换到其他 Profile。".into(),
            ));
        }
        self.state.profiles.retain(|p| p.id != id);
        // Clean up credentials
        let _ = credentials::delete_api_key(id);
        self.save()?;
        Ok(())
    }

    pub fn set_active(&mut self, id: &str) -> AppResult<()> {
        if !self.state.profiles.iter().any(|p| p.id == id) {
            return Err(AppError::InvalidInput(format!("Profile {id} 不存在")));
        }
        // Deactivate all
        for p in &mut self.state.profiles {
            p.is_active = p.id == id;
        }
        self.state.active_profile_id = Some(id.to_string());
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
        // Strip sensitive fields
        let mut value =
            serde_json::to_value(&profile).map_err(|e| AppError::Storage(e.to_string()))?;
        if let Some(obj) = value.as_object_mut() {
            obj.remove("isActive");
        }
        Ok(value)
    }
}

impl Default for ProfileManager {
    fn default() -> Self {
        let data_dir = data_directory().unwrap_or_else(|_| PathBuf::from(".ai-deck"));
        Self {
            state: StateDocument::default(),
            state_path: data_dir.join("state.json"),
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
        assert!(!dup.is_active);
        assert_eq!(dup.providers.len(), created.providers.len());
        assert_eq!(dup.clients.len(), created.clients.len());
    }

    #[test]
    fn test_only_active_profile_takes_effect_and_no_interference() {
        let (mut pm, _dir) = test_pm();

        // 1. Create Profile A and Profile B
        let p_a = pm.create_profile_simple("方案A").unwrap();
        let p_b = pm.create_profile_simple("方案B").unwrap();
        let p_c = pm.create_profile_simple("方案C").unwrap();

        // 2. Activate Profile A
        pm.set_active(&p_a.id).unwrap();
        assert_eq!(pm.active_profile().unwrap().id, p_a.id);

        let list1 = pm.list_profiles();
        let a_active = list1.iter().find(|p| p.id == p_a.id).unwrap();
        let b_inactive = list1.iter().find(|p| p.id == p_b.id).unwrap();
        let c_inactive = list1.iter().find(|p| p.id == p_c.id).unwrap();
        assert!(a_active.is_active, "方案A 必须处于激活状态");
        assert!(!b_inactive.is_active, "方案B 必须处于未激活状态");
        assert!(!c_inactive.is_active, "方案C 必须处于未激活状态");

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
            !updated_b.is_active,
            "修改未激活方案B后，其状态仍应保持未激活"
        );
        assert_eq!(
            pm.active_profile().unwrap().id,
            p_a.id,
            "当前生效激活方案依然是方案A"
        );

        // 4. Duplicate inactive Profile B should create an inactive Profile D
        let p_d = pm.duplicate_profile(&p_b.id).unwrap();
        assert!(!p_d.is_active, "复制生成的方案副本必须处于未激活状态");
        assert_eq!(
            pm.active_profile().unwrap().id,
            p_a.id,
            "激活方案仍保持方案A"
        );

        // 5. Deleting inactive Profile C should succeed without disturbing active Profile A
        let del_res = pm.delete_profile(&p_c.id);
        assert!(del_res.is_ok(), "删除未激活方案C应当成功");
        assert_eq!(
            pm.active_profile().unwrap().id,
            p_a.id,
            "激活方案仍保持方案A"
        );

        // 6. Attempting to delete active Profile A should be safely rejected
        let del_active_res = pm.delete_profile(&p_a.id);
        assert!(del_active_res.is_err(), "删除当前激活方案A必须被安全拒绝");

        // 7. Switch active from Profile A to Profile B
        pm.set_active(&p_b.id).unwrap();
        assert_eq!(
            pm.active_profile().unwrap().id,
            p_b.id,
            "激活方案应切换为方案B"
        );

        let list2 = pm.list_profiles();
        let a_now_inactive = list2.iter().find(|p| p.id == p_a.id).unwrap();
        let b_now_active = list2.iter().find(|p| p.id == p_b.id).unwrap();
        assert!(!a_now_inactive.is_active, "切换后方案A必须变为未激活");
        assert!(b_now_active.is_active, "切换后方案B必须变为激活");
    }
}
