//! Claude Desktop's third-party (3P) endpoint configuration.
//!
//! Desktop keeps its own account login in one data directory and its
//! third-party endpoint in a *separate* one — `Claude` and `Claude-3p`. The
//! endpoint is not in `claude_desktop_config.json` next to the MCP servers,
//! which is why a search of the roaming directory turns up nothing and makes the
//! setting look account-side. It is local, and this module writes it.
//!
//! Four files take part:
//!
//! | File | Role |
//! |---|---|
//! | `<normal>/claude_desktop_config.json` | `deploymentMode` |
//! | `<3p>/claude_desktop_config.json` | `deploymentMode` |
//! | `<3p>/configLibrary/<uuid>.json` | the endpoint profile |
//! | `<3p>/configLibrary/_meta.json` | which profile is applied |
//!
//! Both `claude_desktop_config.json` files also hold unrelated user
//! preferences, so `deploymentMode` is merged in rather than written over.

use crate::error::{AppError, AppResult};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// PolyDeck's own profile id, distinct from any other tool's.
///
/// Fixed rather than per-profile: Desktop shows one entry per id, and a new id
/// on every profile switch would pile up dead rows in its config library.
pub const PROFILE_UUID: &str = "9d1f5a6c-0b7e-4a21-9c34-b01d5eca0000";

/// The name shown against our entry in Desktop's config library.
pub const PROFILE_NAME: &str = "PolyDeck";

const CONFIG_FILE: &str = "claude_desktop_config.json";
const CONFIG_LIBRARY_DIR: &str = "configLibrary";
const META_FILE: &str = "_meta.json";

/// One tier as Desktop's model menu should show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceModel {
    /// The name that travels on the wire.
    pub name: String,
    /// Shown instead of `name`, when they differ.
    pub label_override: Option<String>,
    /// Whether this tier can serve a 1M context window.
    pub supports_1m: bool,
}

/// Everything Desktop needs to reach one endpoint.
#[derive(Debug, Clone)]
pub struct EndpointSpec {
    /// Base URL with no `/v1` suffix — Desktop appends the path itself.
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<InferenceModel>,
}

/// The pair of directories Desktop uses, when this platform has them.
fn data_dirs() -> Option<(PathBuf, PathBuf)> {
    #[cfg(windows)]
    {
        let root = crate::local_app_data_dir()?;
        Some((
            pick_dir(&root, false).unwrap_or_else(|| root.join("Claude")),
            pick_dir(&root, true).unwrap_or_else(|| root.join("Claude-3p")),
        ))
    }
    #[cfg(target_os = "macos")]
    {
        let root = crate::roaming_app_data_dir()?;
        Some((root.join("Claude"), root.join("Claude-3p")))
    }
    // Desktop ships no Linux build, so there is nothing to write.
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        None
    }
}

/// Find Desktop's data directory, tolerating a suffixed install.
///
/// Installers have shipped directories beyond the plain `Claude` /
/// `Claude-3p`, so an exact miss falls back to scanning for the right shape.
#[cfg(any(windows, test))]
fn pick_dir(root: &Path, threep: bool) -> Option<PathBuf> {
    let exact = root.join(if threep { "Claude-3p" } else { "Claude" });
    if exact.is_dir() {
        return Some(exact);
    }
    let mut hits: Vec<PathBuf> = std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("Claude") && name.contains("-3p") == threep)
        })
        .collect();
    hits.sort();
    hits.into_iter().next()
}

/// Read a JSON object, treating absent or unparseable as empty.
///
/// Unparseable is deliberately not an error: refusing to write because Desktop
/// left a half-flushed file would strand the user with no way to fix it from
/// PolyDeck.
fn read_object(path: &Path) -> Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

fn write_json(path: &Path, value: &Value) -> AppResult<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    crate::storage::atomic_replace(path, &bytes)
}

/// Merge `deploymentMode` into a config file, preserving every other key.
fn set_deployment_mode(path: &Path, mode: &str) -> AppResult<()> {
    let mut config = read_object(path);
    config.insert("deploymentMode".into(), json!(mode));
    write_json(path, &Value::Object(config))
}

