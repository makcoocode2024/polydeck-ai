//! Codex++ native user-script installation.
//!
//! Only AI Deck-owned files and manifest entries are touched. Third-party scripts
//! and market metadata remain byte-for-byte represented in the JSON document.

use polydeck_core::storage;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

pub const SCRIPT_FILE_NAME: &str = "ai-deck-bridge.user.js";
pub const SCRIPT_KEY: &str = "user:ai-deck-bridge.user.js";
pub const OWNERSHIP_FILE_NAME: &str = "ai-deck-injection.json";
const OWNERSHIP_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum ScriptManagerError {
    #[error("Codex++ user script manifest is malformed")]
    MalformedManifest,
    #[error("AI Deck script ownership does not match this installation")]
    OwnershipMismatch,
    #[error("selector override is invalid: {0}")]
    InvalidSelectorOverride(String),
    #[error("storage error: {0}")]
    Storage(#[from] polydeck_core::AppError),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type ScriptManagerResult<T> = Result<T, ScriptManagerError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptPaths {
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub scripts_dir: PathBuf,
    pub script: PathBuf,
    pub ownership: PathBuf,
}

impl ScriptPaths {
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let scripts_dir = root.join("user_scripts");
        Self {
            manifest: root.join("user_scripts.json"),
            script: scripts_dir.join(SCRIPT_FILE_NAME),
            ownership: root.join(OWNERSHIP_FILE_NAME),
            root, scripts_dir,
        }
    }
    pub fn platform_default() -> Option<Self> {
        if cfg!(target_os = "windows") {
            dirs::config_dir().map(|p| Self::from_root(p.join("Codex++")))
        } else if cfg!(target_os = "macos") {
            dirs::home_dir().map(|p| Self::from_root(p.join("Library").join("Application Support").join("Codex++")))
        } else {
            dirs::config_dir().map(|p| Self::from_root(p.join("Codex++")))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptStatus {
    pub available: bool,
    pub installed: bool,
    pub enabled: bool,
    pub healthy: bool,
    pub restart_required: bool,
    pub script_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct OwnershipManifest { version: u32, script_key: String, script_hash: String }

pub struct ScriptManager { paths: ScriptPaths, script_source: String }

impl ScriptManager {
    pub fn new(paths: ScriptPaths, script_source: impl Into<String>) -> Self {
        Self { paths, script_source: script_source.into() }
    }
    pub fn paths(&self) -> &ScriptPaths { &self.paths }
    pub fn set_script_source(&mut self, source: impl Into<String>) { self.script_source = source.into(); }

    pub fn install(&self) -> ScriptManagerResult<ScriptStatus> {
        self.write_owned_script()?;
        self.update_manifest(true)?;
        self.write_ownership()?;
        self.status()
    }
    pub fn enable(&self) -> ScriptManagerResult<ScriptStatus> { self.require_owned_script()?; self.update_manifest(true)?; self.status() }
    pub fn disable(&self) -> ScriptManagerResult<ScriptStatus> { self.require_owned_script()?; self.update_manifest(false)?; self.status() }
    pub fn uninstall(&self) -> ScriptManagerResult<ScriptStatus> {
        if self.paths.script.exists() { self.require_owned_script()?; fs::remove_file(&self.paths.script)?; }
        if self.paths.ownership.exists() { fs::remove_file(&self.paths.ownership)?; }
        self.remove_manifest_entry()?;
        self.status()
    }
    pub fn repair(&self) -> ScriptManagerResult<ScriptStatus> {
        if self.paths.script.exists() { self.require_owned_script()?; }
        self.write_owned_script()?; self.update_manifest(true)?; self.write_ownership()?;
        self.status()
    }
    pub fn status(&self) -> ScriptManagerResult<ScriptStatus> {
        let available = self.paths.root.exists() || self.paths.manifest.exists();
        let installed = self.paths.script.exists() && self.paths.ownership.exists();
        let expected_hash = hash(&self.script_source);
        let actual_hash = fs::read_to_string(&self.paths.script).ok().map(|s| hash(&s));
        let ownership = self.read_ownership().ok();
        let healthy = installed
            && actual_hash.as_deref() == Some(expected_hash.as_str())
            && ownership.as_ref().map(|m| m.script_hash.as_str()) == Some(expected_hash.as_str());
        let enabled = self.read_manifest().ok()
            .and_then(|v| v.get("scripts")?.get(SCRIPT_KEY)?.as_bool()).unwrap_or(false);
        Ok(ScriptStatus { available, installed, enabled, healthy, restart_required: installed, script_hash: actual_hash })
    }

    fn write_owned_script(&self) -> ScriptManagerResult<()> {
        storage::atomic_write(&self.paths.script, self.script_source.as_bytes())?; Ok(())
    }
    fn write_ownership(&self) -> ScriptManagerResult<()> {
        storage::atomic_write_json(&self.paths.ownership, &OwnershipManifest {
            version: OWNERSHIP_VERSION, script_key: SCRIPT_KEY.to_string(), script_hash: hash(&self.script_source),
        })?; Ok(())
    }
    fn require_owned_script(&self) -> ScriptManagerResult<()> {
        let ownership = self.read_ownership()?;
        if ownership.version != OWNERSHIP_VERSION || ownership.script_key != SCRIPT_KEY
            || ownership.script_hash != hash(&self.script_source)
            || fs::read_to_string(&self.paths.script).map(|s| hash(&s)).ok().as_deref() != Some(ownership.script_hash.as_str())
        { return Err(ScriptManagerError::OwnershipMismatch); }
        Ok(())
    }
    fn read_ownership(&self) -> ScriptManagerResult<OwnershipManifest> {
        let bytes = fs::read(&self.paths.ownership)?;
        serde_json::from_slice(&bytes).map_err(ScriptManagerError::Json)
    }
    fn read_manifest(&self) -> ScriptManagerResult<Value> {
        if !self.paths.manifest.exists() { return Ok(default_manifest()); }
        let value: Value = serde_json::from_slice(&fs::read(&self.paths.manifest)?)?;
        validate_manifest(&value)?; Ok(value)
    }
    fn update_manifest(&self, enabled: bool) -> ScriptManagerResult<()> {
        let mut manifest = self.read_manifest()?;
        let scripts = manifest.as_object_mut().ok_or(ScriptManagerError::MalformedManifest)?
            .entry("scripts").or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut().ok_or(ScriptManagerError::MalformedManifest)?;
        scripts.insert(SCRIPT_KEY.to_string(), Value::Bool(enabled));
        storage::atomic_write_json(&self.paths.manifest, &manifest)?; Ok(())
    }
    fn remove_manifest_entry(&self) -> ScriptManagerResult<()> {
        if !self.paths.manifest.exists() { return Ok(()); }
        let mut manifest = self.read_manifest()?;
        if let Some(scripts) = manifest.get_mut("scripts").and_then(Value::as_object_mut) { scripts.remove(SCRIPT_KEY); }
        storage::atomic_write_json(&self.paths.manifest, &manifest)?; Ok(())
    }
}

pub fn validate_selector_overrides(value: &Value) -> ScriptManagerResult<BTreeMap<String, String>> {
    let object = value.as_object().ok_or_else(|| ScriptManagerError::InvalidSelectorOverride("root must be an object".into()))?;
    let allowed = ["pluginMarketUnlock", "sessionActions", "uiOptimization", "stepwise", "developerWorkflow", "userScripts"];
    let mut overrides = BTreeMap::new();
    for (feature, selector) in object {
        if !allowed.contains(&feature.as_str()) { return Err(ScriptManagerError::InvalidSelectorOverride(format!("unknown feature '{feature}'"))); }
        let selector = selector.as_str().ok_or_else(|| ScriptManagerError::InvalidSelectorOverride(format!("'{feature}' must be a string")))?;
        if selector.is_empty() || selector.len() > 512 || selector.contains('\0') { return Err(ScriptManagerError::InvalidSelectorOverride(format!("'{feature}' has an unsafe selector"))); }
        overrides.insert(feature.clone(), selector.to_string());
    }
    Ok(overrides)
}

fn validate_manifest(value: &Value) -> ScriptManagerResult<()> {
    let object = value.as_object().ok_or(ScriptManagerError::MalformedManifest)?;
    for key in ["enabled", "scripts", "market"] {
        if let Some(v) = object.get(key) {
            let valid = match key { "enabled" => v.is_boolean(), _ => v.is_object() };
            if !valid { return Err(ScriptManagerError::MalformedManifest); }
        }
    }
    Ok(())
}

fn default_manifest() -> Value { serde_json::json!({"enabled": true, "scripts": {}, "market": {}}) }
fn hash(source: &str) -> String { format!("{:x}", Sha256::digest(source.as_bytes())) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_layout_is_scoped() {
        let paths = ScriptPaths::from_root("test-root");
        assert_eq!(paths.manifest, PathBuf::from("test-root/user_scripts.json"));
    }

    #[test]
    fn selector_override_accepts_known_features() {
        let result = validate_selector_overrides(&serde_json::json!({"stepwise":"[data-input]"})).unwrap();
        assert_eq!(result["stepwise"], "[data-input]");
    }

    #[test]
    fn selector_override_rejects_unknown_feature() {
        assert!(validate_selector_overrides(&serde_json::json!({"__proto__":"x"})).is_err());
    }
}
