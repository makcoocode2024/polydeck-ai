//! Which profile each client is bound to.
//!
//! Every client writer targets one fixed global path — `~/.codex/config.toml`,
//! `~/.claude/settings.json`, Desktop's `Claude-3p/configLibrary`. A client can
//! therefore only follow one profile at a time; the filesystem enforces it. So
//! the relation is a *function* from client to profile, not a many-to-many, and
//! this module holds one entry per bound client.
//!
//! It replaces the single `active_profile_id` that came before, which could not
//! express "Codex on A while Claude Code is on B".

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One client pinned to one profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ClientBinding {
    /// Normalized by [`normalize_client_id`], so lookups never depend on how the
    /// id was cased or padded when it was stored.
    pub client_id: String,
    pub profile_id: String,
    /// RFC3339. Shown in the UI and useful when a binding looks unexpected.
    pub bound_at: String,
}

/// Canonical form of a client id.
///
/// Ids reach us from three places that do not agree on case: a profile's
/// `clients` list as edited in the UI, `client_detector::detect_all`, and
/// hand-edited `state.json`. Comparing raw strings made `"Codex-CLI"` and
/// `"codex-cli"` two different clients, which in a map keyed by client id means
/// one client silently bound twice.
pub fn normalize_client_id(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

/// Whether this id means Claude Desktop.
///
/// Kept next to [`normalize_client_id`] because Desktop is the only client with
/// teardown semantics — releasing it puts Desktop back on the user's own Claude
/// account — so the predicate is consulted from more than one place and must not
/// drift between them.
///
/// The `contains` arm matches the dispatch in `profile_switch::write_client_config`,
/// which has always accepted a suffixed id.
pub fn is_claude_desktop(client_id: &str) -> bool {
    let clean = normalize_client_id(client_id);
    clean == "claude-desktop" || clean.contains("desktop")
}

/// Normalize, drop blanks, dedupe, and sort a caller-supplied id list.
///
/// Sorted so a `SwitchResult` and the configs it wrote come back in a stable
/// order rather than the caller's incidental one.
pub fn normalize_client_ids<I, S>(raw: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut ids: Vec<String> = raw
        .into_iter()
        .map(|id| normalize_client_id(id.as_ref()))
        .filter(|id| !id.is_empty())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_folds_case_and_padding() {
        assert_eq!(normalize_client_id("  Codex-CLI "), "codex-cli");
        assert_eq!(normalize_client_id("CLAUDE-CODE"), "claude-code");
        assert_eq!(normalize_client_id(""), "");
        assert_eq!(normalize_client_id("   "), "");
    }

    /// The suffixed form is what `write_client_config` has always accepted, so
    /// the predicate has to keep matching it or a bound Desktop would stop being
    /// recognized as one.
    #[test]
    fn desktop_predicate_matches_the_dispatch() {
        for yes in [
            "claude-desktop",
            "Claude-Desktop",
            "  claude-desktop  ",
            "claude-desktop-canary",
            "desktop",
        ] {
            assert!(is_claude_desktop(yes), "{yes} 应识别为 Desktop");
        }
        for no in ["claude-code", "codex-cli", "hermes", "cursor", ""] {
            assert!(!is_claude_desktop(no), "{no} 不应识别为 Desktop");
        }
    }

    /// Two spellings of one client must collapse; otherwise a client-keyed map
    /// holds it twice and both entries claim a profile.
    #[test]
    fn normalize_list_dedupes_across_spellings() {
        let ids = normalize_client_ids(["Codex-CLI", " codex-cli", "claude-code", "", "  "]);
        assert_eq!(
            ids,
            vec!["claude-code".to_string(), "codex-cli".to_string()]
        );
    }

    #[test]
    fn normalize_list_is_sorted_regardless_of_input_order() {
        let a = normalize_client_ids(["hermes", "claude-code", "codex-cli"]);
        let b = normalize_client_ids(["codex-cli", "hermes", "claude-code"]);
        assert_eq!(a, b);
        assert_eq!(
            a,
            vec![
                "claude-code".to_string(),
                "codex-cli".to_string(),
                "hermes".to_string()
            ]
        );
    }
}
