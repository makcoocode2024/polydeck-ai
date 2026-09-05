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
    /// The provider node this conversation ran against, when known.
    ///
    /// Recorded so switching providers or rotating a key stops looking like data
    /// loss: the session stays in one list and carries where it came from, rather
    /// than the app inferring provenance from whatever is configured right now.
    #[serde(default)]
    pub provider_id: Option<String>,
    /// The profile bound to the client when this conversation was indexed.
    #[serde(default)]
    pub profile_id: Option<String>,
    /// How many on-disk session files were merged into this row.
    ///
    /// Above 1 means the same conversation existed under several id schemes and was
    /// consolidated; surfaced so a merge is visible rather than silent.
    #[serde(default)]
    pub merged_from: usize,
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

/// The canonical client id for a session row.
///
/// The database on disk carries four spellings for two clients (`Codex`/`codex`,
/// `Claude Code`/`claude-code`): earlier versions wrote display names, later ones
/// wrote detector ids. Filtering by client then splits one client into two buckets,
/// and the counts never add up. Everything is normalized onto the detector ids that
/// `client_detector::detect_all` produces, since those are what the rest of the app
/// keys on.
/// Read a column that may hold either TEXT or INTEGER as a string.
///
/// SQLite columns are typed per value, not per column, and this database has 623
/// rows whose `created_at`/`updated_at` are stored as INTEGER Unix seconds next to
/// 387 rows holding ISO TEXT. Asking for a `String` on an INTEGER value returns
/// `InvalidColumnType`, and because the read path used `rows.flatten()` those rows
/// were dropped silently — the same 623 conversations that appeared to have gone
/// missing. Reading through `ValueRef` accepts either representation.
fn text_or_number(row: &rusqlite::Row<'_>, idx: usize) -> Result<String, rusqlite::Error> {
    use rusqlite::types::ValueRef;
    Ok(match row.get_ref(idx)? {
        ValueRef::Null => String::new(),
        ValueRef::Integer(n) => n.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).to_string(),
        ValueRef::Blob(bytes) => String::from_utf8_lossy(bytes).to_string(),
    })
}

/// Read an integer column that may have been stored as TEXT.
fn number_or_text(row: &rusqlite::Row<'_>, idx: usize) -> Result<i64, rusqlite::Error> {
    use rusqlite::types::ValueRef;
    Ok(match row.get_ref(idx)? {
        ValueRef::Integer(n) => n,
        ValueRef::Real(f) => f as i64,
        ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).trim().parse().unwrap_or(0),
        _ => 0,
    })
}

/// Map one `sessions` row onto a summary, normalizing as it goes.
///
/// Column order must match the SELECT lists in `list_summaries` and `query`. Every
/// column is read tolerantly: this database spans several schema generations, and a
/// strict read drops rows rather than reporting a problem.
fn row_to_summary(row: &rusqlite::Row<'_>) -> Result<SessionSummary, rusqlite::Error> {
    Ok(SessionSummary {
        id: text_or_number(row, 0)?,
        client: normalize_client(&text_or_number(row, 1)?),
        title: text_or_number(row, 2)?,
        message_count: number_or_text(row, 3)?.max(0) as usize,
        total_tokens: number_or_text(row, 4)?.max(0) as usize,
        created_at: normalize_timestamp(&text_or_number(row, 5)?),
        updated_at: normalize_timestamp(&text_or_number(row, 6)?),
        provider_id: row.get(7).ok().flatten(),
        profile_id: row.get(8).ok().flatten(),
        merged_from: number_or_text(row, 9)?.max(1) as usize,
    })
}

/// Whether a title is one of the generated fallbacks rather than real content.
///
/// The parsers synthesize "Codex 会话 (abc12345)" when no user message is found.
/// Merging must not let such a placeholder overwrite a title recovered from a
/// richer copy of the same conversation.
fn is_placeholder_title(title: &str) -> bool {
    let t = title.trim();
    t.is_empty()
        || t == "对话会话"
        || t == "Hermes 会话"
        || ((t.starts_with("Codex 会话 (") || t.starts_with("Claude 会话 (")) && t.ends_with(')'))
}

/// The earlier of two RFC 3339 timestamps, ignoring blanks.
fn earliest(a: &str, b: &str) -> String {
    match (a.trim().is_empty(), b.trim().is_empty()) {
        (true, _) => b.to_string(),
        (_, true) => a.to_string(),
        _ => {
            if a <= b {
                a.to_string()
            } else {
                b.to_string()
            }
        }
    }
}

