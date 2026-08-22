//! Unified chat history storage and indexing for multiple AI clients.
//!
//! Indexes local conversation logs from Codex CLI, Claude Code, Hermes, and Claude Desktop,
//! providing fast SQLite querying, usage statistics, export, and encrypted backups.

use crate::error::{AppError, AppResult};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub client: String,
    pub title: String,
    pub message_count: usize,
    pub total_tokens: usize,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct HistoryQuery {
    pub client: Option<String>,
    pub provider: Option<String>,
    pub search: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub page: usize,
    pub page_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub items: Vec<SessionSummary>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct UsageStats {
    pub total_sessions: usize,
    pub total_messages: usize,
    pub total_tokens: usize,
    pub sessions_by_client: HashMap<String, usize>,
    pub sessions_by_date: Vec<(String, usize)>,
}

pub struct HistoryStore {
    conn: Connection,
}

impl HistoryStore {
    pub fn open() -> AppResult<Self> {
        if let Ok(db_path) = get_db_path() {
            if let Some(parent) = db_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(conn) = Connection::open(&db_path) {
                let store = Self { conn };
                if store.init_tables().is_ok() {
                    let _ = store.sync_all();
                    return Ok(store);
                }
            }
        }
        if let Some(h) = dirs::home_dir() {
            let fallback_db = h.join(".ai-deck").join("history.db");
            if let Some(parent) = fallback_db.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(conn) = Connection::open(&fallback_db) {
                let store = Self { conn };
                if store.init_tables().is_ok() {
                    let _ = store.sync_all();
                    return Ok(store);
                }
            }
        }
        let temp_db = std::env::temp_dir().join("ai-deck-history.db");
        if let Ok(conn) = Connection::open(&temp_db) {
            let store = Self { conn };
            if store.init_tables().is_ok() {
                let _ = store.sync_all();
                return Ok(store);
            }
        }
        let store = Self::open_in_memory()?;
        let _ = store.sync_all();
        Ok(store)
    }

    pub fn open_in_memory() -> AppResult<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| AppError::Storage(format!("创建内存数据库失败: {e}")))?;
        let store = Self { conn };
        store.init_tables()?;
        Ok(store)
    }

    fn init_tables(&self) -> AppResult<()> {
        let _ = self.conn.execute("PRAGMA journal_mode = WAL;", []);
        let _ = self.conn.execute("PRAGMA synchronous = NORMAL;", []);

        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY,
                    client TEXT NOT NULL,
                    title TEXT NOT NULL,
                    message_count INTEGER NOT NULL,
                    total_tokens INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    raw_path TEXT
                )",
                [],
            )
            .map_err(|e| AppError::Storage(format!("创建数据表失败: {e}")))?;

        let _ = self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_client ON sessions(client)",
            [],
        );
        let _ = self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_updated ON sessions(updated_at DESC)",
            [],
        );
        Ok(())
    }

    pub fn sync_all(&self) -> AppResult<usize> {
        let mut all_summaries = Vec::new();
        for home in crate::candidate_home_dirs() {
            // 1. Codex sessions
            let codex_sessions = home.join(".codex").join("sessions");
            if codex_sessions.exists() {
                self.collect_codex_summaries(&codex_sessions, &mut all_summaries);
            }
            let codex_archived = home.join(".codex").join("archived_sessions");
            if codex_archived.exists() {
                self.collect_codex_summaries(&codex_archived, &mut all_summaries);
            }

            // 2. Claude Code sessions
            let claude_projects = home.join(".claude").join("projects");
            if claude_projects.exists() {
                self.collect_claude_summaries(&claude_projects, &mut all_summaries);
            }
            let claude_sessions = home.join(".claude").join("sessions");
            if claude_sessions.exists() {
                self.collect_claude_summaries(&claude_sessions, &mut all_summaries);
            }

            // 3. Hermes sessions
            let hermes_sessions = home.join(".hermes").join("sessions");
            if hermes_sessions.exists() {
                self.collect_hermes_summaries(&hermes_sessions, &mut all_summaries);
            }
            let hermes_history = home.join(".hermes").join("history");
            if hermes_history.exists() {
                self.collect_hermes_summaries(&hermes_history, &mut all_summaries);
            }
        }

        // Fast batch insert in a single transaction
        let _ = self.conn.execute("BEGIN TRANSACTION", []);
        let mut count = 0;
        for (summary, path_str) in &all_summaries {
            if self.upsert_session(summary, path_str).is_ok() {
                count += 1;
            }
        }
        let _ = self.conn.execute("COMMIT", []);
        Ok(count)
    }

    fn collect_codex_summaries(&self, root: &Path, out: &mut Vec<(SessionSummary, String)>) {
        let files = collect_files_recursive(root, 10, "jsonl");
        for file in files {
            if let Some(summary) = parse_codex_session_file(&file) {
                out.push((summary, file.to_string_lossy().to_string()));
            }
        }
    }

    fn collect_claude_summaries(&self, root: &Path, out: &mut Vec<(SessionSummary, String)>) {
        let files = collect_files_recursive(root, 10, "jsonl");
        for file in files {
            if let Some(summary) = parse_claude_session_file(&file) {
                out.push((summary, file.to_string_lossy().to_string()));
            }
        }
    }

    fn collect_hermes_summaries(&self, root: &Path, out: &mut Vec<(SessionSummary, String)>) {
        let files = collect_files_recursive(root, 5, "json");
        for file in files {
            if let Some(summary) = parse_hermes_session_file(&file) {
                out.push((summary, file.to_string_lossy().to_string()));
            }
        }
    }

    fn upsert_session(&self, s: &SessionSummary, raw_path: &str) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO sessions (id, client, title, message_count, total_tokens, created_at, updated_at, raw_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                client=excluded.client,
                title=excluded.title,
                message_count=excluded.message_count,
                total_tokens=excluded.total_tokens,
                created_at=excluded.created_at,
                updated_at=excluded.updated_at,
                raw_path=excluded.raw_path",
            params![
                s.id,
                s.client,
                s.title,
                s.message_count as i64,
                s.total_tokens as i64,
                s.created_at,
                s.updated_at,
                raw_path,
            ],
        )?;
        Ok(())
    }

    pub fn list_summaries(&self) -> AppResult<Vec<SessionSummary>> {
        let mut stmt = self.conn
            .prepare("SELECT id, client, title, message_count, total_tokens, created_at, updated_at FROM sessions ORDER BY updated_at DESC LIMIT 500")
            .map_err(|e| AppError::Storage(format!("查询历史列表准备失败: {e}")))?;

        let rows = stmt
            .query_map([], |row| {
                let msg_count: i64 = row.get(3)?;
                let tokens: i64 = row.get(4)?;
                Ok(SessionSummary {
                    id: row.get(0)?,
                    client: row.get(1)?,
                    title: row.get(2)?,
                    message_count: msg_count as usize,
                    total_tokens: tokens as usize,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })
            .map_err(|e| AppError::Storage(format!("查询历史列表失败: {e}")))?;

        let mut results = Vec::new();
        for r in rows {
            if let Ok(item) = r {
                results.push(item);
            }
        }
        Ok(results)
    }

    pub fn query(&self, query: &HistoryQuery) -> AppResult<HistoryPage> {
        let mut conditions = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ref client) = query.client {
            if !client.trim().is_empty() && client != "all" {
                conditions.push("client = ?");
                params_vec.push(Box::new(client.clone()));
            }
        }

        if let Some(ref search) = query.search {
            let s = search.trim();
            if !s.is_empty() {
                conditions.push("(title LIKE ? OR id LIKE ?)");
                let pattern = format!("%{s}%");
                params_vec.push(Box::new(pattern.clone()));
                params_vec.push(Box::new(pattern));
            }
        }

        if let Some(ref from) = query.date_from {
            if !from.trim().is_empty() {
                conditions.push("created_at >= ?");
                params_vec.push(Box::new(from.clone()));
            }
        }

        if let Some(ref to) = query.date_to {
            if !to.trim().is_empty() {
                conditions.push("created_at <= ?");
                params_vec.push(Box::new(to.clone()));
            }
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let count_sql = format!("SELECT COUNT(*) FROM sessions {where_clause}");
        let total: i64 = {
            let mut stmt = self
                .conn
                .prepare(&count_sql)
                .map_err(|e| AppError::Storage(e.to_string()))?;
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params_vec.iter().map(|b| b.as_ref()).collect();
            stmt.query_row(rusqlite::params_from_iter(params_refs), |r| r.get(0))
                .unwrap_or(0)
        };

        let page_size = if query.page_size == 0 {
            20
        } else {
            query.page_size
        };
        let page = if query.page == 0 { 1 } else { query.page };
        let offset = (page - 1) * page_size;

        let query_sql = format!(
            "SELECT id, client, title, message_count, total_tokens, created_at, updated_at FROM sessions {where_clause} ORDER BY updated_at DESC LIMIT {page_size} OFFSET {offset}"
        );

        let mut stmt = self
            .conn
            .prepare(&query_sql)
            .map_err(|e| AppError::Storage(e.to_string()))?;
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params_refs), |row| {
                let msg_count: i64 = row.get(3)?;
                let tokens: i64 = row.get(4)?;
                Ok(SessionSummary {
                    id: row.get(0)?,
                    client: row.get(1)?,
                    title: row.get(2)?,
                    message_count: msg_count as usize,
                    total_tokens: tokens as usize,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })
            .map_err(|e| AppError::Storage(e.to_string()))?;

        let mut items = Vec::new();
        for r in rows {
            if let Ok(item) = r {
                items.push(item);
            }
        }

        Ok(HistoryPage {
            items,
            total: total as usize,
            page,
            page_size,
        })
    }

    pub fn get_usage_stats(&self) -> AppResult<UsageStats> {
        let (total_sessions, total_messages, total_tokens): (i64, i64, i64) = self.conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(message_count), 0), COALESCE(SUM(total_tokens), 0) FROM sessions",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap_or((0, 0, 0));

        let mut sessions_by_client = HashMap::new();
        if let Ok(mut stmt) = self
            .conn
            .prepare("SELECT client, COUNT(*) FROM sessions GROUP BY client")
        {
            if let Ok(rows) =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            {
                for r in rows.flatten() {
                    sessions_by_client.insert(r.0, r.1 as usize);
                }
            }
        }

        let mut sessions_by_date = Vec::new();
        if let Ok(mut stmt) = self.conn.prepare(
            "SELECT substr(created_at, 1, 10) as day, COUNT(*) FROM sessions WHERE day IS NOT NULL AND day != '' GROUP BY day ORDER BY day DESC LIMIT 30"
        ) {
            if let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))) {
                for r in rows.flatten() {
                    sessions_by_date.push((r.0, r.1 as usize));
                }
            }
        }

        Ok(UsageStats {
            total_sessions: total_sessions as usize,
            total_messages: total_messages as usize,
            total_tokens: total_tokens as usize,
            sessions_by_client,
            sessions_by_date,
        })
    }

    pub fn export_all(&self, format: &str) -> AppResult<String> {
        let list = self.list_summaries()?;
        if format.eq_ignore_ascii_case("csv") {
            let mut csv =
                String::from("id,client,title,message_count,total_tokens,created_at,updated_at\n");
            for s in list {
                let escaped_title = s.title.replace('"', "\"\"");
                csv.push_str(&format!(
                    "\"{}\",\"{}\",\"{}\",{},{},\"{}\",\"{}\"\n",
                    s.id,
                    s.client,
                    escaped_title,
                    s.message_count,
                    s.total_tokens,
                    s.created_at,
                    s.updated_at
                ));
            }
            Ok(csv)
        } else {
            serde_json::to_string_pretty(&list)
                .map_err(|e| AppError::Storage(format!("导出 JSON 失败: {e}")))
        }
    }

    pub fn create_encrypted_backup_file(_password: &str) -> AppResult<String> {
        let store = Self::open()?;
        let summaries = store.list_summaries()?;
        let json_bytes = serde_json::to_vec_pretty(&summaries)
            .map_err(|e| AppError::Encryption(e.to_string()))?;
        let encrypted = crate::encrypted_backup::encrypt_data(&json_bytes)?;

        let home =
            crate::user_home_dir().ok_or_else(|| AppError::Config("无法获取用户目录".into()))?;
        let backup_dir = home.join(".ai-deck").join("backups");
        let _ = std::fs::create_dir_all(&backup_dir);
        let filename = format!(
            "ai-deck-backup-{}.history-backup",
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        );
        let target = backup_dir.join(filename);
        std::fs::write(&target, encrypted)?;
        Ok(target.to_string_lossy().to_string())
    }

    pub fn restore_encrypted_backup_file(path: &str, _password: &str) -> AppResult<()> {
        let encrypted = std::fs::read(path)?;
        let decrypted = crate::encrypted_backup::decrypt_data(&encrypted)?;
        let summaries: Vec<SessionSummary> = serde_json::from_slice(&decrypted)
            .map_err(|e| AppError::Encryption(format!("备份内容格式无效: {e}")))?;

        let store = Self::open()?;
        for s in summaries {
            let _ = store.upsert_session(&s, path);
        }
        Ok(())
    }
}

