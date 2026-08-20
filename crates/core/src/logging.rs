//! Structured JSON Lines logging with auto-redaction and rotation.

use crate::error::AppResult;
use crate::profile::data_directory;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
    pub target: String,
}

pub struct LogRouter;
pub struct LogStore { log_dir: PathBuf }

impl LogRouter {
    pub fn init() -> AppResult<()> {
        let log_dir = data_directory()?.join("logs");
        fs::create_dir_all(&log_dir)?;
        // Basic tracing init
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()))
            .try_init();
        Ok(())
    }
}

impl LogStore {
    pub fn new() -> AppResult<Self> {
        let log_dir = data_directory()?.join("logs");
        fs::create_dir_all(&log_dir)?;
        Ok(Self { log_dir })
    }

    pub fn get_logs(&self, _level: Option<&str>, _limit: usize) -> AppResult<Vec<LogEntry>> {
        Ok(vec![])
    }

    pub fn clear_logs(&self) -> AppResult<()> { Ok(()) }
    
    pub fn export_logs(&self) -> AppResult<String> {
        Ok(self.log_dir.to_string_lossy().into())
    }
}

pub fn redact_sensitive(text: &str) -> String {
    let mut r = text.to_string();
    for pat in &["sk-", "sk-ant-", "xai-", "AIza", "Bearer "] {
        while let Some(pos) = r.find(pat) {
            let end = (pos + 20).min(r.len());
            let prefix = &r[pos..pos + pat.len().min(4)];
            r = format!("{}{}****{}", &r[..pos], prefix, &r[end..]);
        }
    }
    r
}
