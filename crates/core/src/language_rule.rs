//! Forced-Chinese-output rule, written into each client's global instructions file.
//!
//! Claude Code reads `~/.claude/CLAUDE.md` and Codex reads `~/.codex/AGENTS.md`
//! on every new session, ahead of any conversation content, which is what makes
//! a rule placed there survive long multi-step tasks. Neither client has a
//! setting for output language, so the instructions file is the only lever.
//!
//! These files belong to the user, and unlike the JSON and TOML configs in
//! `profile_switch` there is no parser to merge at the key level. So the rule
//! lives inside a sentinel-delimited block: everything outside it is never
//! touched, and turning the feature off removes only that block.

use crate::error::{AppError, AppResult};
use std::path::PathBuf;

const BLOCK_START: &str = "<!-- polydeck:zh-output:start -->";
const BLOCK_END: &str = "<!-- polydeck:zh-output:end -->";

/// A client whose global instructions file can carry the rule.
///
/// Claude Desktop and Hermes are absent: neither reads a markdown instructions
/// file, so writing one would leave the switch looking effective while changing
/// nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleTarget {
    ClaudeCode,
    Codex,
}

impl RuleTarget {
    pub const ALL: [RuleTarget; 2] = [RuleTarget::ClaudeCode, RuleTarget::Codex];

    /// Path to this client's global instructions file, relative to the home dir.
    ///
    /// Codex resolves `AGENTS.override.md` before `AGENTS.md` and returns on the
    /// first hit, so an override file present on disk would shadow anything
    /// written here. It is the user's own escape hatch, so it is left alone and
    /// reported by [`shadowed_by`] instead.
    fn relative_path(self) -> &'static [&'static str] {
        match self {
            RuleTarget::ClaudeCode => &[".claude", "CLAUDE.md"],
            RuleTarget::Codex => &[".codex", "AGENTS.md"],
        }
    }

    /// A file that would take precedence over the one this rule writes.
    fn shadow_path(self) -> Option<&'static [&'static str]> {
        match self {
            RuleTarget::ClaudeCode => None,
            RuleTarget::Codex => Some(&[".codex", "AGENTS.override.md"]),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RuleTarget::ClaudeCode => "Claude Code",
            RuleTarget::Codex => "Codex",
        }
    }

    /// The rule text for this client, wording per its own convention.
    fn rule_body(self) -> &'static str {
        match self {
            RuleTarget::ClaudeCode => {
                "# 全局行为约束\n\
                 ## 语言规则\n\
                 1. 所有解释、分析、推理、总结、报告，**一律简体中文输出**。\n\
                 2. 允许保留代码、报错、标识符英文；禁止大段英文自然语言。\n\
                 3. 长时间多步骤任务，全程维持中文，不要自动切回英文。\n\
                 4. 代码注释优先中文。"
            }
            RuleTarget::Codex => {
                "# 全局行为约束\n\
                 1. 所有分析、推演、报告、解释文字，**必须使用简体中文输出**。\n\
                 2. 仅代码、标识符、报错信息保留英文；禁止大段英文自然文本。\n\
                 3. 长任务、多步骤 Agent 任务全程保持中文，不要自动切英文。\n\
                 4. 代码注释优先使用简体中文。\n\
                 5. 除非我明确要求英文，全部输出中文。"
            }
        }
    }

    fn resolve(self, home: &std::path::Path) -> PathBuf {
        self.relative_path()
            .iter()
            .fold(home.to_path_buf(), |acc, part| acc.join(part))
    }

    fn resolve_shadow(self, home: &std::path::Path) -> Option<PathBuf> {
        self.shadow_path().map(|parts| {
            parts
                .iter()
                .fold(home.to_path_buf(), |acc, part| acc.join(part))
        })
    }
}

/// The state of the rule in one client's instructions file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct RuleOutcome {
    pub target: String,
    pub path: String,
    /// Whether the block is present in the file, as opposed to whether the app
    /// setting is on. The two diverge when the user edits the file by hand.
    pub rule_present: bool,
    /// True when this call changed the file's bytes.
    pub changed: bool,
    /// Set when another file takes precedence and the rule will not be read.
    pub shadowed_by: Option<String>,
    /// Set when this client's file could not be handled. The other clients are
    /// still processed, so one unreadable file does not block the rest.
    pub error: Option<String>,
}

/// The sentinel-wrapped block for `target`.
fn render_block(target: RuleTarget) -> String {
    format!("{BLOCK_START}\n{}\n{BLOCK_END}", target.rule_body())
}

