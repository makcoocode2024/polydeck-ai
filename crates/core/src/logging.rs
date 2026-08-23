//! Structured JSON Lines logging with auto-redaction and rotation.

use crate::error::AppResult;
use crate::profile::data_directory;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;
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
pub struct LogStore {
    log_dir: PathBuf,
}

/// Non-blocking writers must outlive the subscriber, or buffered lines are lost
/// when the guard drops. Held for the process lifetime.
static GUARDS: OnceLock<Vec<WorkerGuard>> = OnceLock::new();

/// True for events emitted by the gateway crate, which get their own file.
fn is_gateway_target(target: &str) -> bool {
    target.starts_with("polydeck_gateway")
}

impl LogRouter {
    /// Install the file subscriber. Idempotent; safe to call more than once.
    ///
    /// This previously called `tracing_subscriber::fmt()` with no writer, so every
    /// `info!`/`warn!` in the workspace went to stdout — which a windowed Tauri
    /// build discards. The `logs/` directory was created and left empty, and
    /// nothing had been written since 2026-08-19. Three separate Codex failures had
    /// to be diagnosed by reverse-engineering client transcripts because the
    /// gateway kept no record of what it sent or received.
    ///
    /// Routes gateway events to `gateway-YYYY-MM-DD.log` and everything else to
    /// `app-YYYY-MM-DD.log`, matching the file layout that existed before the
    /// regression, as JSON Lines with the same field shape.
    pub fn init() -> AppResult<()> {
        let log_dir = data_directory()?.join("logs");
        fs::create_dir_all(&log_dir)?;

        // `rolling::daily` alone names files `gateway.2026-08-23` with no
        // extension, which `get_logs` would skip and which breaks from the
        // `gateway-2026-08-19.log` files already on disk. The builder restores the
        // `.log` suffix so both old and new files are picked up.
        let gateway_file = tracing_appender::rolling::Builder::new()
            .filename_prefix("gateway")
            .filename_suffix("log")
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .build(&log_dir)
            .map_err(|e| crate::error::AppError::Storage(format!("log appender: {e}")))?;
        let app_file = tracing_appender::rolling::Builder::new()
            .filename_prefix("app")
            .filename_suffix("log")
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .build(&log_dir)
            .map_err(|e| crate::error::AppError::Storage(format!("log appender: {e}")))?;
        let (gateway_writer, gateway_guard) = tracing_appender::non_blocking(gateway_file);
        let (app_writer, app_guard) = tracing_appender::non_blocking(app_file);

        // INFO by default; RUST_LOG still wins so a debug session can raise it
        // without a rebuild.
        let env_filter =
            || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

        let gateway_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_current_span(false)
            .with_span_list(false)
            .with_writer(RedactingMakeWriter(
                gateway_writer.with_max_level(tracing::Level::TRACE),
            ))
            .with_filter(filter_fn(|meta| is_gateway_target(meta.target())))
            .with_filter(env_filter());

        let app_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_current_span(false)
            .with_span_list(false)
            .with_writer(RedactingMakeWriter(
                app_writer.with_max_level(tracing::Level::TRACE),
            ))
            .with_filter(filter_fn(|meta| !is_gateway_target(meta.target())))
            .with_filter(env_filter());

        // `try_init` rather than `init`: a second call, or a test that already
        // installed a subscriber, must not abort the process.
        let installed = tracing_subscriber::registry()
            .with(gateway_layer)
            .with(app_layer)
            .try_init()
            .is_ok();

        if installed {
            // Only keep the guards if this call owns the subscriber; dropping them
            // on a losing call would close writers the winner is still using.
            let _ = GUARDS.set(vec![gateway_guard, app_guard]);
            tracing::info!(
                log_dir = %log_dir.display(),
                "File logging initialised (gateway-*.log / app-*.log, daily rotation)"
            );
        }
        Ok(())
    }
}

