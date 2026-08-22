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

    pub async fn check_for_update(&mut self) -> AppResult<Option<String>> {
        // TODO: Check GitHub releases API
        self.config.last_check = Some(chrono::Utc::now().to_rfc3339());
        Ok(None)
    }
}