/// Byte range of the existing block in `text`, covering the markers and the body
/// between them but no surrounding whitespace.
///
/// `Ok(None)` means no block. An unterminated start marker is an error rather
/// than a miss: treating it as absent would prepend a second block and leave the
/// first one's body stranded as loose prose in the user's file.
fn find_block(text: &str) -> AppResult<Option<std::ops::Range<usize>>> {
    let Some(start) = text.find(BLOCK_START) else {
        return Ok(None);
    };
    let after_start = start + BLOCK_START.len();
    let Some(rel_end) = text[after_start..].find(BLOCK_END) else {
        return Err(AppError::Config(format!(
            "指令文件里的 PolyDeck 标记块缺少结束标记 `{BLOCK_END}`，已跳过以免破坏文件内容"
        )));
    };
    Ok(Some(start..after_start + rel_end + BLOCK_END.len()))
}

/// Length of the line ending at the start of `text`, or 0 if there is none.
fn leading_newline_len(text: &str) -> usize {
    if text.starts_with("\r\n") {
        2
    } else if text.starts_with('\n') {
        1
    } else {
        0
    }
}

/// A UTF-8 BOM, which several Windows editors put at the start of the file.
///
/// It has to stay the very first bytes: pushed anywhere else it stops marking the
/// encoding and becomes a zero-width space embedded in the instruction text the
/// client reads.
const BOM: &str = "\u{feff}";

/// `text` with the block inserted or updated in place.
///
/// An existing block is replaced where it sits rather than moved to the top:
/// the user may have deliberately placed it after their own rules, and
/// reordering their file is a bigger change than the switch asks for.
fn with_block(text: &str, target: RuleTarget) -> AppResult<String> {
    let block = render_block(target);
    match find_block(text)? {
        // A pure swap of the marked region; whitespace around it is the user's.
        Some(range) => {
            let mut out = String::with_capacity(text.len() + block.len());
            out.push_str(&text[..range.start]);
            out.push_str(&block);
            out.push_str(&text[range.end..]);
            Ok(out)
        }
        None => {
            // Insertion goes after any BOM, never before it.
            let (bom, body) = match text.strip_prefix(BOM) {
                Some(rest) => (BOM, rest),
                None => ("", text),
            };
            if body.trim().is_empty() {
                return Ok(format!("{bom}{block}\n"));
            }
            // Blank line between the block and the user's first line, which
            // `without_block` takes back so a toggle round-trip is byte-exact.
            Ok(format!(
                "{bom}{block}\n\n{}",
                body.trim_start_matches(['\n', '\r'])
            ))
        }
    }
}

/// `text` with the block removed, or `None` when there was no block.
///
/// Takes back exactly what `with_block` added: the block, its line ending, and
/// the separating blank line — that last one only when content follows, since
/// with none there was no separator to begin with.
fn without_block(text: &str) -> AppResult<Option<String>> {
    let Some(range) = find_block(text)? else {
        return Ok(None);
    };
    let mut rest = &text[range.end..];
    let first = leading_newline_len(rest);
    rest = &rest[first..];
    if !rest.is_empty() {
        rest = &rest[leading_newline_len(rest)..];
    }

    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..range.start]);
    out.push_str(rest);
    Ok(Some(out))
}

/// Read an instructions file, or `None` when it does not exist.
///
/// Deliberately not `storage::read_with_fallback`: falling back to a `.bak` here
/// would resurrect a stale copy of a file the user edits by hand, and the write
/// that follows would make that stale copy authoritative.
///
/// Invalid UTF-8 is an error rather than a lossy decode, since the decoded text
/// is what gets written back — replacement characters would corrupt the user's
/// own content.
fn read_instructions(path: &std::path::Path) -> AppResult<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map(Some).map_err(|_| {
            AppError::Config(format!(
                "指令文件不是有效的 UTF-8，已跳过以免破坏内容：{}",
                path.display()
            ))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::Io(e)),
    }
}

/// Turn the forced-Chinese rule on or off for every supported client.
///
/// Idempotent: re-running with the same value rewrites nothing and reports
/// `changed: false`. A client whose file cannot be handled fails on its own
/// without stopping the others, so one unreadable file does not block the rest.
pub fn apply(enabled: bool) -> AppResult<Vec<RuleOutcome>> {
    each_target(|target, home| apply_to(target, enabled, home))
}