/// Rewrite `_meta.json`, claiming or releasing our own entry.
///
/// Entries belonging to other tools are carried through untouched — a user's
/// hand-made Desktop profile lives in this same list, and dropping it would
/// silently delete configuration PolyDeck does not own.
fn write_meta(path: &Path, claim: bool) -> AppResult<()> {
    let mut meta = read_object(path);

    let mut entries: Vec<Value> = meta
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    entries.retain(|entry| entry.get("id").and_then(Value::as_str) != Some(PROFILE_UUID));

    if claim {
        entries.push(json!({ "id": PROFILE_UUID, "name": PROFILE_NAME }));
        meta.insert("appliedId".into(), json!(PROFILE_UUID));
    } else if meta.get("appliedId").and_then(Value::as_str) == Some(PROFILE_UUID) {
        // Only reassign when we are the one applied; otherwise the user's own
        // selection stands.
        match entries
            .iter()
            .find_map(|entry| entry.get("id").and_then(Value::as_str))
        {
            Some(next) => {
                meta.insert("appliedId".into(), json!(next));
            }
            None => {
                meta.remove("appliedId");
            }
        }
    }

    meta.insert("entries".into(), Value::Array(entries));
    write_json(path, &Value::Object(meta))
}

/// Whether Claude Desktop's model menu will accept this name.
///
/// Desktop validates the whole `inferenceModels` array and rejects **all** of it
/// if any entry is malformed, so one unusable name costs every tier. The shape it
/// requires is `claude-` (or `anthropic/claude-`) followed by a role and a
/// non-empty identifier: `claude-opus-5` passes, a relay's own `model-T` does
/// not, and a degenerate `claude-opus-` does not either.
///
/// The `[1m]` marker is a Claude Code convention; Desktop expresses the same
/// thing with the separate `supports1m` field and refuses the suffix.
fn is_menu_safe(model: &str) -> bool {
    let normalized = model.trim().to_ascii_lowercase();
    if normalized.contains("[1m]") {
        return false;
    }
    let Some(tail) = normalized
        .strip_prefix("anthropic/claude-")
        .or_else(|| normalized.strip_prefix("claude-"))
    else {
        return false;
    };
    ["sonnet-", "opus-", "haiku-", "fable-"]
        .iter()
        .any(|role| tail.strip_prefix(role).is_some_and(|rest| !rest.is_empty()))
}

fn model_json(model: &InferenceModel) -> Value {
    if model.label_override.is_none() && !model.supports_1m {
        return json!(model.name);
    }
    let mut item = json!({ "name": model.name });
    if let Some(label) = &model.label_override {
        item["labelOverride"] = json!(label);
    }
    if model.supports_1m {
        item["supports1m"] = json!(true);
    }
    item
}

/// Point Claude Desktop at `spec` and switch it into third-party mode.
///
/// Write order matters. The profile and its `_meta.json` reference land first,
/// and the two `deploymentMode` flips last, so a failure part-way leaves Desktop
/// still on the user's Claude account with at most an unreferenced profile file.
/// Flipping the mode first would risk Desktop starting in 3P mode pointed at a
/// profile that was never written.
pub fn apply(spec: &EndpointSpec) -> AppResult<()> {
    if spec.api_key.trim().is_empty() {
        // `inferenceCredentialKind: "static"` with a blank key surfaces inside
        // Desktop as an opaque auth failure, so refuse before touching a file.
        return Err(AppError::Config(
            "Claude Desktop 需要一个 API Key，当前为空。请在方案里填好凭据后重试".into(),
        ));
    }

    // Drop what Desktop's validator would choke on, rather than let one bad name
    // take the whole menu down with it.
    let (menu, rejected): (Vec<_>, Vec<_>) = spec
        .models
        .iter()
        .cloned()
        .partition(|model| is_menu_safe(&model.name));
    if menu.is_empty() {
        let names: Vec<&str> = rejected.iter().map(|m| m.name.as_str()).collect();
        return Err(AppError::Config(format!(
            "没有一个模型名是 Claude Desktop 能接受的（它只认 claude-<opus|sonnet|haiku|fable>-… 这种形状），被拒的是：{}。\
             开启网关即可让它拿到档位名",
            names.join("、")
        )));
    }

    let (normal_dir, threep_dir) = data_dirs()
        .ok_or_else(|| AppError::Config("当前平台没有 Claude Desktop 的第三方配置目录".into()))?;
    let library = threep_dir.join(CONFIG_LIBRARY_DIR);

    let profile = json!({
        "inferenceProvider": "gateway",
        "inferenceGatewayBaseUrl": spec.base_url,
        "inferenceGatewayApiKey": spec.api_key,
        "inferenceGatewayAuthScheme": "bearer",
        "inferenceCredentialKind": "static",
        "inferenceModels": menu.iter().map(model_json).collect::<Vec<_>>(),
        "coworkEgressAllowedHosts": ["*"],
        "disableDeploymentModeChooser": true,
    });

    write_json(&library.join(format!("{PROFILE_UUID}.json")), &profile)?;
    write_meta(&library.join(META_FILE), true)?;
    set_deployment_mode(&threep_dir.join(CONFIG_FILE), "3p")?;
    set_deployment_mode(&normal_dir.join(CONFIG_FILE), "3p")?;

    Ok(())
}

