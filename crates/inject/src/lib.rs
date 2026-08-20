//! Codex++ injection primitives.
//!
//! CDP is optional and only attaches to positively verified loopback DevTools
//! targets. Native Codex++ user scripts provide the supported fallback.

pub mod bridge;
pub mod cdp_client;
pub mod injection_manager;
pub mod launcher;
pub mod script_manager;
pub mod stepwise;
pub mod worktree;

pub use bridge::{
    BridgeCommand, BridgeFuture, BridgeHandler, BridgeRequest, BridgeResponse, BridgeServer,
    BridgeStatus,
};
pub use cdp_client::{CdpClient, CdpError, CdpTarget, CdpVersion};
pub use injection_manager::{
    renderer_config, InjectStatus, InjectionChannel, InjectionManager, InjectionStage,
};
pub use launcher::{detect_runtime, launch_policy, LaunchPolicy, RuntimeKind};
pub use script_manager::{ScriptManager, ScriptManagerError, ScriptPaths, ScriptStatus};
pub use stepwise::{
    CredentialResolver, KeyringCredentialResolver, StepwiseCache, StepwiseError, StepwiseService,
};
pub use worktree::{
    create as create_worktree, open_in_editor, preflight as preflight_worktree, WorktreeConfig,
    WorktreeError, WorktreeResult,
};
