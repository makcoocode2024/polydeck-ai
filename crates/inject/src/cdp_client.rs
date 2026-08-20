//! Passive Chromium DevTools Protocol discovery and fixed-script attach.

use futures_util::{SinkExt, StreamExt};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use thiserror::Error;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CdpTarget {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default, rename = "type")]
    pub target_type: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    pub websocket_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CdpVersion {
    #[serde(default, rename = "Browser")]
    pub browser: String,
    #[serde(default, rename = "webSocketDebuggerUrl")]
    pub websocket_url: Option<String>,
}

#[derive(Debug, Error)]
pub enum CdpError {
    #[error("CDP endpoint must be loopback")]
    NonLoopback,
    #[error("CDP endpoint unavailable")]
    Unavailable,
    #[error("CDP response malformed")]
    Malformed,
    #[error("Codex page target not found")]
    TargetNotFound,
    #[error("CDP request timed out")]
    Timeout,
    #[error("CDP transport failed: {0}")]
    Transport(String),
    #[error("CDP command failed: {0}")]
    Command(String),
}

#[derive(Clone)]
pub struct CdpClient {
    http: reqwest::Client,
    timeout: Duration,
}

impl Default for CdpClient {
    fn default() -> Self { Self::new(Duration::from_millis(700)) }
}

impl CdpClient {
    pub fn new(timeout: Duration) -> Self {
        Self {
            http: reqwest::Client::builder().timeout(timeout).build().unwrap_or_default(),
            timeout,
        }
    }

    pub async fn probe(&self, port: u16) -> Result<Vec<CdpTarget>, CdpError> {
        let base = Url::parse(&format!("http://127.0.0.1:{port}")).map_err(|_| CdpError::Malformed)?;
        if !is_loopback(base.host_str()) { return Err(CdpError::NonLoopback); }
        let _version: CdpVersion = self.http.get(base.join("/json/version").map_err(|_| CdpError::Malformed)?)
            .send().await.map_err(|_| CdpError::Unavailable)?
            .json().await.map_err(|_| CdpError::Malformed)?;
        self.http.get(base.join("/json/list").map_err(|_| CdpError::Malformed)?)
            .send().await.map_err(|_| CdpError::Unavailable)?
            .json::<Vec<CdpTarget>>().await.map_err(|_| CdpError::Malformed)
    }

    pub fn select_codex_target(targets: &[CdpTarget]) -> Result<CdpTarget, CdpError> {
        targets.iter()
            .filter(|t| t.target_type == "page" && t.websocket_url.as_deref().is_some_and(valid_websocket_url))
            .find(|t| {
                let haystack = format!("{} {}", t.title, t.url).to_ascii_lowercase();
                haystack.contains("codex") || t.url.starts_with("app://openai-codex")
            })
            .cloned().ok_or(CdpError::TargetNotFound)
    }

    pub async fn install_fixed_script(&self, target: &CdpTarget, script: &str) -> Result<(), CdpError> {
        let ws_url = target.websocket_url.as_deref().ok_or(CdpError::TargetNotFound)?;
        if !valid_websocket_url(ws_url) { return Err(CdpError::NonLoopback); }
        let (mut socket, _) = connect_async(ws_url).await.map_err(|e| CdpError::Transport(e.to_string()))?;
        let cmd = json!({"id": 1, "method": "Page.addScriptToEvaluateOnNewDocument", "params": {"source": script}});
        socket.send(Message::Text(cmd.to_string().into())).await.map_err(|e| CdpError::Transport(e.to_string()))?;
        while let Some(message) = tokio::time::timeout(self.timeout, socket.next()).await.map_err(|_| CdpError::Timeout)? {
            let message = message.map_err(|e| CdpError::Transport(e.to_string()))?;
            if let Message::Text(text) = message {
                let response: Value = serde_json::from_str(&text).map_err(|_| CdpError::Malformed)?;
                if response.get("id") == Some(&json!(1)) {
                    if let Some(error) = response.get("error") { return Err(CdpError::Command(error.to_string())); }
                    return Ok(());
                }
            }
        }
        Err(CdpError::Unavailable)
    }
}

fn is_loopback(host: Option<&str>) -> bool {
    host.is_some_and(|h| h == "127.0.0.1" || h == "localhost" || h == "[::1]")
}

fn valid_websocket_url(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else { return false; };
    matches!(url.scheme(), "ws" | "wss") && is_loopback(url.host_str())
}

pub fn is_verified_loopback(address: SocketAddr) -> bool {
    address.ip() == IpAddr::V4(Ipv4Addr::LOCALHOST) || address.ip().is_loopback()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_loopback_ws() { assert!(!valid_websocket_url("ws://192.168.1.2:9222/devtools/page/1")); }

    #[test]
    fn accepts_loopback_ws() { assert!(valid_websocket_url("ws://127.0.0.1:9222/devtools/page/1")); }

    #[test]
    fn selects_codex_page() {
        let target = CdpClient::select_codex_target(&[
            CdpTarget { id: "1".into(), title: "DevTools".into(), url: "devtools://devtools".into(), target_type: "page".into(), websocket_url: Some("ws://127.0.0.1:1/1".into()) },
            CdpTarget { id: "2".into(), title: "Codex".into(), url: "app://openai-codex/".into(), target_type: "page".into(), websocket_url: Some("ws://127.0.0.1:1/2".into()) },
        ]).unwrap();
        assert_eq!(target.id, "2");
    }

    #[test]
    fn verifies_loopback_socket() {
        assert!(is_verified_loopback("127.0.0.1:9222".parse().unwrap()));
        assert!(!is_verified_loopback("10.0.0.1:9222".parse().unwrap()));
    }
}
