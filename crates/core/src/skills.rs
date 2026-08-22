//! Skills management: built-in + GitHub-installed, per-Profile sync.

use crate::error::AppResult;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ManagedSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub enum SkillSource {
    Builtin,
    GitHub { repo: String, path: Option<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct GithubSkillSpec {
    pub repo: String,
    pub branch: Option<String>,
    pub path: Option<String>,
}

pub fn builtin_skills() -> Vec<ManagedSkill> {
    vec![]
}

pub fn install_from_github(_spec: &GithubSkillSpec) -> AppResult<ManagedSkill> {
    Err(crate::error::AppError::Internal(
        "GitHub 技能安装尚未实现".into(),
    ))
}