/// The later of two RFC 3339 timestamps, ignoring blanks.
fn latest(a: &str, b: &str) -> String {
    match (a.trim().is_empty(), b.trim().is_empty()) {
        (true, _) => b.to_string(),
        (_, true) => a.to_string(),
        _ => {
            if a >= b {
                a.to_string()
            } else {
                b.to_string()
            }
        }
    }
}

pub fn normalize_client(raw: &str) -> String {
    let lower = raw.trim().to_ascii_lowercase().replace(' ', "-");
    match lower.as_str() {
        "codex" | "codex-cli" => "codex-cli".to_string(),
        "claude" | "claude-code" => "claude-code".to_string(),
        "claude-desktop" => "claude-desktop".to_string(),
        "hermes" => "hermes".to_string(),
        // A row with no client is kept rather than dropped, under a name that makes
        // the gap visible in the filter list.
        "" => "unknown".to_string(),
        other => other.to_string(),
    }
}

/// The conversation identity inside a session id, independent of id scheme.
///
/// Session ids have been written three ways over time: a bare uuid, a
/// `rollout-<date>-<uuid>` filename stem, and a `codex_`/`claude_` prefixed uuid.
/// All three can name the *same* conversation, which is how 325 conversations came
/// to be stored twice in one database — once under a bare uuid and once prefixed.
/// The embedded uuid is the stable part, so it is the merge key; ids without one
/// fall back to themselves and simply never merge.
pub fn session_identity(id: &str) -> String {
    let bytes = id.as_bytes();
    let is_hex = |b: u8| b.is_ascii_hexdigit();
    // Scan for a 8-4-4-4-12 uuid anywhere in the id.
    if bytes.len() >= 36 {
        for start in 0..=bytes.len() - 36 {
            let w = &bytes[start..start + 36];
            let shaped = w[8] == b'-'
                && w[13] == b'-'
                && w[18] == b'-'
                && w[23] == b'-'
                && w[..8].iter().all(|&b| is_hex(b))
                && w[9..13].iter().all(|&b| is_hex(b))
                && w[14..18].iter().all(|&b| is_hex(b))
                && w[19..23].iter().all(|&b| is_hex(b))
                && w[24..].iter().all(|&b| is_hex(b));
            if shaped {
                return id[start..start + 36].to_ascii_lowercase();
            }
        }
    }
    id.trim().to_ascii_lowercase()
}