/// Whether each client's file currently carries the rule.
pub fn status() -> AppResult<Vec<RuleOutcome>> {
    each_target(|target, home| {
        let path = target.resolve(home);
        let present = read_instructions(&path)?
            .map(|text| find_block(&text).map(|r| r.is_some()))
            .transpose()?
            .unwrap_or(false);
        Ok((path, present, false))
    })
}

/// Run `f` for every target, turning a per-target failure into a reported
/// outcome instead of an early return.
fn each_target<F>(f: F) -> AppResult<Vec<RuleOutcome>>
where
    F: Fn(RuleTarget, &std::path::Path) -> AppResult<(PathBuf, bool, bool)>,
{
    let home =
        crate::user_home_dir().ok_or_else(|| AppError::Config("无法确定用户主目录".into()))?;
    Ok(RuleTarget::ALL
        .iter()
        .map(|target| {
            let shadowed_by = target
                .resolve_shadow(&home)
                .filter(|p| p.is_file())
                .map(|p| p.display().to_string());
            match f(*target, &home) {
                Ok((path, rule_present, changed)) => RuleOutcome {
                    target: target.label().to_string(),
                    path: path.display().to_string(),
                    rule_present,
                    changed,
                    shadowed_by,
                    error: None,
                },
                Err(e) => RuleOutcome {
                    target: target.label().to_string(),
                    path: target.resolve(&home).display().to_string(),
                    rule_present: false,
                    changed: false,
                    shadowed_by,
                    error: Some(e.to_string()),
                },
            }
        })
        .collect())
}

/// Returns `(path, rule_present, changed)`.
fn apply_to(
    target: RuleTarget,
    enabled: bool,
    home: &std::path::Path,
) -> AppResult<(PathBuf, bool, bool)> {
    let path = target.resolve(home);
    let existing = read_instructions(&path)?;
    let updated = match (&existing, enabled) {
        (None, false) => None,
        (None, true) => Some(with_block("", target)?),
        (Some(text), true) => Some(with_block(text, target)?),
        (Some(text), false) => without_block(text)?,
    };

    let changed = updated.is_some() && updated.as_ref() != existing.as_ref();
    if changed {
        if let Some(next) = &updated {
            crate::storage::atomic_replace(&path, next.as_bytes())?;
        }
    }
    Ok((path, enabled, changed))
}

#[cfg(test)]
mod tests {
    use super::*;

    const USER_TEXT: &str = "# My own rules\n\nAlways run the tests.\n";

    #[test]
    fn block_is_prepended_and_user_content_survives() {
        let out = with_block(USER_TEXT, RuleTarget::Codex).unwrap();
        assert!(out.starts_with(BLOCK_START));
        assert!(out.contains(BLOCK_END));
        assert!(
            out.ends_with(USER_TEXT),
            "用户原有内容必须一字不动地留在块后面"
        );
    }