impl LogStore {
    pub fn new() -> AppResult<Self> {
        let log_dir = data_directory()?.join("logs");
        fs::create_dir_all(&log_dir)?;
        Ok(Self { log_dir })
    }

    /// Read the newest log lines, most recent first.
    ///
    /// Returned `Ok(vec![])` unconditionally before, so the Settings page log view
    /// was permanently empty even while the files held data.
    pub fn get_logs(&self, level: Option<&str>, limit: usize) -> AppResult<Vec<LogEntry>> {
        let wanted = level.map(str::to_ascii_uppercase);
        let mut files: Vec<PathBuf> = fs::read_dir(&self.log_dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "log"))
            .collect();
        // Newest file first, so `limit` fills from the most recent day.
        files.sort();
        files.reverse();

        let mut entries = Vec::new();
        for path in files {
            if entries.len() >= limit {
                break;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            // Within a file, newest line last — walk backwards.
            for line in text.lines().rev() {
                if entries.len() >= limit {
                    break;
                }
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let Some(entry) = parse_log_line(line) else {
                    continue;
                };
                if let Some(want) = &wanted {
                    if &entry.level != want {
                        continue;
                    }
                }
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    /// Delete every rotated log file. Returns quietly if the directory is absent.
    pub fn clear_logs(&self) -> AppResult<()> {
        let Ok(dir) = fs::read_dir(&self.log_dir) else {
            return Ok(());
        };
        for path in dir
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "log"))
        {
            // A file the appender still holds open cannot be removed on Windows;
            // truncating keeps the intent without failing the whole call.
            if fs::remove_file(&path).is_err() {
                let _ = fs::write(&path, b"");
            }
        }
        Ok(())
    }

    pub fn export_logs(&self) -> AppResult<String> {
        Ok(self.log_dir.to_string_lossy().into())
    }
}

/// Writer that masks credentials before anything reaches the disk.
///
/// Redacting only on read (`parse_log_line`) would leave keys in plaintext in the
/// files themselves and mask them merely in the UI — the file is the artefact that
/// gets copied into a bug report, so it is the one that has to be clean. No log
/// statement includes a raw upstream body today, but the gateway does log upstream
/// error text, and one careless `warn!` would otherwise persist a key.
struct RedactingWriter<W: std::io::Write>(W);

impl<W: std::io::Write> std::io::Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match std::str::from_utf8(buf) {
            Ok(text) => {
                let cleaned = redact_sensitive(text);
                self.0.write_all(cleaned.as_bytes())?;
                // Report the original length: the caller counts what it handed us,
                // and a shorter count reads as a partial write.
                Ok(buf.len())
            }
            // Non-UTF-8 cannot contain a key in the forms we match, so pass it on.
            Err(_) => self.0.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

#[derive(Clone)]
struct RedactingMakeWriter<M>(M);

impl<'a, M> tracing_subscriber::fmt::MakeWriter<'a> for RedactingMakeWriter<M>
where
    M: tracing_subscriber::fmt::MakeWriter<'a>,
{
    type Writer = RedactingWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter(self.0.make_writer())
    }