/// Coerce a stored timestamp into RFC 3339.
///
/// Older rows stored Unix seconds in what is now a TEXT column, newer rows store
/// ISO strings. `ORDER BY updated_at DESC` compares them as text, so every numeric
/// timestamp sorts *below* every ISO one: with `LIMIT 500` over 1010 rows, 623
/// numeric rows fell off the end of the list and looked deleted. Normalizing on
/// read and on write makes one ordering apply to all of them.
pub fn normalize_timestamp(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // Purely numeric means Unix seconds (or milliseconds, for a few writers).
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        if let Ok(n) = trimmed.parse::<i64>() {
            let secs = if n > 100_000_000_000 { n / 1000 } else { n };
            if let Some(dt) = chrono::DateTime::from_timestamp(secs, 0) {
                return dt.to_rfc3339();
            }
        }
    }
    trimmed.to_string()
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

    /// Open a specific database file and migrate its schema, without indexing.
    ///
    /// `open` syncs on the way in, which rewrites rows before a caller can inspect
    /// them. Verifying a migration, or consolidating a database whose session files
    /// are not on this machine, needs the two steps separable.
    pub fn open_at(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)
            .map_err(|e| AppError::Storage(format!("打开历史库 {} 失败: {e}", path.display())))?;
        let store = Self { conn };
        store.init_tables()?;
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
                    -- Nullable to match databases written by earlier versions, which
                    -- left the title unset on 623 rows. The read path substitutes an
                    -- empty string, so a fresh database and an upgraded one behave
                    -- identically.
                    title TEXT,
                    message_count INTEGER NOT NULL DEFAULT 0,
                    total_tokens INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT,
                    updated_at TEXT,
                    raw_path TEXT
                )",
                [],
            )
            .map_err(|e| AppError::Storage(format!("创建数据表失败: {e}")))?;

        // Databases written by earlier versions predate these columns. `ALTER TABLE`
        // errors when the column is already there, which is the common case, so the
        // result is deliberately discarded rather than treated as a failure.
        for column in [
            "provider_id TEXT",
            "profile_id TEXT",
            "identity TEXT",
            "merged_from INTEGER NOT NULL DEFAULT 1",
        ] {
            let _ = self
                .conn
                .execute(&format!("ALTER TABLE sessions ADD COLUMN {column}"), []);
        }

        // Grouping and filtering both hit these.
        let _ = self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_identity ON sessions(identity)",
            [],
        );
        let _ = self.conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_provider ON sessions(provider_id)",
            [],
        );

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

        // Indexing writes rows keyed on whatever id scheme each file uses, so folding
        // duplicates together belongs at the end of every sync rather than only in a
        // manual pass. Idempotent, and a failure here must not fail the sync — the
        // rows are already stored and remain queryable, just unmerged.
        if let Err(e) = self.consolidate() {
            tracing::warn!("会话整合失败，历史仍可查询但可能存在重复：{e}");
        }
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

    /// Insert or merge one session.
    ///
    /// Keyed on `identity` rather than the raw id: the same conversation appears
    /// under several id schemes across versions, and keying on `id` is what let one
    /// conversation occupy two rows. When a row for this identity already exists the
    /// two are merged — the richer title wins, counts take the larger value, and the
    /// date range widens — so re-indexing converges instead of duplicating.
    fn upsert_session(&self, s: &SessionSummary, raw_path: &str) -> Result<(), rusqlite::Error> {
        let identity = session_identity(&s.id);
        let client = normalize_client(&s.client);
        let created = normalize_timestamp(&s.created_at);
        let updated = normalize_timestamp(&s.updated_at);

        let existing: Option<(String, String, i64, i64, String, String, i64)> = self
            .conn
            .query_row(
                "SELECT id, title, message_count, total_tokens, created_at, updated_at, \
                        COALESCE(merged_from, 1)
                 FROM sessions WHERE identity = ?1 LIMIT 1",
                params![identity],
                |r| {
                    Ok((
                        text_or_number(r, 0)?,
                        text_or_number(r, 1)?,
                        number_or_text(r, 2)?,
                        number_or_text(r, 3)?,
                        text_or_number(r, 4)?,
                        text_or_number(r, 5)?,
                        number_or_text(r, 6)?,
                    ))
                },
            )
            .ok();

        if let Some((keep_id, old_title, old_msgs, old_tokens, old_created, old_updated, merged)) =
            existing
        {
            // A generated placeholder ("Codex 会话 (abc12345)") carries no
            // information, so a real first message replaces it; otherwise keep what
            // is already there rather than churning the list on every re-index.
            let title = if is_placeholder_title(&old_title) && !is_placeholder_title(&s.title) {
                s.title.clone()
            } else {
                old_title
            };
            let created = earliest(&old_created, &created);
            let updated = latest(&old_updated, &updated);
            let same_row = keep_id == s.id;

            self.conn.execute(
                "UPDATE sessions SET
                    client=?2, title=?3,
                    message_count=MAX(?4, message_count),
                    total_tokens=MAX(?5, total_tokens),
                    created_at=?6, updated_at=?7, raw_path=?8,
                    provider_id=COALESCE(?9, provider_id),
                    profile_id=COALESCE(?10, profile_id),
                    merged_from=?11
                 WHERE id=?1",
                params![
                    keep_id,
                    client,
                    title,
                    s.message_count as i64,
                    s.total_tokens as i64,
                    created,
                    updated,
                    raw_path,
                    s.provider_id,
                    s.profile_id,
                    if same_row { merged } else { merged + 1 },
                ],
            )?;

            // The duplicate row, if this identity previously had one under a
            // different id scheme.
            if !same_row {
                let _ = self
                    .conn
                    .execute("DELETE FROM sessions WHERE id=?1", params![s.id]);
            }
            let _ = old_msgs;
            let _ = old_tokens;
            return Ok(());
        }

        self.conn.execute(
            "INSERT INTO sessions (id, client, title, message_count, total_tokens, created_at, updated_at, raw_path, provider_id, profile_id, identity, merged_from)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1)
             ON CONFLICT(id) DO UPDATE SET
                client=excluded.client,
                title=excluded.title,
                message_count=excluded.message_count,
                total_tokens=excluded.total_tokens,
                created_at=excluded.created_at,
                updated_at=excluded.updated_at,
                raw_path=excluded.raw_path,
                provider_id=COALESCE(excluded.provider_id, sessions.provider_id),
                profile_id=COALESCE(excluded.profile_id, sessions.profile_id),
                identity=excluded.identity",
            params![
                s.id,
                client,
                s.title,
                s.message_count as i64,
                s.total_tokens as i64,
                created,
                updated,
                raw_path,
                s.provider_id,
                s.profile_id,
                identity,
            ],
        )?;
        Ok(())
    }

    /// Every indexed session, newest first.
    ///
    /// The `LIMIT 500` this used to carry silently truncated the list: combined with
    /// the mixed timestamp formats, which sort numeric values below every ISO
    /// string, it hid 623 of 1010 rows on the developer's own database and read as
    /// "my history disappeared after switching providers". Rows are normalized on
    /// read as well as write, so a database migrated but not yet re-indexed still
    /// sorts correctly.
    pub fn list_summaries(&self) -> AppResult<Vec<SessionSummary>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, client, title, message_count, total_tokens, created_at, updated_at, \
                        provider_id, profile_id, COALESCE(merged_from, 1) \
                 FROM sessions",
            )
            .map_err(|e| AppError::Storage(format!("查询历史列表准备失败: {e}")))?;

        let rows = stmt
            .query_map([], row_to_summary)
            .map_err(|e| AppError::Storage(format!("查询历史列表失败: {e}")))?;

        let mut results: Vec<SessionSummary> = rows.flatten().collect();
        // Sorted here rather than in SQL: a database that has been migrated but not
        // re-indexed still holds raw numeric timestamps, and only the normalized
        // values compare correctly.
        results.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(results)
    }

    pub fn query(&self, query: &HistoryQuery) -> AppResult<HistoryPage> {
        let mut conditions = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(ref client) = query.client {
            if !client.trim().is_empty() && client != "all" {
                // Compared against the normalized form: rows written by earlier
                // versions hold `Codex`/`Claude Code`, so an equality test against a
                // detector id matched only part of one client's sessions.
                conditions.push("LOWER(REPLACE(client, ' ', '-')) IN (?, REPLACE(?, '-cli', ''))");
                let normalized = normalize_client(client);
                params_vec.push(Box::new(normalized.clone()));
                params_vec.push(Box::new(normalized));
            }
        }

        // `provider` has been on HistoryQuery since the type was introduced but was
        // never read, so filtering by it silently returned everything. Now that rows
        // carry provenance it does what it says.
        if let Some(ref provider) = query.provider {
            if !provider.trim().is_empty() && provider != "all" {
                conditions.push("provider_id = ?");
                params_vec.push(Box::new(provider.clone()));
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
            "SELECT id, client, title, message_count, total_tokens, created_at, updated_at, \
                    provider_id, profile_id, COALESCE(merged_from, 1) \
             FROM sessions {where_clause}"
        );

        let mut stmt = self
            .conn
            .prepare(&query_sql)
            .map_err(|e| AppError::Storage(e.to_string()))?;
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params_refs), row_to_summary)
            .map_err(|e| AppError::Storage(e.to_string()))?;

        // Ordering and paging happen after normalization for the same reason as in
        // `list_summaries`: raw numeric timestamps do not sort against ISO ones.
        let mut all: Vec<SessionSummary> = rows.flatten().collect();
        all.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        let items: Vec<SessionSummary> = all.into_iter().skip(offset).take(page_size).collect();

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

