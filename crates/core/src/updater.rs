//! Auto-update checker: daily/weekly/disabled, GitHub Release check.

use crate::error::AppResult;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum UpdateFrequency {
    Daily,
    Weekly,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConfig {
    pub frequency: UpdateFrequency,
    pub last_check: Option<String>,
}

pub struct UpdateStore {
    config: UpdateConfig,
}

impl Default for UpdateStore {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateStore {
    pub fn new() -> Self {
        Self {
            config: UpdateConfig {
                frequency: UpdateFrequency::Weekly,
                last_check: None,
            },
        }
    }

    pub fn should_check(&self) -> bool {
        matches!(
            self.config.frequency,
            UpdateFrequency::Daily | UpdateFrequency::Weekly
        )
    }

    /// Not implemented.
    ///
    /// Returned `Ok(None)` before, which the Settings page renders as "已是最新
    /// 稳定版本" — a claim nothing had checked. An error is the honest answer until
    /// this queries the releases API.
    pub async fn check_for_update(&mut self) -> AppResult<Option<String>> {
        self.config.last_check = Some(chrono::Utc::now().to_rfc3339());
        Err(crate::error::AppError::Internal(
            "自动更新检查尚未实现，请前往 GitHub Releases 手动查看".to_string(),
        ))
    }
}