    fn make_writer_for(&'a self, meta: &tracing::Metadata<'_>) -> Self::Writer {
        RedactingWriter(self.0.make_writer_for(meta))
    }
}

/// Parse one JSON Lines record written by the fmt layer into a `LogEntry`.
///
/// The layer nests the message under `fields.message`, so it needs lifting to the
/// flat shape the frontend expects.
fn parse_log_line(line: &str) -> Option<LogEntry> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let message = value
        .pointer("/fields/message")
        .and_then(|m| m.as_str())
        .map(str::to_string)
        // Fall back to the whole fields object when an event carries structured
        // fields but no `message`, so the line is not silently dropped.
        .or_else(|| value.get("fields").map(|f| f.to_string()))
        .unwrap_or_default();
    Some(LogEntry {
        timestamp: value
            .get("timestamp")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_string(),
        level: value
            .get("level")
            .and_then(|l| l.as_str())
            .unwrap_or("INFO")
            .to_string(),
        message: redact_sensitive(&message),
        target: value
            .get("target")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

/// Prefixes that introduce a credential in log text.
///
/// Longest first: `sk-ant-` has to be tried before `sk-`, or the shorter match
/// consumes the token and the more specific label is never applied.
const SECRET_PREFIXES: [&str; 5] = ["sk-ant-", "sk-", "xai-", "AIza", "Bearer "];

/// Mask API keys in text destined for a log file.
///
/// The previous implementation hung. It searched from the start on every pass and
/// rewrote `sk-…` to `sk-****`, which still contains `sk-`, so `find` returned the
/// same position forever — an infinite loop on any text containing `sk-`, which is
/// every Agnes, OpenAI or DeepSeek error message. It also cut a fixed 20 bytes and
/// so left the tail of longer keys in place (`Bearer abcdefghijklmnop` came out as
/// `Bear****nop`), and could panic by slicing a multi-byte character mid-way.
///
/// This walks forward instead: each match consumes the whole credential token and
/// the scan resumes *after* what it wrote, so it always terminates and never
/// rescans its own output.
pub fn redact_sensitive(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;

    loop {
        // Earliest match across all prefixes, so overlapping ones cannot be skipped.
        let found = SECRET_PREFIXES
            .iter()
            .filter_map(|pat| rest.find(pat).map(|pos| (pos, *pat)))
            .min_by_key(|(pos, pat)| (*pos, std::cmp::Reverse(pat.len())));

        let Some((pos, pat)) = found else {
            out.push_str(rest);
            return out;
        };

        out.push_str(&rest[..pos]);
        let after_prefix = &rest[pos + pat.len()..];
        // The credential runs until the first character that cannot be part of a
        // token. Everything up to there is dropped, tail included.
        let token_len = after_prefix
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(after_prefix.len());

        // Keep the prefix so the log still says what kind of secret it was, but
        // trim a trailing space so `Bearer ` reads as `Bearer****`.
        out.push_str(pat.trim_end());
        out.push_str("****");
        rest = &after_prefix[token_len..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The old implementation looped forever on any text containing `sk-`, because
    /// it rewrote `sk-…` to `sk-****` and then found `sk-` again at the same index.
    /// Every Agnes/OpenAI/DeepSeek error message contains `sk-`, so putting it on
    /// the logging path would have frozen the app rather than recorded anything.
    #[test]
    fn redaction_terminates_on_a_key_prefix() {
        assert_eq!(redact_sensitive("key sk-abcdefghijklmnop"), "key sk-****");
        // The literal prefix with nothing after it also has to terminate.
        assert_eq!(redact_sensitive("sk-"), "sk-****");
        assert_eq!(redact_sensitive("sk-sk-sk-"), "sk-****");
    }

    #[test]
    fn redaction_removes_the_whole_token() {
        // The old version cut a fixed 20 bytes and left the tail: `Bearer
        // abcdefghijklmnop` came out as `Bear****nop`.
        assert_eq!(
            redact_sensitive("Authorization: Bearer abcdefghijklmnopqrstuvwxyz0123"),
            "Authorization: Bearer****"
        );
        let long = format!("token sk-{}", "a".repeat(200));
        assert_eq!(redact_sensitive(&long), "token sk-****");
    }

    #[test]
    fn redaction_keeps_surrounding_text_and_handles_several_secrets() {
        assert_eq!(
            redact_sensitive("first sk-aaaaaaaaaaaa then sk-bbbbbbbbbbbb done"),
            "first sk-**** then sk-**** done"
        );
        assert_eq!(redact_sensitive("nothing to hide"), "nothing to hide");
        // Trailing punctuation is not part of the token and must survive.
        assert_eq!(
            redact_sensitive("(key: sk-abc123, ok)"),
            "(key: sk-****, ok)"
        );
    }

    #[test]
    fn redaction_prefers_the_more_specific_prefix() {
        // `sk-ant-` must win over `sk-`, or the label loses which vendor it was.
        assert_eq!(
            redact_sensitive("sk-ant-api03-abcdefghijklmnop"),
            "sk-ant-****"
        );
    }

    #[test]
    fn redaction_does_not_split_multibyte_characters() {
        // Slicing by byte offset used to risk panicking mid-character.
        let text = "密钥是 sk-abcdefghij，请勿泄露";
        let out = redact_sensitive(text);
        assert!(out.contains("sk-****"), "got {out}");
        assert!(out.contains("请勿泄露"), "tail lost: {out}");
        assert!(!out.contains("abcdefghij"), "secret leaked: {out}");
    }

    #[test]
    fn gateway_events_route_to_their_own_file() {
        assert!(is_gateway_target("polydeck_gateway::router"));
        assert!(is_gateway_target("polydeck_gateway::middleware"));
        assert!(!is_gateway_target("polydeck_core::profile_switch"));
        assert!(!is_gateway_target("polydeck"));
    }

    #[test]
    fn parses_the_json_lines_shape_the_fmt_layer_writes() {
        let line = r#"{"timestamp":"2026-08-23T20:00:00.123456+08:00","level":"INFO","fields":{"message":"Gateway listening on 127.0.0.1:18888"},"target":"polydeck_gateway::server"}"#;
        let entry = parse_log_line(line).expect("should parse");
        assert_eq!(entry.level, "INFO");
        assert_eq!(entry.target, "polydeck_gateway::server");
        assert_eq!(entry.message, "Gateway listening on 127.0.0.1:18888");
        assert!(entry.timestamp.starts_with("2026-08-23"));
    }

    #[test]
    fn parsed_messages_are_redacted() {
        let line = r#"{"timestamp":"t","level":"ERROR","fields":{"message":"auth failed for sk-abcdefghijklmnop"},"target":"polydeck_gateway::client"}"#;
        let entry = parse_log_line(line).expect("should parse");
        assert_eq!(entry.message, "auth failed for sk-****");
    }

    #[test]
    fn the_writer_redacts_before_anything_reaches_disk() {
        use std::io::Write;
        // Redacting only on read would leave keys in the file itself, and the file
        // is what gets attached to a bug report.
        let mut sink: Vec<u8> = Vec::new();
        {
            let mut w = RedactingWriter(&mut sink);
            let line = br#"{"fields":{"message":"auth Bearer sk-abcdefghijklmnopqrst failed"}}"#;
            let n = w.write(line).expect("write should succeed");
            // Must report the caller's length, or it looks like a partial write.
            assert_eq!(n, line.len());
            w.flush().unwrap();
        }
        let written = String::from_utf8(sink).unwrap();
        assert!(
            !written.contains("abcdefghijklmnopqrst"),
            "secret hit the disk: {written}"
        );
        // `Bearer ` starts before `sk-`, and earliest-match wins, so the whole
        // `Bearer sk-…` is consumed as one credential and labelled `Bearer****`.
        assert!(written.contains("Bearer****"), "no marker left: {written}");
        assert!(
            written.contains("auth ") && written.contains(" failed"),
            "surrounding text lost: {written}"
        );
    }

    #[test]
    fn non_json_lines_are_skipped_not_fatal() {
        assert!(parse_log_line("not json at all").is_none());
        assert!(parse_log_line("").is_none());
    }

    #[test]
    fn an_event_without_a_message_field_still_yields_something() {
        let line = r#"{"timestamp":"t","level":"WARN","fields":{"rpm":20},"target":"polydeck_gateway::rate_limiter"}"#;
        let entry = parse_log_line(line).expect("should parse");
        assert!(
            entry.message.contains("rpm"),
            "fields lost: {}",
            entry.message
        );
    }
}