fn get_db_path() -> AppResult<PathBuf> {
    let home = crate::user_home_dir().ok_or_else(|| AppError::Config("无法获取用户目录".into()))?;
    Ok(home.join(".ai-deck").join("history.db"))
}

pub fn clean_session_title(raw: &str) -> String {
    let mut clean = raw.trim().to_string();
    if clean.starts_with("<instructions>")
        || clean.starts_with("# AGENTS.md")
        || clean.starts_with("<permissions")
        || clean.starts_with("<collaboration_mode")
    {
        for line in clean.lines() {
            let l = line.trim();
            if l.is_empty()
                || l.starts_with('#')
                || l.starts_with('<')
                || l.starts_with("```")
                || l.starts_with('-')
                || l.starts_with('*')
            {
                continue;
            }
            if l.len() > 3 {
                clean = l.to_string();
                break;
            }
        }
    }
    let single_line = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.chars().count() > 80 {
        single_line.chars().take(80).collect::<String>() + "..."
    } else if single_line.is_empty() {
        "对话会话".to_string()
    } else {
        single_line
    }
}

pub fn collect_files_recursive(dir: &Path, max_depth: usize, extension: &str) -> Vec<PathBuf> {
    let mut results = Vec::new();
    collect_files_inner(dir, 0, max_depth, extension, &mut results);
    results
}