    #[test]
    fn applying_twice_is_idempotent() {
        let once = with_block(USER_TEXT, RuleTarget::ClaudeCode).unwrap();
        let twice = with_block(&once, RuleTarget::ClaudeCode).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn existing_block_is_replaced_where_it_sits() {
        // A block the user moved below their own rules stays below them.
        let stale = format!("{USER_TEXT}\n{BLOCK_START}\nold wording\n{BLOCK_END}\n");
        let out = with_block(&stale, RuleTarget::Codex).unwrap();
        assert!(out.starts_with("# My own rules"), "块不应被搬到文件开头");
        assert!(!out.contains("old wording"), "旧措辞应被替换");
        assert_eq!(out.matches(BLOCK_START).count(), 1, "不应出现第二个块");
    }

    #[test]
    fn removal_restores_the_original_bytes() {
        let with = with_block(USER_TEXT, RuleTarget::Codex).unwrap();
        let without = without_block(&with).unwrap().unwrap();
        assert_eq!(without, USER_TEXT, "关掉开关必须完全还原用户原文");
    }

    #[test]
    fn removal_reports_nothing_to_do_without_a_block() {
        assert_eq!(without_block(USER_TEXT).unwrap(), None);
    }

    #[test]
    fn empty_file_gets_only_the_block() {
        let out = with_block("", RuleTarget::ClaudeCode).unwrap();
        assert_eq!(out, format!("{}\n", render_block(RuleTarget::ClaudeCode)));
        assert_eq!(without_block(&out).unwrap().unwrap(), "");
    }

    #[test]
    fn unterminated_block_is_refused_rather_than_duplicated() {
        let broken = format!("{BLOCK_START}\n半截内容，结束标记被用户删了\n{USER_TEXT}");
        assert!(with_block(&broken, RuleTarget::Codex).is_err());
        assert!(without_block(&broken).is_err());
    }

    #[test]
    fn bom_stays_at_the_front_of_the_file() {
        // A real user file: UTF-8 BOM plus CRLF, as written by a Windows editor.
        let bom = "\u{feff}";
        let original = format!("{bom}# My own rules\r\n\r\nRule one.\r\n");

        let out = with_block(&original, RuleTarget::Codex).unwrap();
        assert!(
            out.starts_with(bom),
            "BOM 必须留在文件最前面，否则会被推到块后面、卡进正文"
        );
        assert_eq!(
            out.matches(bom).count(),
            1,
            "BOM 只能有一个，不能既留在前面又复制一份到正文里"
        );
        assert!(
            out[bom.len()..].starts_with(BLOCK_START),
            "块应紧跟在 BOM 之后"
        );
        assert_eq!(
            without_block(&out).unwrap().unwrap(),
            original,
            "关掉开关必须字节级还原，BOM 位置也要一致"
        );
    }

    #[test]
    fn crlf_files_are_handled() {
        let crlf = "# Mine\r\n\r\nRule one.\r\n";
        let out = with_block(crlf, RuleTarget::Codex).unwrap();
        assert!(out.ends_with(crlf), "CRLF 原文应保持原样");
        assert_eq!(without_block(&out).unwrap().unwrap(), crlf);
    }

    #[test]
    fn each_client_gets_its_own_wording() {
        let claude = render_block(RuleTarget::ClaudeCode);
        let codex = render_block(RuleTarget::Codex);
        assert!(claude.contains("## 语言规则"));
        assert!(codex.contains("除非我明确要求英文"));
        assert_ne!(claude, codex);
    }

    /// `AI_DECK_HOME_OVERRIDE` is process-global, so tests that repoint HOME must
    /// not run concurrently.
    static HOME_ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn apply_writes_both_files_then_cleans_up_after_itself() {
        let _guard = HOME_ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_DECK_HOME_OVERRIDE", home.path());

        // Seed one file with user content, leave the other missing.
        let codex_md = home.path().join(".codex").join("AGENTS.md");
        std::fs::create_dir_all(codex_md.parent().unwrap()).unwrap();
        std::fs::write(&codex_md, USER_TEXT).unwrap();

        let on = apply(true).unwrap();
        assert_eq!(on.len(), 2);
        assert!(on.iter().all(|o| o.error.is_none()));
        assert!(on.iter().all(|o| o.rule_present && o.changed));

        let claude_md = home.path().join(".claude").join("CLAUDE.md");
        assert!(claude_md.is_file(), "缺失的文件应被创建");
        assert!(std::fs::read_to_string(&codex_md)
            .unwrap()
            .ends_with(USER_TEXT));

        // Re-running changes nothing.
        let again = apply(true).unwrap();
        assert!(again.iter().all(|o| !o.changed), "重复应用不应重写文件");

        // `status` reads the files rather than the app setting.
        let seen = status().unwrap();
        assert!(seen.iter().all(|o| o.rule_present && !o.changed));

        let off = apply(false).unwrap();
        assert!(off.iter().all(|o| !o.rule_present && o.changed));
        assert_eq!(
            std::fs::read_to_string(&codex_md).unwrap(),
            USER_TEXT,
            "关掉开关后用户原文必须完好"
        );
        assert_eq!(std::fs::read_to_string(&claude_md).unwrap(), "");

        std::env::remove_var("AI_DECK_HOME_OVERRIDE");
    }

    #[test]
    fn codex_override_file_is_reported_as_shadowing() {
        let _guard = HOME_ENV_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_DECK_HOME_OVERRIDE", home.path());
        let codex_dir = home.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(codex_dir.join("AGENTS.override.md"), "wins").unwrap();

        let outcomes = apply(true).unwrap();
        let codex = outcomes
            .iter()
            .find(|o| o.target == RuleTarget::Codex.label())
            .unwrap();
        assert!(
            codex.shadowed_by.is_some(),
            "AGENTS.override.md 会抢在 AGENTS.md 前面，必须报告"
        );
        assert_eq!(
            std::fs::read_to_string(codex_dir.join("AGENTS.override.md")).unwrap(),
            "wins",
            "不应改写用户的 override 文件"
        );

        std::env::remove_var("AI_DECK_HOME_OVERRIDE");
    }
}
