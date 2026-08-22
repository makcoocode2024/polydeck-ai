//! Safe upstream worktree and editor launch planning.

use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeConfig {
    pub repository: PathBuf,
    pub remote: String,
    pub base_branch: String,
    pub branch: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeResult {
    pub path: PathBuf,
    pub branch: String,
    pub base: String,
}

#[derive(Debug, Error)]
pub enum WorktreeError {
    #[error("not a git repository")]
    NotRepository,
    #[error("remote does not exist: {0}")]
    RemoteMissing(String),
    #[error("base ref does not exist: {0}")]
    BaseMissing(String),
    #[error("branch already exists: {0}")]
    BranchExists(String),
    #[error("target path already exists: {0}")]
    PathExists(PathBuf),
    #[error("fetch failed")]
    FetchFailed,
    #[error("worktree creation failed")]
    AddFailed,
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("editor not found: {0}")]
    EditorNotFound(String),
    #[error("editor launch failed")]
    EditorLaunchFailed,
}

fn create_cmd(executable: &str) -> Command {
    let mut cmd = Command::new(executable);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub fn preflight(config: &WorktreeConfig) -> Result<(), WorktreeError> {
    validate_name(&config.remote)?;
    validate_name(&config.base_branch)?;
    validate_name(&config.branch)?;
    if !config.repository.join(".git").is_file() && !config.repository.join(".git").is_dir() {
        return Err(WorktreeError::NotRepository);
    }
    if config.path.exists() {
        return Err(WorktreeError::PathExists(config.path.clone()));
    }
    if !git_ok(&config.repository, &["remote", "get-url", &config.remote]) {
        return Err(WorktreeError::RemoteMissing(config.remote.clone()));
    }
    if !git_ok(
        &config.repository,
        &["rev-parse", "--verify", &config.base_branch],
    ) {
        return Err(WorktreeError::BaseMissing(config.base_branch.clone()));
    }
    if git_ok(
        &config.repository,
        &[
            "show-ref",
            "--verify",
            &format!("refs/heads/{}", config.branch),
        ],
    ) {
        return Err(WorktreeError::BranchExists(config.branch.clone()));
    }
    Ok(())
}

pub fn create(config: &WorktreeConfig) -> Result<WorktreeResult, WorktreeError> {
    preflight(config)?;
    run_git(&config.repository, &["fetch", &config.remote])
        .map_err(|_| WorktreeError::FetchFailed)?;
    let base = format!("{}/{}", config.remote, config.base_branch);
    run_git(
        &config.repository,
        &[
            "worktree",
            "add",
            "-b",
            &config.branch,
            path_arg(&config.path).as_str(),
            &base,
        ],
    )
    .map_err(|_| WorktreeError::AddFailed)?;
    Ok(WorktreeResult {
        path: config.path.clone(),
        branch: config.branch.clone(),
        base,
    })
}

pub fn open_in_editor(editor: &str, path: &Path) -> Result<(), WorktreeError> {
    if !matches!(editor, "vscode" | "zed") {
        return Err(WorktreeError::EditorNotFound(editor.to_string()));
    }
    let executable = if editor == "vscode" { "code" } else { "zed" };
    create_cmd(executable)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|_| WorktreeError::EditorLaunchFailed)
}

fn path_arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn validate_name(value: &str) -> Result<(), WorktreeError> {
    if value.is_empty() || value.starts_with('-') || value.contains('\0') || value.contains(' ') {
        return Err(WorktreeError::InvalidInput(
            "branch, remote and base use safe names".into(),
        ));
    }
    Ok(())
}

fn git_ok(repository: &Path, args: &[&str]) -> bool {
    create_cmd("git")
        .args(["-C", path_arg(repository).as_str()])
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run_git(repository: &Path, args: &[&str]) -> Result<(), ()> {
    create_cmd("git")
        .args(["-C", path_arg(repository).as_str()])
        .args(args)
        .status()
        .map_err(|_| ())
        .and_then(|s| s.success().then_some(()).ok_or(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_shell_like_branch_input() {
        let config = WorktreeConfig {
            repository: PathBuf::from("."),
            remote: "origin".into(),
            base_branch: "main".into(),
            branch: "feature; echo bad".into(),
            path: PathBuf::from("out"),
        };
        assert!(matches!(
            preflight(&config),
            Err(WorktreeError::InvalidInput(_))
        ));
    }

    #[test]
    fn editor_allowlist_rejects_arbitrary_command() {
        assert!(matches!(
            open_in_editor("powershell", Path::new(".")),
            Err(WorktreeError::EditorNotFound(_))
        ));
    }
}
