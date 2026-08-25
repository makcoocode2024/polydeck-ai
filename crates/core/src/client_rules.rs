//! Behaviour rules written into each client's global instructions file.
//!
//! Claude Code reads `~/.claude/CLAUDE.md` and Codex reads `~/.codex/AGENTS.md`
//! on every new session, ahead of any conversation content, which is what makes
//! a rule placed there survive long multi-step tasks. Neither client has a
//! setting for the behaviours here, so the instructions file is the only lever.
//!
//! These files belong to the user, and unlike the JSON and TOML configs in
//! `profile_switch` there is no parser to merge at the key level. So each rule
//! lives inside its own sentinel-delimited block: everything outside it is never
//! touched, and turning one rule off removes only that rule's block.
//!
//! The sentinels carry the rule's slug, so two rules coexist in one file and
//! toggling either leaves the other alone. One shared pair of markers would have
//! made the second rule overwrite the first.

use crate::error::{AppError, AppResult};
use std::path::PathBuf;

/// Which rule a block carries.
///
/// Each variant owns its own sentinel pair via its slug, so blocks are found and
/// replaced independently of each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    /// Force Simplified Chinese for prose output.
    ChineseOutput,
    /// Forbid reporting tool results that were never observed.
    ToolTruthfulness,
}

impl RuleKind {
    fn slug(self) -> &'static str {
        match self {
            RuleKind::ChineseOutput => "zh-output",
            RuleKind::ToolTruthfulness => "tool-truth",
        }
    }

    fn block_start(self) -> String {
        format!("<!-- polydeck:{}:start -->", self.slug())
    }

    fn block_end(self) -> String {
        format!("<!-- polydeck:{}:end -->", self.slug())
    }
}

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

    /// The rule text for this client and kind, wording per the client's own
    /// convention.
    ///
    /// The tool-truthfulness wording differs by client in one respect only: the
    /// auto-compact clause names Claude Code's own mechanism, and Codex has no
    /// equivalent, so citing it there would be an instruction about nothing.
    fn rule_body(self, kind: RuleKind) -> &'static str {
        match (kind, self) {
            (RuleKind::ChineseOutput, RuleTarget::ClaudeCode) => {
                "# 全局行为约束\n\
                 ## 语言规则\n\
                 1. 所有解释、分析、推理、总结、报告，**一律简体中文输出**。\n\
                 2. 允许保留代码、报错、标识符英文；禁止大段英文自然语言。\n\
                 3. 长时间多步骤任务，全程维持中文，不要自动切回英文。\n\
                 4. 代码注释优先中文。"
            }
            (RuleKind::ChineseOutput, RuleTarget::Codex) => {
                "# 全局行为约束\n\
                 1. 所有分析、推演、报告、解释文字，**必须使用简体中文输出**。\n\
                 2. 仅代码、标识符、报错信息保留英文；禁止大段英文自然文本。\n\
                 3. 长任务、多步骤 Agent 任务全程保持中文，不要自动切英文。\n\
                 4. 代码注释优先使用简体中文。\n\
                 5. 除非我明确要求英文，全部输出中文。"
            }
            (RuleKind::ToolTruthfulness, RuleTarget::ClaudeCode) => {
                "# 工具执行真实性强制检查\n\
                 \n\
                 工具返回结果是唯一可信的执行事实。模型不得模拟、猜测、虚构或补全工具结果。\n\
                 \n\
                 每次完成涉及工具的操作后，必须先确认实际工具调用及其返回结果，再输出结论。\n\
                 \n\
                 1. 只报告实际执行过的操作。\n\
                 2. 只报告工具实际返回的内容，或能够从工具返回结果直接、明确推导出的结论。\n\
                 3. 未执行的操作必须标记为【未执行】；无法从实际工具结果确认的内容必须标记为【无法确认】。\n\
                 4. 如果工具输出显示 (truncated) 或内容被截断，必须重新调用工具获取完整内容；禁止基于截断内容推导结论。\n\
                 5. 经过 auto-compact 上下文压缩后，涉及文件状态、代码状态、测试状态、命令执行状态等关键事实，必须重新调用相关工具复核；不得直接依赖压缩前的会话记忆。\n\
                 6. 禁止为了让任务看起来完成而虚构测试、文件修改、命令执行、工具调用或验证结果。\n\
                 7. 禁止手动模拟、伪造或编造 bash、read、grep、测试、编译等工具的输出。\n\
                 8. 必须严格区分“工具实际返回的事实”和“模型自己的推测”。推测不得表述为已经验证的事实。\n\
                 9. 如果工具执行失败、超时、返回为空或结果不足以得出结论，必须明确说明，不得自行补全结果。\n\
                 10. 如果无法确认，就直接说明【无法确认】，并在必要时重新调用工具验证。"
            }
            (RuleKind::ToolTruthfulness, RuleTarget::Codex) => {
                "# 工具执行真实性强制检查\n\
                 \n\
                 工具返回结果是唯一可信的执行事实。模型不得模拟、猜测、虚构或补全工具结果。\n\
                 \n\
                 每次完成涉及工具的操作后，必须先确认实际工具调用及其返回结果，再输出结论。\n\
                 \n\
                 1. 只报告实际执行过的操作。\n\
                 2. 只报告工具实际返回的内容，或能够从工具返回结果直接、明确推导出的结论。\n\
                 3. 未执行的操作必须标记为【未执行】；无法从实际工具结果确认的内容必须标记为【无法确认】。\n\
                 4. 如果工具输出显示 (truncated) 或内容被截断，必须重新调用工具获取完整内容；禁止基于截断内容推导结论。\n\
                 5. 上下文被压缩或截断后，涉及文件状态、代码状态、测试状态、命令执行状态等关键事实，必须重新调用相关工具复核；不得直接依赖压缩前的会话记忆。\n\
                 6. 禁止为了让任务看起来完成而虚构测试、文件修改、命令执行、工具调用或验证结果。\n\
                 7. 禁止手动模拟、伪造或编造 bash、read、grep、测试、编译等工具的输出。\n\
                 8. 必须严格区分“工具实际返回的事实”和“模型自己的推测”。推测不得表述为已经验证的事实。\n\
                 9. 如果工具执行失败、超时、返回为空或结果不足以得出结论，必须明确说明，不得自行补全结果。\n\
                 10. 如果无法确认，就直接说明【无法确认】，并在必要时重新调用工具验证。"
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

/// The sentinel-wrapped block for `target` and `kind`.
fn render_block(target: RuleTarget, kind: RuleKind) -> String {
    format!(
        "{}\n{}\n{}",
        kind.block_start(),
        target.rule_body(kind),
        kind.block_end()
    )
}

/// Byte range of `kind`'s existing block in `text`, covering the markers and the
/// body between them but no surrounding whitespace.
///
/// `Ok(None)` means no block of that kind. An unterminated start marker is an
/// error rather than a miss: treating it as absent would prepend a second block
/// and leave the first one's body stranded as loose prose in the user's file.
fn find_block(text: &str, kind: RuleKind) -> AppResult<Option<std::ops::Range<usize>>> {
    let (start_marker, end_marker) = (kind.block_start(), kind.block_end());
    let Some(start) = text.find(&start_marker) else {
        return Ok(None);
    };
    let after_start = start + start_marker.len();
    let Some(rel_end) = text[after_start..].find(&end_marker) else {
        return Err(AppError::Config(format!(
            "指令文件里的 PolyDeck 标记块缺少结束标记 `{end_marker}`，已跳过以免破坏文件内容"
        )));
    };
    Ok(Some(start..after_start + rel_end + end_marker.len()))
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
fn with_block(text: &str, target: RuleTarget, kind: RuleKind) -> AppResult<String> {
    let block = render_block(target, kind);
    match find_block(text, kind)? {
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
fn without_block(text: &str, kind: RuleKind) -> AppResult<Option<String>> {
    let Some(range) = find_block(text, kind)? else {
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

/// Turn one rule on or off for every supported client.
///
/// Idempotent: re-running with the same value rewrites nothing and reports
/// `changed: false`. A client whose file cannot be handled fails on its own
/// without stopping the others, so one unreadable file does not block the rest.
/// Only `kind`'s own block is touched; the other rule's block is left as it is.
pub fn apply(kind: RuleKind, enabled: bool) -> AppResult<Vec<RuleOutcome>> {
    each_target(|target, home| apply_to(target, kind, enabled, home))
}

/// Whether each client's file currently carries `kind`'s rule.
pub fn status(kind: RuleKind) -> AppResult<Vec<RuleOutcome>> {
    each_target(|target, home| {
        let path = target.resolve(home);
        let present = read_instructions(&path)?
            .map(|text| find_block(&text, kind).map(|r| r.is_some()))
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
    kind: RuleKind,
    enabled: bool,
    home: &std::path::Path,
) -> AppResult<(PathBuf, bool, bool)> {
    let path = target.resolve(home);
    let existing = read_instructions(&path)?;
    let updated = match (&existing, enabled) {
        (None, false) => None,
        (None, true) => Some(with_block("", target, kind)?),
        (Some(text), true) => Some(with_block(text, target, kind)?),
        (Some(text), false) => without_block(text, kind)?,
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

    /// The kind under test where the assertion does not depend on which one it is.
    const ZH: RuleKind = RuleKind::ChineseOutput;
    const TOOL: RuleKind = RuleKind::ToolTruthfulness;

    #[test]
    fn block_is_prepended_and_user_content_survives() {
        let out = with_block(USER_TEXT, RuleTarget::Codex, ZH).unwrap();
        assert!(out.starts_with(&ZH.block_start()));
        assert!(out.contains(&ZH.block_end()));
        assert!(
            out.ends_with(USER_TEXT),
            "用户原有内容必须一字不动地留在块后面"
        );
    }

    #[test]
    fn applying_twice_is_idempotent() {
        let once = with_block(USER_TEXT, RuleTarget::ClaudeCode, ZH).unwrap();
        let twice = with_block(&once, RuleTarget::ClaudeCode, ZH).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn existing_block_is_replaced_where_it_sits() {
        // A block the user moved below their own rules stays below them.
        let stale = format!(
            "{USER_TEXT}\n{}\nold wording\n{}\n",
            ZH.block_start(),
            ZH.block_end()
        );
        let out = with_block(&stale, RuleTarget::Codex, ZH).unwrap();
        assert!(out.starts_with("# My own rules"), "块不应被搬到文件开头");
        assert!(!out.contains("old wording"), "旧措辞应被替换");
        assert_eq!(
            out.matches(&ZH.block_start()).count(),
            1,
            "不应出现第二个块"
        );
    }

    #[test]
    fn removal_restores_the_original_bytes() {
        let with = with_block(USER_TEXT, RuleTarget::Codex, ZH).unwrap();
        let without = without_block(&with, ZH).unwrap().unwrap();
        assert_eq!(without, USER_TEXT, "关掉开关必须完全还原用户原文");
    }

    #[test]
    fn removal_reports_nothing_to_do_without_a_block() {
        assert_eq!(without_block(USER_TEXT, ZH).unwrap(), None);
    }

    #[test]
    fn empty_file_gets_only_the_block() {
        let out = with_block("", RuleTarget::ClaudeCode, ZH).unwrap();
        assert_eq!(
            out,
            format!("{}\n", render_block(RuleTarget::ClaudeCode, ZH))
        );
        assert_eq!(without_block(&out, ZH).unwrap().unwrap(), "");
    }

    #[test]
    fn unterminated_block_is_refused_rather_than_duplicated() {
        let broken = format!(
            "{}\n半截内容，结束标记被用户删了\n{USER_TEXT}",
            ZH.block_start()
        );
        assert!(with_block(&broken, RuleTarget::Codex, ZH).is_err());
        assert!(without_block(&broken, ZH).is_err());
    }

    /// The reason the sentinels carry a slug. With one shared pair of markers the
    /// second rule would have found the first rule's block and overwritten it.
    #[test]
    fn the_two_rules_coexist_and_toggle_independently() {
        let both = with_block(
            &with_block(USER_TEXT, RuleTarget::ClaudeCode, ZH).unwrap(),
            RuleTarget::ClaudeCode,
            TOOL,
        )
        .unwrap();
        assert!(both.contains(&ZH.block_start()), "中文规则块丢失：{both}");
        assert!(
            both.contains(&TOOL.block_start()),
            "工具真实性规则块丢失：{both}"
        );
        assert!(both.ends_with(USER_TEXT), "用户原文必须保留");

        // Turning one off leaves the other and the user's text untouched.
        let zh_off = without_block(&both, ZH).unwrap().unwrap();
        assert!(!zh_off.contains(&ZH.block_start()));
        assert!(
            zh_off.contains(&TOOL.block_start()),
            "关掉中文规则不应带走工具规则：{zh_off}"
        );
        assert!(zh_off.ends_with(USER_TEXT));

        // Turning the second off restores the file byte for byte.
        let tool_off = without_block(&zh_off, TOOL).unwrap().unwrap();
        assert_eq!(tool_off, USER_TEXT, "两条都关掉必须还原用户原文");
    }

    /// A truncated block of one kind must not make the *other* kind unusable:
    /// they are found by different markers, so one broken block is one broken
    /// rule.
    #[test]
    fn a_broken_block_of_one_kind_does_not_block_the_other() {
        let broken_zh = format!("{}\n结束标记没了\n{USER_TEXT}", ZH.block_start());
        assert!(with_block(&broken_zh, RuleTarget::Codex, ZH).is_err());
        assert!(
            with_block(&broken_zh, RuleTarget::Codex, TOOL).is_ok(),
            "另一条规则的开关不应被这个坏块卡住"
        );
    }

    #[test]
    fn bom_stays_at_the_front_of_the_file() {
        // A real user file: UTF-8 BOM plus CRLF, as written by a Windows editor.
        let bom = "\u{feff}";
        let original = format!("{bom}# My own rules\r\n\r\nRule one.\r\n");

        let out = with_block(&original, RuleTarget::Codex, ZH).unwrap();
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
            out[bom.len()..].starts_with(&ZH.block_start()),
            "块应紧跟在 BOM 之后"
        );
        assert_eq!(
            without_block(&out, ZH).unwrap().unwrap(),
            original,
            "关掉开关必须字节级还原，BOM 位置也要一致"
        );
    }

    #[test]
    fn crlf_files_are_handled() {
        let crlf = "# Mine\r\n\r\nRule one.\r\n";
        let out = with_block(crlf, RuleTarget::Codex, ZH).unwrap();
        assert!(out.ends_with(crlf), "CRLF 原文应保持原样");
        assert_eq!(without_block(&out, ZH).unwrap().unwrap(), crlf);
    }

    #[test]
    fn each_client_gets_its_own_wording() {
        let claude = render_block(RuleTarget::ClaudeCode, ZH);
        let codex = render_block(RuleTarget::Codex, ZH);
        assert!(claude.contains("## 语言规则"));
        assert!(codex.contains("除非我明确要求英文"));
        assert_ne!(claude, codex);
    }

    /// The tool-truthfulness rule must arrive whole: it is a numbered list the
    /// client is meant to follow, so a silently dropped item is a silently
    /// dropped constraint.
    #[test]
    fn the_tool_rule_carries_all_ten_items() {
        for target in RuleTarget::ALL {
            let body = target.rule_body(TOOL);
            for n in 1..=10 {
                assert!(
                    body.contains(&format!("\n{n}. ")),
                    "{} 缺第 {n} 条",
                    target.label()
                );
            }
            assert!(body.contains("【未执行】"));
            assert!(body.contains("【无法确认】"));
            assert!(
                body.contains("唯一可信的执行事实"),
                "{} 缺开头的总则",
                target.label()
            );
        }
    }

    /// Claude Code's auto-compact is named in its own wording; Codex has no such
    /// mechanism, so naming it there would be an instruction about nothing.
    #[test]
    fn the_auto_compact_clause_is_claude_specific() {
        assert!(RuleTarget::ClaudeCode
            .rule_body(TOOL)
            .contains("auto-compact"));
        assert!(!RuleTarget::Codex.rule_body(TOOL).contains("auto-compact"));
        assert!(
            RuleTarget::Codex.rule_body(TOOL).contains("上下文被压缩"),
            "Codex 仍须覆盖压缩后复核这条要求"
        );
    }

    #[test]
    fn apply_writes_both_files_then_cleans_up_after_itself() {
        let _guard = crate::lock_home_env();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_DECK_HOME_OVERRIDE", home.path());

        // Seed one file with user content, leave the other missing.
        let codex_md = home.path().join(".codex").join("AGENTS.md");
        std::fs::create_dir_all(codex_md.parent().unwrap()).unwrap();
        std::fs::write(&codex_md, USER_TEXT).unwrap();

        let on = apply(ZH, true).unwrap();
        assert_eq!(on.len(), 2);
        assert!(on.iter().all(|o| o.error.is_none()));
        assert!(on.iter().all(|o| o.rule_present && o.changed));

        let claude_md = home.path().join(".claude").join("CLAUDE.md");
        assert!(claude_md.is_file(), "缺失的文件应被创建");
        assert!(std::fs::read_to_string(&codex_md)
            .unwrap()
            .ends_with(USER_TEXT));

        // Re-running changes nothing.
        let again = apply(ZH, true).unwrap();
        assert!(again.iter().all(|o| !o.changed), "重复应用不应重写文件");

        // `status` reads the files rather than the app setting.
        let seen = status(ZH).unwrap();
        assert!(seen.iter().all(|o| o.rule_present && !o.changed));

        // The other rule is independent: its status is false while this one is on.
        assert!(
            status(TOOL).unwrap().iter().all(|o| !o.rule_present),
            "另一条规则不应因为这条开启而报告已写入"
        );

        let off = apply(ZH, false).unwrap();
        assert!(off.iter().all(|o| !o.rule_present && o.changed));
        assert_eq!(
            std::fs::read_to_string(&codex_md).unwrap(),
            USER_TEXT,
            "关掉开关后用户原文必须完好"
        );
        assert_eq!(std::fs::read_to_string(&claude_md).unwrap(), "");

        std::env::remove_var("AI_DECK_HOME_OVERRIDE");
    }

    /// Both rules on, then each off in turn, against real files. The unit-level
    /// coexistence test covers the string handling; this covers the file path,
    /// where a shared-marker bug would show up as one rule's block vanishing.
    #[test]
    fn both_rules_survive_in_one_real_file() {
        let _guard = crate::lock_home_env();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_DECK_HOME_OVERRIDE", home.path());
        let claude_md = home.path().join(".claude").join("CLAUDE.md");
        std::fs::create_dir_all(claude_md.parent().unwrap()).unwrap();
        std::fs::write(&claude_md, USER_TEXT).unwrap();

        apply(ZH, true).unwrap();
        apply(TOOL, true).unwrap();
        let both = std::fs::read_to_string(&claude_md).unwrap();
        assert!(both.contains("## 语言规则"), "中文规则丢失：{both}");
        assert!(
            both.contains("# 工具执行真实性强制检查"),
            "工具规则丢失：{both}"
        );
        assert!(both.ends_with(USER_TEXT), "用户原文必须保留");
        assert!(status(ZH).unwrap().iter().all(|o| o.rule_present));
        assert!(status(TOOL).unwrap().iter().all(|o| o.rule_present));

        apply(ZH, false).unwrap();
        let tool_only = std::fs::read_to_string(&claude_md).unwrap();
        assert!(!tool_only.contains("## 语言规则"));
        assert!(
            tool_only.contains("# 工具执行真实性强制检查"),
            "关掉中文规则带走了工具规则：{tool_only}"
        );

        apply(TOOL, false).unwrap();
        assert_eq!(
            std::fs::read_to_string(&claude_md).unwrap(),
            USER_TEXT,
            "两条都关掉必须还原用户原文"
        );

        std::env::remove_var("AI_DECK_HOME_OVERRIDE");
    }

    #[test]
    fn codex_override_file_is_reported_as_shadowing() {
        let _guard = crate::lock_home_env();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_DECK_HOME_OVERRIDE", home.path());
        let codex_dir = home.path().join(".codex");
        std::fs::create_dir_all(&codex_dir).unwrap();
        std::fs::write(codex_dir.join("AGENTS.override.md"), "wins").unwrap();

        let outcomes = apply(ZH, true).unwrap();
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
