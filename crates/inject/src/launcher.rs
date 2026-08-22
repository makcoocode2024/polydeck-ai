//! Conservative Codex runtime launch policy.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchPolicy {
    AttachOnly,
    VerifiedChromiumLaunch,
    NeverLaunch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeKind {
    WebView2,
    Chromium,
    Unknown,
}

pub fn launch_policy(runtime: RuntimeKind, experimental_enabled: bool) -> LaunchPolicy {
    match (runtime, experimental_enabled) {
        (RuntimeKind::Chromium, true) => LaunchPolicy::VerifiedChromiumLaunch,
        (RuntimeKind::WebView2, _) => LaunchPolicy::NeverLaunch,
        _ => LaunchPolicy::AttachOnly,
    }
}

pub fn detect_runtime(executable: &Path) -> RuntimeKind {
    let name = executable
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.contains("webview") || name.contains("ebwebview") {
        RuntimeKind::WebView2
    } else if name.contains("chrome") || name.contains("chromium") {
        RuntimeKind::Chromium
    } else {
        RuntimeKind::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webview2_is_never_launched() {
        assert_eq!(
            launch_policy(RuntimeKind::WebView2, true),
            LaunchPolicy::NeverLaunch
        );
    }

    #[test]
    fn chromium_requires_explicit_opt_in() {
        assert_eq!(
            launch_policy(RuntimeKind::Chromium, false),
            LaunchPolicy::AttachOnly
        );
        assert_eq!(
            launch_policy(RuntimeKind::Chromium, true),
            LaunchPolicy::VerifiedChromiumLaunch
        );
    }

    #[test]
    fn executable_name_detects_webview2() {
        assert_eq!(
            detect_runtime(Path::new("EBWebView.exe")),
            RuntimeKind::WebView2
        );
    }
}