/// What a consolidation pass changed.
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ConsolidateReport {
    /// Rows whose `client` spelling was rewritten to a detector id.
    pub clients_normalized: usize,
    /// Rows whose timestamp was converted from Unix seconds to RFC 3339.
    pub timestamps_normalized: usize,
    /// Rows that gained an `identity` value.
    pub identities_filled: usize,
    /// Duplicate rows removed after being merged into a surviving row.
    pub duplicates_merged: usize,
    /// Sessions remaining after the pass.
    pub sessions_after: usize,
}

impl HistoryStore {
    /// Fold duplicate rows together and normalize the columns they are compared on.
    ///
    /// Idempotent, and safe to run on an already-clean database: every step is a
    /// no-op when its precondition is already met. Reports what it touched instead
    /// of returning a bare success, since "consolidated 0 things" and "consolidated
    /// 325 things" are very different outcomes for the user.
    ///
    /// Merging keeps the row with the most messages and widens its date range,
    /// because the id schemes carry no ordering: neither a bare uuid nor a prefixed
    /// one is inherently the newer copy.
    pub fn consolidate(&self) -> AppResult<ConsolidateReport> {
        let mut report = ConsolidateReport::default();

        // 1. Client spellings. Done in Rust rather than SQL so one normalization
        //    function governs both this and the read path.
        let rows: Vec<(String, String)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, client FROM sessions")
                .map_err(|e| AppError::Storage(e.to_string()))?;
            let mapped = stmt
                .query_map([], |r| Ok((text_or_number(r, 0)?, text_or_number(r, 1)?)))
                .map_err(|e| AppError::Storage(e.to_string()))?;
            mapped.flatten().collect()
        };
        for (id, client) in rows {
            let normalized = normalize_client(&client);
            if normalized != client {
                let _ = self.conn.execute(
                    "UPDATE sessions SET client=?2 WHERE id=?1",
                    params![id, normalized],
                );
                report.clients_normalized += 1;
            }
        }

