//! File system watcher for chat session files.
//! Monitors Claude Code and Codex session directories for changes,
//! triggering incremental re-indexing.

use crate::error::{AppError, AppResult};
use std::path::PathBuf;

pub struct HistoryWatcher {
    watched_dirs: Vec<PathBuf>,
}

impl HistoryWatcher {
    pub fn new() -> Self {
        Self { watched_dirs: vec![] }
    }

    pub fn start(&mut self) -> AppResult<()> {
        let home = dirs::home_dir()
            .ok_or_else(|| AppError::Config("无法确定用户主目录".into()))?;
        
        // Watch Claude Code sessions
        let claude_dir = home.join(".claude").join("projects");
        if claude_dir.exists() {
            self.watched_dirs.push(claude_dir);
        }

        // Watch Codex sessions  
        let codex_dir = home.join(".codex").join("sessions");
        if codex_dir.exists() {
            self.watched_dirs.push(codex_dir);
        }

        tracing::info!("历史监听器已启动，监控 {} 个目录", self.watched_dirs.len());
        Ok(())
    }

    pub fn stop(&mut self) {
        self.watched_dirs.clear();
        tracing::info!("历史监听器已停止");
    }
}
