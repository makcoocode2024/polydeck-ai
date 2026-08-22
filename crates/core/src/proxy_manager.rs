//! Proxy tool detection and management.
//! Detects Clash Verge, Mihomo, v2rayN, Sing-box, Shadowsocks, Charles, and Windows system proxy settings.

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ProxyToolInfo {
    pub name: String,
    pub detected: bool,
    pub port: Option<u16>,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub tools: Vec<ProxyToolInfo>,
    pub active_proxy: Option<String>,
    pub http_proxy: Option<String>,
    pub https_proxy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ProxySettings {
    pub custom_proxy_path: Option<String>,
    pub auto_detect: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub enum ProxyRunState {
    Running,
    Stopped,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct LatencyResult {
    pub target: String,
    pub latency_ms: Option<u64>,
    pub success: bool,
}

pub struct ProxyManager {
    pub settings: ProxySettings,
}

impl Default for ProxyManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ProxyManager {
    pub fn new() -> Self {
        Self {
            settings: ProxySettings {
                custom_proxy_path: None,
                auto_detect: true,
            },
        }
    }

    pub fn detect_tools(&self) -> Vec<ProxyToolInfo> {
        let tool_candidates: Vec<(&str, &[u16])> = vec![
            (
                "Clash / Mihomo / Clash Verge",
                &[7890, 7897, 7891, 7892, 7893, 7895, 7896, 9090, 9097],
            ),
            ("v2rayN / Xray", &[10809, 10808, 10810, 10811]),
            ("Sing-box", &[2080, 20808, 2081, 5353]),
            ("Shadowsocks", &[1080, 1081]),
            ("Fiddler / Charles / Proxyman", &[8888, 9090]),
            ("Nekoray / Matsuri", &[2080, 2081]),
            ("Watt Toolkit (Steam++)", &[1082]),
        ];

        let mut results = Vec::new();

        for (name, ports) in tool_candidates {
            let mut detected_port = None;
            let mut is_running = false;

            for &p in ports {
                if is_port_listening(p) {
                    detected_port = Some(p);
                    is_running = true;
                    break;
                }
            }

            if is_running {
                results.push(ProxyToolInfo {
                    name: name.to_string(),
                    detected: true,
                    port: detected_port,
                    running: true,
                });
            }
        }

        // If no tool ports were active, add common tools with detected: false
        if results.is_empty() {
            results.push(ProxyToolInfo {
                name: "Clash / Mihomo / Verge".to_string(),
                detected: false,
                port: Some(7897),
                running: false,
            });
            results.push(ProxyToolInfo {
                name: "v2rayN / Xray".to_string(),
                detected: false,
                port: Some(10809),
                running: false,
            });
            results.push(ProxyToolInfo {
                name: "Sing-box".to_string(),
                detected: false,
                port: Some(2080),
                running: false,
            });
        }

        results
    }

    pub fn get_status(&self) -> ProxyStatus {
        let tools = self.detect_tools();
        let sys_proxy = detect_system_proxy();
        let env_http = std::env::var("HTTP_PROXY")
            .or_else(|_| std::env::var("http_proxy"))
            .ok();
        let env_https = std::env::var("HTTPS_PROXY")
            .or_else(|_| std::env::var("https_proxy"))
            .ok();
        let env_all = std::env::var("ALL_PROXY")
            .or_else(|_| std::env::var("all_proxy"))
            .ok();

        let active_proxy = sys_proxy
            .or_else(|| env_https.clone())
            .or_else(|| env_http.clone())
            .or_else(|| env_all)
            .or_else(|| {
                tools
                    .iter()
                    .find(|t| t.running && t.port.is_some())
                    .map(|t| format!("http://127.0.0.1:{}", t.port.unwrap()))
            });

        ProxyStatus {
            tools,
            active_proxy,
            http_proxy: env_http,
            https_proxy: env_https,
        }
    }
}

pub fn get_configured_proxy() -> Option<String> {
    let mgr = ProxyManager::new();
    let status = mgr.get_status();
    status.active_proxy.map(|p| {
        let clean = p.trim();
        if !clean.starts_with("http://")
            && !clean.starts_with("https://")
            && !clean.starts_with("socks5://")
        {
            format!("http://{clean}")
        } else {
            clean.to_string()
        }
    })
}

fn is_port_listening(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(40)).is_ok()
}

pub fn parse_proxy_server_string(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    // Handles format like: "http=127.0.0.1:7890;https=127.0.0.1:7890;socks=127.0.0.1:10808"
    if raw.contains('=') {
        for part in raw.split(';') {
            let part = part.trim();
            if part.starts_with("https=") {
                let target = part.trim_start_matches("https=").trim();
                return Some(normalize_proxy_scheme(target, "http://"));
            }
            if part.starts_with("http=") {
                let target = part.trim_start_matches("http=").trim();
                return Some(normalize_proxy_scheme(target, "http://"));
            }
            if part.starts_with("socks=") {
                let target = part.trim_start_matches("socks=").trim();
                return Some(normalize_proxy_scheme(target, "socks5://"));
            }
        }
    }

    // Plain format like: "127.0.0.1:7890" or "http://127.0.0.1:7890"
    Some(normalize_proxy_scheme(raw, "http://"))
}

fn normalize_proxy_scheme(target: &str, default_scheme: &str) -> String {
    let t = target.trim();
    if t.starts_with("http://")
        || t.starts_with("https://")
        || t.starts_with("socks5://")
        || t.starts_with("socks5h://")
    {
        t.to_string()
    } else {
        format!("{default_scheme}{t}")
    }
}

pub fn detect_system_proxy() -> Option<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let output = std::process::Command::new("reg")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                "/v",
                "ProxyEnable",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;

        let text = String::from_utf8_lossy(&output.stdout);
        let enabled = text.lines().any(|line| {
            line.contains("ProxyEnable") && (line.contains("0x1") || line.contains("1"))
        });

        if enabled {
            let server_output = std::process::Command::new("reg")
                .args([
                    "query",
                    r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
                    "/v",
                    "ProxyServer",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .ok()?;

            let server_text = String::from_utf8_lossy(&server_output.stdout);
            for line in server_text.lines() {
                if line.contains("ProxyServer") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 3 {
                        let raw_server = parts[2..].join(" ");
                        if let Some(parsed) = parse_proxy_server_string(&raw_server) {
                            return Some(parsed);
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proxy_formats() {
        assert_eq!(
            parse_proxy_server_string("127.0.0.1:7890"),
            Some("http://127.0.0.1:7890".to_string())
        );
        assert_eq!(
            parse_proxy_server_string("http=127.0.0.1:7890;https=127.0.0.1:7890"),
            Some("http://127.0.0.1:7890".to_string())
        );
        assert_eq!(
            parse_proxy_server_string("socks=127.0.0.1:10808"),
            Some("socks5://127.0.0.1:10808".to_string())
        );
        assert_eq!(
            parse_proxy_server_string("http://127.0.0.1:7897"),
            Some("http://127.0.0.1:7897".to_string())
        );
    }
}