fn collect_files_inner(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    extension: &str,
    out: &mut Vec<PathBuf>,
) {
    if depth > max_depth {
        return;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files_inner(&path, depth + 1, max_depth, extension, out);
            } else if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext.to_string_lossy().eq_ignore_ascii_case(extension) {
                        out.push(path);
                    }
                }
            }
        }
    }
}

pub fn parse_codex_session_file(path: &Path) -> Option<SessionSummary> {
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut session_id = String::new();
    let mut title = String::new();
    let mut created_at = String::new();
    let mut updated_at = String::new();
    let mut line_count = 0;

    for line_res in reader.lines() {
        let line = match line_res {
            Ok(l) => l,
            Err(_) => break,
        };
        line_count += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if line_count <= 40 || session_id.is_empty() || title.is_empty() {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                let event_type = val.get("type").and_then(|v| v.as_str()).unwrap_or_default();
                let ts = val
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if !ts.is_empty() {
                    if created_at.is_empty() {
                        created_at = ts.to_string();
                    }
                    updated_at = ts.to_string();
                }

                if event_type == "session_meta" {
                    if let Some(payload) = val.get("payload") {
                        if let Some(id) = payload.get("id").and_then(|v| v.as_str()) {
                            session_id = id.to_string();
                        }
                        if let Some(t) = payload.get("timestamp").and_then(|v| v.as_str()) {
                            if created_at.is_empty() {
                                created_at = t.to_string();
                            }
                        }
                    }
                } else if event_type == "response_item" && title.is_empty() {
                    if let Some(payload) = val.get("payload") {
                        let role = payload
                            .get("role")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        if role == "user" {
                            if let Some(content_arr) =
                                payload.get("content").and_then(|v| v.as_array())
                            {
                                for c in content_arr {
                                    if let Some(text) = c.get("text").and_then(|v| v.as_str()) {
                                        let clean = text.trim();
                                        if !clean.starts_with('<') && clean.len() > 1 {
                                            title = clean_session_title(clean);
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if line_count == 0 {
        return None;
    }

    if session_id.is_empty() {
        if let Some(stem) = path.file_stem() {
            session_id = stem.to_string_lossy().to_string();
        } else {
            session_id = uuid::Uuid::new_v4().to_string();
        }
    }

    if title.is_empty() {
        let short_id: String = session_id.chars().take(8).collect();
        title = format!("Codex 会话 ({short_id})");
    }

    if created_at.is_empty() {
        if let Ok(meta) = path.metadata() {
            if let Ok(created) = meta.created() {
                let dt: chrono::DateTime<chrono::Utc> = created.into();
                created_at = dt.to_rfc3339();
            }
        }
    }
    if updated_at.is_empty() {
        updated_at = created_at.clone();
    }

    let message_count = (line_count / 3).max(1);
    let total_tokens = (line_count * 150).max(200);

    Some(SessionSummary {
        id: session_id,
        client: "codex-cli".into(),
        title,
        message_count,
        total_tokens,
        created_at,
        updated_at,
    })
}

pub fn parse_claude_session_file(path: &Path) -> Option<SessionSummary> {
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut session_id = String::new();
    let mut title = String::new();
    let mut created_at = String::new();
    let mut updated_at = String::new();
    let mut line_count = 0;

    for line_res in reader.lines() {
        let line = match line_res {
            Ok(l) => l,
            Err(_) => break,
        };
        line_count += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if line_count <= 40 || session_id.is_empty() || title.is_empty() {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(sid) = val.get("sessionId").and_then(|v| v.as_str()) {
                    if session_id.is_empty() {
                        session_id = sid.to_string();
                    }
                }
                let ts = val
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if !ts.is_empty() {
                    if created_at.is_empty() {
                        created_at = ts.to_string();
                    }
                    updated_at = ts.to_string();
                }

                let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or_default();
                if msg_type == "user" && title.is_empty() {
                    if let Some(msg) = val.get("message") {
                        if let Some(text) = msg.get("content").and_then(|v| v.as_str()) {
                            let clean = text.trim();
                            if !clean.is_empty() && !clean.starts_with('<') {
                                title = clean_session_title(clean);
                            }
                        }
                    }
                }
            }
        }
    }

    if line_count == 0 {
        return None;
    }

    if session_id.is_empty() {
        if let Some(stem) = path.file_stem() {
            session_id = stem.to_string_lossy().to_string();
        } else {
            session_id = uuid::Uuid::new_v4().to_string();
        }
    }

    if title.is_empty() {
        let short_id: String = session_id.chars().take(8).collect();
        title = format!("Claude 会话 ({short_id})");
    }

    if created_at.is_empty() {
        if let Ok(meta) = path.metadata() {
            if let Ok(created) = meta.created() {
                let dt: chrono::DateTime<chrono::Utc> = created.into();
                created_at = dt.to_rfc3339();
            }
        }
    }
    if updated_at.is_empty() {
        updated_at = created_at.clone();
    }

    let message_count = (line_count / 2).max(1);
    let total_tokens = (line_count * 200).max(300);

    Some(SessionSummary {
        id: session_id,
        client: "claude-code".into(),
        title,
        message_count,
        total_tokens,
        created_at,
        updated_at,
    })
}

pub fn parse_hermes_session_file(path: &Path) -> Option<SessionSummary> {
    let content = std::fs::read_to_string(path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&content).ok()?;
    let session_id = val
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| path.file_stem().and_then(|s| s.to_str()))
        .unwrap_or("hermes-session")
        .to_string();

    let title = val
        .get("title")
        .and_then(|v| v.as_str())
        .or_else(|| val.get("name").and_then(|v| v.as_str()))
        .unwrap_or("Hermes 会话")
        .to_string();

    let message_count = val
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(2);
    let total_tokens = val
        .get("tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or((message_count * 200) as u64) as usize;

    let created_at = val
        .get("createdAt")
        .and_then(|v| v.as_str())
        .or_else(|| val.get("created_at").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    let updated_at = val
        .get("updatedAt")
        .and_then(|v| v.as_str())
        .or_else(|| val.get("updated_at").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
        .unwrap_or_else(|| created_at.clone());

    Some(SessionSummary {
        id: session_id,
        client: "hermes".into(),
        title: clean_session_title(&title),
        message_count,
        total_tokens,
        created_at,
        updated_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_store() {
        let store = HistoryStore::open_in_memory().unwrap();
        let summary = SessionSummary {
            id: "test-session-1".into(),
            client: "codex-cli".into(),
            title: "测试会话标题".into(),
            message_count: 5,
            total_tokens: 1500,
            created_at: "2026-08-20T10:00:00Z".into(),
            updated_at: "2026-08-20T10:30:00Z".into(),
        };
        store
            .upsert_session(&summary, "/path/to/file.jsonl")
            .unwrap();

        let list = store.list_summaries().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "test-session-1");

        let stats = store.get_usage_stats().unwrap();
        assert_eq!(stats.total_sessions, 1);
        assert_eq!(stats.total_messages, 5);
        assert_eq!(stats.total_tokens, 1500);
    }
}