/// Hand Claude Desktop back to its own account login.
///
/// Every step is best-effort: a half-finished restore is worse than a finished
/// one, so a failure to drop our profile should not stop the mode from going
/// back to `1p`. Files that do not exist are left alone rather than created,
/// since writing `1p` into a directory Desktop never made would be inventing
/// configuration.
pub fn restore() -> AppResult<()> {
    let Some((normal_dir, threep_dir)) = data_dirs() else {
        return Ok(());
    };
    if !threep_dir.is_dir() {
        return Ok(());
    }

    for dir in [&normal_dir, &threep_dir] {
        let config = dir.join(CONFIG_FILE);
        if config.exists() {
            let _ = set_deployment_mode(&config, "1p");
        }
    }

    let library = threep_dir.join(CONFIG_LIBRARY_DIR);
    let meta = library.join(META_FILE);
    if meta.exists() {
        let _ = write_meta(&meta, false);
    }
    let profile = library.join(format!("{PROFILE_UUID}.json"));
    if profile.exists() {
        let _ = std::fs::remove_file(&profile);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A profile id belonging to some other tool, standing in for one the user
    /// made by hand in Desktop's own configuration window.
    ///
    /// Deliberately not a plausible real id: an earlier version of this fixture
    /// reused a real one from a live machine, which meant that when the path
    /// isolation was briefly broken the test overwrote that machine's actual
    /// `_meta.json` with content that happened to match — damage that its own
    /// assertions could not see.
    const FOREIGN_ID: &str = "test-fixture-not-a-real-profile-id";

    fn spec() -> EndpointSpec {
        EndpointSpec {
            base_url: "http://127.0.0.1:18888".into(),
            api_key: "k".into(),
            models: vec![InferenceModel {
                name: "claude-opus-5".into(),
                label_override: None,
                supports_1m: false,
            }],
        }
    }

    /// A temp home with the 3P tree already present, plus the guard that keeps
    /// the override from leaking into another test.
    fn temp_desktop() -> (
        tempfile::TempDir,
        std::sync::MutexGuard<'static, ()>,
        PathBuf,
    ) {
        let guard = crate::lock_home_env();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_DECK_HOME_OVERRIDE", home.path());
        let (_, threep) = data_dirs().expect("temp home must yield data dirs");
        std::fs::create_dir_all(threep.join(CONFIG_LIBRARY_DIR)).unwrap();
        (home, guard, threep)
    }

    fn read_meta(threep: &Path) -> Map<String, Value> {
        read_object(&threep.join(CONFIG_LIBRARY_DIR).join(META_FILE))
    }

    fn entry_ids(meta: &Map<String, Value>) -> Vec<String> {
        meta.get("entries")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|e| e.get("id").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The user's own Desktop profile shares `_meta.json` with ours, so neither
    /// applying nor restoring may drop it.
    #[test]
    fn meta_merge_keeps_foreign_entries() {
        let (_home, _guard, threep) = temp_desktop();
        let meta_path = threep.join(CONFIG_LIBRARY_DIR).join(META_FILE);
        write_json(
            &meta_path,
            &json!({
                "appliedId": FOREIGN_ID,
                "entries": [{ "id": FOREIGN_ID, "name": "111" }],
            }),
        )
        .unwrap();

        apply(&spec()).unwrap();
        let meta = read_meta(&threep);
        assert_eq!(
            meta.get("appliedId").and_then(Value::as_str),
            Some(PROFILE_UUID),
            "apply 后应由 PolyDeck 接管 appliedId"
        );
        let ids = entry_ids(&meta);
        assert!(
            ids.contains(&FOREIGN_ID.to_string()),
            "外来 entry 不能被删掉"
        );
        assert!(ids.contains(&PROFILE_UUID.to_string()));

        restore().unwrap();
        let meta = read_meta(&threep);
        assert_eq!(
            meta.get("appliedId").and_then(Value::as_str),
            Some(FOREIGN_ID),
            "restore 后 appliedId 应回到用户自己的 profile"
        );
        let ids = entry_ids(&meta);
        assert_eq!(ids, vec![FOREIGN_ID.to_string()]);
        assert!(
            !threep
                .join(CONFIG_LIBRARY_DIR)
                .join(format!("{PROFILE_UUID}.json"))
                .exists(),
            "restore 应删掉 PolyDeck 自己那份 profile"
        );
    }

    /// Both config files carry unrelated user preferences.
    #[test]
    fn deployment_mode_merge_preserves_other_keys() {
        let (_home, _guard, threep) = temp_desktop();
        let (normal, _) = data_dirs().unwrap();
        std::fs::create_dir_all(&normal).unwrap();
        for dir in [&normal, &threep] {
            write_json(
                &dir.join(CONFIG_FILE),
                &json!({
                    "coworkUserFilesPath": "C:\\Users\\admin\\Claude",
                    "preferences": { "sidebarMode": "chat" },
                }),
            )
            .unwrap();
        }

        apply(&spec()).unwrap();

        for dir in [&normal, &threep] {
            let config = read_object(&dir.join(CONFIG_FILE));
            assert_eq!(
                config.get("deploymentMode").and_then(Value::as_str),
                Some("3p")
            );
            assert!(
                config.get("coworkUserFilesPath").is_some(),
                "{}: 用户偏好不能被覆写掉",
                dir.display()
            );
            assert_eq!(
                config["preferences"]["sidebarMode"].as_str(),
                Some("chat"),
                "{}: 嵌套偏好也要保住",
                dir.display()
            );
        }

        restore().unwrap();
        for dir in [&normal, &threep] {
            let config = read_object(&dir.join(CONFIG_FILE));
            assert_eq!(
                config.get("deploymentMode").and_then(Value::as_str),
                Some("1p")
            );
            assert!(config.get("coworkUserFilesPath").is_some());
        }
    }

    /// Nothing to restore, and no reason to invent a directory tree.
    #[test]
    fn restore_is_a_noop_without_a_3p_dir() {
        let _guard = crate::lock_home_env();
        let home = tempfile::tempdir().unwrap();
        std::env::set_var("AI_DECK_HOME_OVERRIDE", home.path());

        restore().unwrap();

        if let Some((_, threep)) = data_dirs() {
            assert!(!threep.exists(), "restore 不应把 3P 目录建出来");
        }
    }

    /// A blank key with `inferenceCredentialKind: "static"` surfaces as an opaque
    /// auth failure, so it must be refused before any file is touched.
    #[test]
    fn blank_api_key_is_refused_before_writing() {
        let (_home, _guard, threep) = temp_desktop();
        let mut blank = spec();
        blank.api_key = "  ".into();

        assert!(apply(&blank).is_err());
        assert!(
            !threep
                .join(CONFIG_LIBRARY_DIR)
                .join(format!("{PROFILE_UUID}.json"))
                .exists(),
            "被拒绝时不应留下 profile 文件"
        );
        assert!(
            !threep.join(CONFIG_FILE).exists(),
            "被拒绝时不应翻转 deploymentMode"
        );
    }

    /// A bare string when there is nothing extra to say, an object otherwise.
    #[test]
    fn model_json_shape_depends_on_extras() {
        assert_eq!(
            model_json(&InferenceModel {
                name: "claude-opus-5".into(),
                label_override: None,
                supports_1m: false,
            }),
            json!("claude-opus-5")
        );

        let decorated = model_json(&InferenceModel {
            name: "claude-opus-5-max".into(),
            label_override: Some("claude-opus-5".into()),
            supports_1m: true,
        });
        assert_eq!(decorated["name"].as_str(), Some("claude-opus-5-max"));
        assert_eq!(decorated["labelOverride"].as_str(), Some("claude-opus-5"));
        assert_eq!(decorated["supports1m"].as_bool(), Some(true));
    }

    /// The written profile must not carry a `/v1` suffix, whatever it was given.
    #[test]
    fn base_url_is_written_verbatim_without_a_v1_suffix() {
        let (_home, _guard, threep) = temp_desktop();
        apply(&spec()).unwrap();

        let profile = read_object(
            &threep
                .join(CONFIG_LIBRARY_DIR)
                .join(format!("{PROFILE_UUID}.json")),
        );
        let base = profile
            .get("inferenceGatewayBaseUrl")
            .and_then(Value::as_str)
            .unwrap();
        assert_eq!(base, "http://127.0.0.1:18888");
        assert!(
            !base.ends_with("/v1"),
            "带 /v1 会让 Desktop 请求 /v1/v1/messages 并 404"
        );
        assert_eq!(
            profile.get("inferenceProvider").and_then(Value::as_str),
            Some("gateway")
        );
        assert_eq!(
            profile
                .get("inferenceGatewayAuthScheme")
                .and_then(Value::as_str),
            Some("bearer")
        );
    }

    /// Desktop rejects the whole `inferenceModels` array if one entry is
    /// malformed, so the shapes it accepts have to be pinned exactly.
    #[test]
    fn menu_safety_matches_desktops_validator() {
        for good in [
            "claude-opus-5",
            "claude-opus-5-max",
            "claude-sonnet-5",
            "claude-haiku-4-5",
            "claude-fable-5",
            "anthropic/claude-opus-5",
            "Claude-Opus-5", // 大小写不敏感
        ] {
            assert!(is_menu_safe(good), "{good} 应被接受");
        }

        for bad in [
            "model-T",           // 中转自己的名字，没有 claude- 前缀
            "gpt-5.6-sol",       // 同上
            "claude-opus-",      // 退化值，角色后没有标识
            "claude-5-opus",     // 角色不在紧跟前缀的位置
            "claude-opus-5[1m]", // Desktop 用 supports1m 字段表达，不认这个后缀
            "opus",              // 裸别名
            "",
        ] {
            assert!(!is_menu_safe(bad), "{bad} 应被拒绝");
        }
    }

    /// A relay tier pointing at a non-Claude name must not take the other tiers
    /// down with it.
    #[test]
    fn unsafe_model_names_are_dropped_not_written() {
        let (_home, _guard, threep) = temp_desktop();
        let mut mixed = spec();
        mixed.models = vec![
            InferenceModel {
                name: "claude-opus-5-max".into(),
                label_override: None,
                supports_1m: false,
            },
            // 用户 sotamodel 方案里 haiku 档的真实取值。
            InferenceModel {
                name: "model-T".into(),
                label_override: None,
                supports_1m: false,
            },
        ];

        apply(&mixed).unwrap();

        let profile = read_object(
            &threep
                .join(CONFIG_LIBRARY_DIR)
                .join(format!("{PROFILE_UUID}.json")),
        );
        let written: Vec<String> = profile["inferenceModels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                m.as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| m["name"].as_str().unwrap().to_string())
            })
            .collect();
        assert_eq!(
            written,
            vec!["claude-opus-5-max".to_string()],
            "不合规的名字要被丢掉，合规的要留下"
        );
    }

    /// Nothing usable means the profile would be rejected wholesale, so say so
    /// instead of writing it.
    #[test]
    fn all_unsafe_model_names_is_an_error() {
        let (_home, _guard, threep) = temp_desktop();
        let mut all_bad = spec();
        all_bad.models = vec![InferenceModel {
            name: "model-T".into(),
            label_override: None,
            supports_1m: false,
        }];

        let err = apply(&all_bad).unwrap_err().to_string();
        assert!(err.contains("model-T"), "报错要点出是哪个名字，实际：{err}");
        assert!(
            !threep
                .join(CONFIG_LIBRARY_DIR)
                .join(format!("{PROFILE_UUID}.json"))
                .exists(),
            "不该留下一份 Desktop 会整组拒收的 profile"
        );
    }

    /// An install that suffixed its directory still gets found.
    #[test]
    fn pick_dir_falls_back_to_a_suffixed_install() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("Claude-3p-canary")).unwrap();
        std::fs::create_dir_all(root.path().join("ClaudeBeta")).unwrap();

        assert_eq!(
            pick_dir(root.path(), true).unwrap(),
            root.path().join("Claude-3p-canary")
        );
        assert_eq!(
            pick_dir(root.path(), false).unwrap(),
            root.path().join("ClaudeBeta")
        );
    }
}
