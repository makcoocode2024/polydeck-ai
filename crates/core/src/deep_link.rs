//! Deep link handler for aideck:// protocol.
//!
//! Supports: profile/new, profile/switch/{id}, import?data={base64}
//! API keys from deep links go straight to keyring, never to state.json.

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub enum DeepLinkAction {
    CreateProfile { name: Option<String> },
    SwitchProfile { id: String },
    ImportProfile { data: String },
    Unknown { path: String },
}

pub fn parse(url: &str) -> AppResult<DeepLinkAction> {
    let url = url.trim();
    let path = url
        .strip_prefix("aideck://")
        .ok_or_else(|| AppError::InvalidInput("不是有效的 aideck:// 链接".into()))?;

    if path.starts_with("profile/new") {
        Ok(DeepLinkAction::CreateProfile { name: None })
    } else if let Some(id) = path.strip_prefix("profile/switch/") {
        Ok(DeepLinkAction::SwitchProfile { id: id.to_string() })
    } else if path.starts_with("import") {
        let data = path
            .split("data=")
            .nth(1)
            .unwrap_or_default()
            .to_string();
        Ok(DeepLinkAction::ImportProfile { data })
    } else {
        Ok(DeepLinkAction::Unknown { path: path.to_string() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_switch_link() {
        match parse("aideck://profile/switch/abc123").unwrap() {
            DeepLinkAction::SwitchProfile { id } => assert_eq!(id, "abc123"),
            _ => panic!("wrong variant"),
        }
    }
}