        // 2. Timestamps.
        let stamps: Vec<(String, String, String)> = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT id, COALESCE(created_at,''), COALESCE(updated_at,'') FROM sessions",
                )
                .map_err(|e| AppError::Storage(e.to_string()))?;
            let mapped = stmt
                .query_map([], |r| {
                    Ok((
                        text_or_number(r, 0)?,
                        text_or_number(r, 1)?,
                        text_or_number(r, 2)?,
                    ))
                })
                .map_err(|e| AppError::Storage(e.to_string()))?;
            mapped.flatten().collect()
        };
        for (id, created, updated) in stamps {
            let nc = normalize_timestamp(&created);
            let nu = normalize_timestamp(&updated);
            if nc != created || nu != updated {
                let _ = self.conn.execute(
                    "UPDATE sessions SET created_at=?2, updated_at=?3 WHERE id=?1",
                    params![id, nc, nu],
                );
                report.timestamps_normalized += 1;
            }
        }

        // 3. Identity, which is what the merge groups on.
        let ids: Vec<(String, Option<String>)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, identity FROM sessions")
                .map_err(|e| AppError::Storage(e.to_string()))?;
            let mapped = stmt
                .query_map([], |r| {
                    Ok((
                        text_or_number(r, 0)?,
                        r.get::<_, Option<String>>(1).ok().flatten(),
                    ))
                })
                .map_err(|e| AppError::Storage(e.to_string()))?;
            mapped.flatten().collect()
        };
        for (id, identity) in ids {
            let want = session_identity(&id);
            if identity.as_deref() != Some(want.as_str()) {
                let _ = self.conn.execute(
                    "UPDATE sessions SET identity=?2 WHERE id=?1",
                    params![id, want],
                );
                report.identities_filled += 1;
            }
        }

        // 4. Merge groups sharing an identity.
        let groups: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT identity FROM sessions WHERE identity IS NOT NULL \
                     GROUP BY identity HAVING COUNT(*) > 1",
                )
                .map_err(|e| AppError::Storage(e.to_string()))?;
            let mapped = stmt
                .query_map([], |r| r.get(0))
                .map_err(|e| AppError::Storage(e.to_string()))?;
            mapped.flatten().collect()
        };

        for identity in groups {
            let members: Vec<(String, String, i64, i64, String, String)> = {
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT id, COALESCE(title,''), message_count, total_tokens, \
                                COALESCE(created_at,''), COALESCE(updated_at,'') \
                         FROM sessions WHERE identity=?1 ORDER BY message_count DESC",
                    )
                    .map_err(|e| AppError::Storage(e.to_string()))?;
                let mapped = stmt
                    .query_map(params![identity], |r| {
                        Ok((
                            text_or_number(r, 0)?,
                            text_or_number(r, 1)?,
                            number_or_text(r, 2)?,
                            number_or_text(r, 3)?,
                            text_or_number(r, 4)?,
                            text_or_number(r, 5)?,
                        ))
                    })
                    .map_err(|e| AppError::Storage(e.to_string()))?;
                mapped.flatten().collect()
            };
            if members.len() < 2 {
                continue;
            }

            // Richest row survives; it already sorts first.
            let keep = &members[0];
            let mut title = keep.1.clone();
            let mut msgs = keep.2;
            let mut tokens = keep.3;
            let mut created = keep.4.clone();
            let mut updated = keep.5.clone();

            for other in &members[1..] {
                if is_placeholder_title(&title) && !is_placeholder_title(&other.1) {
                    title = other.1.clone();
                }
                msgs = msgs.max(other.2);
                tokens = tokens.max(other.3);
                created = earliest(&created, &other.4);
                updated = latest(&updated, &other.5);
            }

            let merged_count = members.len();
            let _ = self.conn.execute(
                "UPDATE sessions SET title=?2, message_count=?3, total_tokens=?4, \
                        created_at=?5, updated_at=?6, merged_from=?7 WHERE id=?1",
                params![
                    keep.0,
                    title,
                    msgs,
                    tokens,
                    created,
                    updated,
                    merged_count as i64
                ],
            );

            for other in &members[1..] {
                // Re-point any recorded source files at the surviving row before the
                // duplicate goes away, so the merge does not lose provenance.
                let _ = self.conn.execute(
                    "UPDATE session_sources SET session_id=?2 WHERE session_id=?1",
                    params![other.0, keep.0],
                );
                if self
                    .conn
                    .execute("DELETE FROM sessions WHERE id=?1", params![other.0])
                    .is_ok()
                {
                    report.duplicates_merged += 1;
                }
            }
        }

        report.sessions_after = self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize;
        Ok(report)
    }

    /// Record which provider and profile a client's sessions are currently running
    /// against.
    ///
    /// Called when a profile is bound or a key rotated, so sessions indexed from that
    /// point carry provenance. Only rows with no provider yet are stamped: a
    /// conversation that already ran against another provider is history, and
    /// rewriting it would misattribute what actually happened.
    pub fn stamp_provenance(
        &self,
        client: &str,
        profile_id: &str,
        provider_id: &str,
    ) -> AppResult<usize> {
        let normalized = normalize_client(client);
        let changed = self
            .conn
            .execute(
                "UPDATE sessions SET provider_id=?3, profile_id=?2 \
                 WHERE LOWER(REPLACE(client, ' ', '-')) IN (?1, REPLACE(?1, '-cli', '')) \
                   AND provider_id IS NULL",
                params![normalized, profile_id, provider_id],
            )
            .map_err(|e| AppError::Storage(format!("写入会话归属失败: {e}")))?;
        Ok(changed)
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
        // Provenance is attached by the caller, which knows the bound profile; the
        // parsers only see files on disk.
        provider_id: None,
        profile_id: None,
        merged_from: 1,
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
        // Provenance is attached by the caller, which knows the bound profile; the
        // parsers only see files on disk.
        provider_id: None,
        profile_id: None,
        merged_from: 1,
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
        // Provenance is attached by the caller, which knows the bound profile; the
        // parsers only see files on disk.
        provider_id: None,
        profile_id: None,
        merged_from: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(id: &str, client: &str, title: &str, msgs: usize, updated: &str) -> SessionSummary {
        SessionSummary {
            id: id.into(),
            client: client.into(),
            title: title.into(),
            message_count: msgs,
            total_tokens: msgs * 100,
            created_at: updated.into(),
            updated_at: updated.into(),
            provider_id: None,
            profile_id: None,
            merged_from: 1,
        }
    }

    /// The four spellings the real database carries must collapse onto two clients.
    /// Filtering by `codex-cli` previously matched only the rows written with that
    /// exact string, leaving the `Codex`-spelled ones invisible.
    #[test]
    fn client_spellings_collapse_onto_detector_ids() {
        for raw in ["Codex", "codex", "codex-cli", " CODEX "] {
            assert_eq!(normalize_client(raw), "codex-cli", "输入：{raw:?}");
        }
        for raw in ["Claude Code", "claude-code", "claude"] {
            assert_eq!(normalize_client(raw), "claude-code", "输入：{raw:?}");
        }
        assert_eq!(normalize_client(""), "unknown");
    }

    /// The three id schemes seen in the wild name the same conversation when they
    /// share a uuid. This is the merge key, so it has to be scheme-independent.
    #[test]
    fn session_identity_ignores_the_id_scheme() {
        let uuid = "019578a4-58eb-4c1c-8f46-c20c63bb598b";
        let variants = [
            uuid.to_string(),
            format!("claude_{uuid}"),
            format!("codex_{uuid}"),
            format!("rollout-2026-05-29T08-29-39-{uuid}"),
        ];
        for v in &variants {
            assert_eq!(session_identity(v), uuid, "输入：{v}");
        }
        // No uuid means no merging, rather than collapsing unrelated sessions.
        assert_eq!(session_identity("hermes-session"), "hermes-session");
    }

    /// Unix seconds sort below every ISO string as text, which is what pushed 623
    /// rows past `LIMIT 500` and made them look deleted.
    #[test]
    fn timestamps_normalize_to_one_comparable_format() {
        let iso = normalize_timestamp("1780014673");
        assert!(iso.starts_with("2026-"), "期望 RFC3339，得到 {iso}");
        // Already-ISO values pass through untouched.
        assert_eq!(
            normalize_timestamp("2026-08-19T17:52:38+00:00"),
            "2026-08-19T17:52:38+00:00"
        );
        assert_eq!(normalize_timestamp("  "), "");
        // Ordering now works across both original formats.
        assert!(
            normalize_timestamp("1780014673") < normalize_timestamp("2026-08-19T00:00:00+00:00")
        );
    }

    /// Two rows for one conversation must become one, keeping the richer title and
    /// the widest date range. This is the case that made history look lost.
    #[test]
    fn consolidate_merges_duplicate_id_schemes() {
        let store = HistoryStore::open_in_memory().unwrap();
        let uuid = "019578a4-58eb-4c1c-8f46-c20c63bb598b";

        // Written straight to the table, bypassing `upsert_session`: that path now
        // merges on write, so it cannot produce the duplicate state this pass exists
        // to repair. This reproduces what earlier versions actually left on disk —
        // one conversation as a bare uuid with a Unix timestamp and no title, and
        // again under a prefixed id with the real title.
        let raw_insert = |id: &str, client: &str, title: &str, msgs: i64, ts: &str| {
            store
                .conn
                .execute(
                    "INSERT INTO sessions (id, client, title, message_count, total_tokens, \
                                           created_at, updated_at, raw_path) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, '/x.jsonl')",
                    params![id, client, title, msgs, msgs * 100, ts],
                )
                .unwrap();
        };
        raw_insert(uuid, "codex", "Codex 会话 (019578a4)", 0, "1780014673");
        raw_insert(
            &format!("codex_{uuid}"),
            "Codex",
            "真实的第一条用户消息",
            17,
            "2026-08-19T17:52:38+00:00",
        );

        // Two rows for one conversation, which is the state users are sitting on.
        assert_eq!(
            store.list_summaries().unwrap().len(),
            2,
            "前置条件：应有两行重复"
        );

        let report = store.consolidate().unwrap();
        let list = store.list_summaries().unwrap();

        assert_eq!(list.len(), 1, "同一会话必须合并为一行，实际 {}", list.len());
        let merged = &list[0];
        assert_eq!(merged.title, "真实的第一条用户消息", "必须保留有内容的标题");
        assert_eq!(merged.message_count, 17, "消息数取较大值");
        assert_eq!(merged.client, "codex-cli", "客户端名必须归一化");
        assert!(
            merged.created_at.starts_with("2026-05-"),
            "创建时间应取更早的那个，实际 {}",
            merged.created_at
        );
        assert_eq!(
            merged.updated_at, "2026-08-19T17:52:38+00:00",
            "更新时间应取更晚的那个"
        );
        assert_eq!(report.sessions_after, 1);
        assert!(report.duplicates_merged >= 1, "应报告合并了重复行");
    }

    /// Running the pass twice must not change anything the second time, since it runs
    /// after every sync.
    #[test]
    fn consolidate_is_idempotent() {
        let store = HistoryStore::open_in_memory().unwrap();
        let uuid = "04a11c4c-8f73-4193-aeb9-880dea6faab6";
        store
            .upsert_session(&summary(uuid, "claude", "", 0, "1780016928"), "/a.jsonl")
            .unwrap();
        store
            .upsert_session(
                &summary(
                    &format!("claude_{uuid}"),
                    "Claude Code",
                    "真实标题",
                    9,
                    "2026-08-19T16:04:43+00:00",
                ),
                "/b.jsonl",
            )
            .unwrap();

        store.consolidate().unwrap();
        let first = store.list_summaries().unwrap();
        let second_report = store.consolidate().unwrap();
        let second = store.list_summaries().unwrap();

        assert_eq!(first.len(), second.len(), "重复运行不应改变行数");
        assert_eq!(second_report.duplicates_merged, 0, "第二次不应再合并任何行");
        assert_eq!(first[0].title, second[0].title);
    }

    /// Unrelated conversations must never be folded together, whatever their ids.
    #[test]
    fn consolidate_leaves_distinct_sessions_alone() {
        let store = HistoryStore::open_in_memory().unwrap();
        store
            .upsert_session(
                &summary(
                    "019578a4-58eb-4c1c-8f46-c20c63bb598b",
                    "codex",
                    "会话甲",
                    3,
                    "2026-08-01T00:00:00+00:00",
                ),
                "/a.jsonl",
            )
            .unwrap();
        store
            .upsert_session(
                &summary(
                    "04a11c4c-8f73-4193-aeb9-880dea6faab6",
                    "codex",
                    "会话乙",
                    4,
                    "2026-08-02T00:00:00+00:00",
                ),
                "/b.jsonl",
            )
            .unwrap();

        store.consolidate().unwrap();
        assert_eq!(store.list_summaries().unwrap().len(), 2, "不同会话不得合并");
    }

    /// SQLite types values per row, not per column, and this database holds 623 rows
    /// whose timestamps are INTEGER Unix seconds beside 387 holding ISO TEXT. Reading
    /// such a column as `String` returns `InvalidColumnType`, and the read path used
    /// `rows.flatten()`, so those rows were dropped without a word — the actual
    /// mechanism behind "my history disappeared". Rows must survive either storage
    /// type.
    #[test]
    fn rows_stored_as_integers_are_not_silently_dropped() {
        let store = HistoryStore::open_in_memory().unwrap();

        // NULL title and INTEGER timestamps, exactly as the legacy rows are stored.
        store
            .conn
            .execute(
                "INSERT INTO sessions (id, client, title, message_count, total_tokens, \
                                       created_at, updated_at, raw_path) \
                 VALUES ('019578a4-58eb-4c1c-8f46-c20c63bb598b', 'codex', NULL, 0, 0, \
                         1780014673, 1780014673, '/legacy.jsonl')",
                [],
            )
            .unwrap();
        // A modern row alongside it.
        store
            .upsert_session(
                &summary(
                    "04a11c4c-8f73-4193-aeb9-880dea6faab6",
                    "codex-cli",
                    "新格式会话",
                    5,
                    "2026-08-19T17:52:38+00:00",
                ),
                "/modern.jsonl",
            )
            .unwrap();

        let list = store.list_summaries().unwrap();
        assert_eq!(
            list.len(),
            2,
            "INTEGER 时间戳的行不得被丢弃，实际读到 {}",
            list.len()
        );

        let legacy = list
            .iter()
            .find(|s| s.id.starts_with("019578a4"))
            .expect("旧格式行必须能被读出");
        assert!(
            legacy.updated_at.starts_with("2026-"),
            "旧行时间戳应转为 RFC3339，实际 {}",
            legacy.updated_at
        );

        // And ordering has to hold across the two original storage types.
        assert!(
            list[0].updated_at >= list[1].updated_at,
            "混合存储类型下排序仍须单调"
        );

        // consolidate() reads the same columns and must see both rows too.
        let report = store.consolidate().unwrap();
        assert_eq!(report.sessions_after, 2, "整合不得漏掉 INTEGER 行");
        assert_eq!(report.timestamps_normalized, 1, "应修正那一行的时间格式");
    }

    /// Provenance answers "which provider did this run against", so it must not
    /// rewrite sessions that already carry one — that would misattribute history
    /// every time the user switched providers.
    #[test]
    fn stamping_provenance_never_overwrites_recorded_history() {
        let store = HistoryStore::open_in_memory().unwrap();
        store
            .upsert_session(
                &summary(
                    "019578a4-58eb-4c1c-8f46-c20c63bb598b",
                    "codex",
                    "旧会话",
                    2,
                    "2026-08-01T00:00:00+00:00",
                ),
                "/a.jsonl",
            )
            .unwrap();

        assert_eq!(
            store
                .stamp_provenance("codex-cli", "prof_a", "prov_a")
                .unwrap(),
            1,
            "首次应写入归属"
        );
        assert_eq!(
            store
                .stamp_provenance("codex-cli", "prof_b", "prov_b")
                .unwrap(),
            0,
            "已有归属的会话不得被改写"
        );

        let list = store.list_summaries().unwrap();
        assert_eq!(list[0].provider_id.as_deref(), Some("prov_a"));
        assert_eq!(list[0].profile_id.as_deref(), Some("prof_a"));
    }

    /// Filtering by client has to reach rows written under the old display-name
    /// spelling, which is where "my Codex history vanished" came from.
    #[test]
    fn query_by_client_matches_legacy_spellings() {
        let store = HistoryStore::open_in_memory().unwrap();
        store
            .upsert_session(
                &summary(
                    "019578a4-58eb-4c1c-8f46-c20c63bb598b",
                    "Codex",
                    "旧拼写",
                    2,
                    "2026-08-01T00:00:00+00:00",
                ),
                "/a.jsonl",
            )
            .unwrap();
        store
            .upsert_session(
                &summary(
                    "04a11c4c-8f73-4193-aeb9-880dea6faab6",
                    "codex-cli",
                    "新拼写",
                    2,
                    "2026-08-02T00:00:00+00:00",
                ),
                "/b.jsonl",
            )
            .unwrap();

        let page = store
            .query(&HistoryQuery {
                client: Some("codex-cli".into()),
                provider: None,
                search: None,
                date_from: None,
                date_to: None,
                page: 1,
                page_size: 50,
            })
            .unwrap();
        assert_eq!(page.total, 2, "两种拼写都应被 codex-cli 命中");
    }

    /// The list must not silently truncate; the old `LIMIT 500` is what hid rows.
    #[test]
    fn list_returns_more_than_the_old_five_hundred_limit() {
        let store = HistoryStore::open_in_memory().unwrap();
        for i in 0..640 {
            // Distinct uuids so nothing merges.
            let id = format!("{:08x}-0000-4000-8000-000000000000", i);
            store
                .upsert_session(
                    &summary(&id, "codex", &format!("会话 {i}"), 1, "1780014673"),
                    "/x.jsonl",
                )
                .unwrap();
        }
        assert_eq!(
            store.list_summaries().unwrap().len(),
            640,
            "不得截断到 500 条"
        );
    }

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
            provider_id: None,
            profile_id: None,
            merged_from: 1,
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
