//! Injection lifecycle selection. Native scripts stay preferred; CDP is opt-in.

use crate::{BridgeHandler, BridgeRequest, BridgeResponse, BridgeServer, CdpClient, ScriptManager, ScriptPaths, ScriptStatus};
use polydeck_core::profile::InjectionSettings;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InjectionChannel { NativeUserScript, Cdp, None }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InjectionStage { Stopped, NativeReady, BridgeRunning, Unavailable, Failed }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InjectStatus {
    pub stage: InjectionStage,
    pub channel: InjectionChannel,
    pub native: ScriptStatus,
    pub message: Option<String>,
}

pub struct InjectionManager {
    scripts: ScriptManager,
    script_source: String,
    bridge: Option<BridgeServer>,
    status: InjectStatus,
}

impl InjectionManager {
    pub fn new(script_source: impl Into<String>) -> Self {
        let script_source = script_source.into();
        let scripts = ScriptManager::new(
            ScriptPaths::platform_default().unwrap_or_else(|| ScriptPaths::from_root(".ai-deck-codex")),
            script_source.clone(),
        );
        let native = scripts.status().unwrap_or_else(|_| unavailable_script_status());
        Self {
            scripts, script_source, bridge: None,
            status: InjectStatus { stage: InjectionStage::Stopped, channel: InjectionChannel::None, native, message: None },
        }
    }

    pub fn status(&self) -> &InjectStatus { &self.status }

    pub fn install_native(&mut self) -> Result<InjectStatus, String> {
        self.status.native = self.scripts.install().map_err(|e| e.to_string())?;
        self.status.stage = InjectionStage::NativeReady;
        self.status.channel = InjectionChannel::NativeUserScript;
        self.status.message = Some("Restart Codex++ to load AI Deck native script.".into());
        Ok(self.status.clone())
    }

    pub fn repair(&mut self) -> Result<InjectStatus, String> {
        self.status.native = self.scripts.repair().map_err(|e| e.to_string())?;
        self.status.stage = InjectionStage::NativeReady;
        self.status.channel = InjectionChannel::NativeUserScript;
        Ok(self.status.clone())
    }

    pub fn uninstall_native(&mut self) -> Result<InjectStatus, String> {
        self.status.native = self.scripts.uninstall().map_err(|e| e.to_string())?;
        self.status.stage = InjectionStage::Stopped;
        self.status.channel = InjectionChannel::None;
        Ok(self.status.clone())
    }

    pub async fn start(&mut self, settings: &InjectionSettings, handler: BridgeHandler) -> Result<InjectStatus, String> {
        if !settings.enabled { return Err("Codex enhancement is disabled.".into()); }
        if self.bridge.is_none() {
            self.bridge = Some(BridgeServer::start(handler).await
                .map_err(|_| "Could not start loopback injection bridge.".to_string())?);
        }
        self.scripts.set_script_source(self.script_source.clone());
        self.status.native = self.scripts.install().map_err(|e| e.to_string())?;
        self.status.stage = InjectionStage::BridgeRunning;
        self.status.channel = InjectionChannel::NativeUserScript;
        self.status.message = Some("Bridge running on local loopback.".into());
        self.notify(json!({"kind":"configure","config":renderer_config(settings)})).await;
        Ok(self.status.clone())
    }

    pub async fn attach_verified_cdp(&mut self, settings: &InjectionSettings, port: u16, handler: BridgeHandler) -> Result<InjectStatus, String> {
        if !settings.enabled {
            return Err("CDP attachment is disabled in Codex enhancement settings.".into());
        }
        if self.bridge.is_none() {
            self.bridge = Some(BridgeServer::start(handler).await
                .map_err(|_| "Could not start loopback injection bridge.".to_string())?);
        }
        let client = CdpClient::default();
        let targets = client.probe(port).await.map_err(|e| e.to_string())?;
        let target = CdpClient::select_codex_target(&targets).map_err(|e| e.to_string())?;
        let bootstrap = self.bridge.as_ref().expect("bridge started above").bootstrap_source();
        let rendered_script = self.script_source.replacen("(() => {", &format!("{bootstrap}\n(() => {{"), 1);
        client.install_fixed_script(&target, &rendered_script).await.map_err(|e| e.to_string())?;
        self.status.stage = InjectionStage::BridgeRunning;
        self.status.channel = InjectionChannel::Cdp;
        self.status.message = Some("Attached to a verified loopback Codex CDP target.".into());
        self.notify(json!({"kind":"configure","config":renderer_config(settings)})).await;
        Ok(self.status.clone())
    }

    pub async fn notify(&self, notification: Value) {
        if let Some(bridge) = &self.bridge { bridge.notify(notification).await; }
    }

    pub async fn bridge_status(&self) -> Option<crate::BridgeStatus> {
        match &self.bridge { Some(bridge) => Some(bridge.status().await), None => None }
    }

    pub async fn stop(&mut self) -> InjectStatus {
        if let Some(bridge) = self.bridge.take() { bridge.stop().await; }
        self.status.stage = InjectionStage::Stopped;
        self.status.channel = InjectionChannel::None;
        self.status.message = None;
        self.status.clone()
    }
}

pub fn renderer_config(settings: &InjectionSettings) -> Value {
    json!({
        "version": 1, "enabled": settings.enabled,
        "features": settings.features,
    })
}

pub fn unsupported_bridge_request(request: BridgeRequest) -> BridgeResponse {
    BridgeResponse::error(request.id, "command is unavailable")
}

fn unavailable_script_status() -> ScriptStatus {
    ScriptStatus { available: false, installed: false, enabled: false, healthy: false, restart_required: false, script_hash: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_config_maps_features_without_secrets() {
        let config = renderer_config(&InjectionSettings {
            enabled: true,
            port_range_start: 9222,
            port_range_end: 9322,
            features: vec!["pluginMarketUnlock".into()],
        });
        assert_eq!(config["enabled"], true);
        assert!(config.get("apiKey").is_none());
    }

    #[test]
    fn unavailable_requests_have_safe_error() {
        let response = unsupported_bridge_request(BridgeRequest {
            id: "request-1".into(), command: crate::BridgeCommand::Ack, payload: Value::Null,
        });
        assert!(!response.ok);
        assert_eq!(response.error.as_deref(), Some("command is unavailable"));
    }
}