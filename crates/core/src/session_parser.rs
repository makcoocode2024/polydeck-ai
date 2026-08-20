//! Session file parsers for Claude Code and Codex JSONL formats.

use serde_json::Value;

pub mod claude_code {
    use super::*;

    pub struct ClaudeCodeParser;
    impl ClaudeCodeParser {
        pub fn parse(content: &str) -> Result<Vec<Value>, String> {
            let mut entries = Vec::new();
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
                    entries.push(v);
                }
            }
            Ok(entries)
        }
    }
}

pub mod codex_jsonl {
    use super::*;

    pub struct CodexJsonlParser;
    impl CodexJsonlParser {
        pub fn parse(content: &str) -> Result<Vec<Value>, String> {
            let mut entries = Vec::new();
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
                    entries.push(v);
                }
            }
            Ok(entries)
        }
    }
}

pub trait SessionParser {
    fn parse(content: &str) -> Result<Vec<Value>, String>;
}
