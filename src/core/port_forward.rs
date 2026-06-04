//! 端口转发：标签、审计详情、表单解析（与 SSH 层 spawn 解耦，便于单测）。

use serde_json::{json, Value};

use crate::ssh::{DynamicPortForward, LocalPortForward, RemotePortForward};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardFormKind {
    Local,
    Remote,
    Dynamic,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PortForwardKind {
    Local(LocalPortForward),
    Remote(RemotePortForward),
    Dynamic(DynamicPortForward),
}

impl PortForwardKind {
    pub fn display_label(&self) -> String {
        match self {
            Self::Local(f) => format!(
                "L {}:{} → {}:{}",
                empty_bind(&f.bind_address),
                f.local_port,
                f.remote_host,
                f.remote_port
            ),
            Self::Remote(f) => format!(
                "R :{} → {}:{}",
                f.remote_port, f.target_host, f.target_port
            ),
            Self::Dynamic(f) => format!(
                "D {}:{} (SOCKS5)",
                empty_bind(&f.bind_address),
                f.local_port
            ),
        }
    }

    pub fn audit_action_start(&self) -> &'static str {
        match self {
            Self::Local(_) => "port_forward.local.start",
            Self::Remote(_) => "port_forward.remote.start",
            Self::Dynamic(_) => "port_forward.dynamic.start",
        }
    }

    pub fn audit_action_stop(&self) -> &'static str {
        match self {
            Self::Local(_) => "port_forward.local.stop",
            Self::Remote(_) => "port_forward.remote.stop",
            Self::Dynamic(_) => "port_forward.dynamic.stop",
        }
    }

    pub fn audit_detail(&self) -> Value {
        match self {
            Self::Local(f) => json!({
                "type": "local",
                "bind": empty_bind(&f.bind_address),
                "local_port": f.local_port,
                "remote_host": f.remote_host,
                "remote_port": f.remote_port,
            }),
            Self::Remote(f) => json!({
                "type": "remote",
                "remote_port": f.remote_port,
                "target_host": f.target_host,
                "target_port": f.target_port,
            }),
            Self::Dynamic(f) => json!({
                "type": "dynamic",
                "bind": empty_bind(&f.bind_address),
                "local_port": f.local_port,
            }),
        }
    }
}

fn empty_bind(bind: &str) -> &str {
    if bind.is_empty() { "127.0.0.1" } else { bind }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardFormInput {
    pub bind_address: String,
    pub local_port: String,
    pub remote_host: String,
    pub remote_port: String,
}

impl Default for ForwardFormInput {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".into(),
            local_port: String::new(),
            remote_host: "127.0.0.1".into(),
            remote_port: String::new(),
        }
    }
}

pub fn parse_forward_form(
    kind: ForwardFormKind,
    input: &ForwardFormInput,
) -> Result<PortForwardKind, String> {
    match kind {
        ForwardFormKind::Local => {
            let local_port = parse_port(&input.local_port, "local port")?;
            let remote_port = parse_port(&input.remote_port, "remote port")?;
            let remote_host = require_host(&input.remote_host)?;
            Ok(PortForwardKind::Local(LocalPortForward {
                local_port,
                remote_host,
                remote_port,
                bind_address: input.bind_address.trim().to_string(),
            }))
        }
        ForwardFormKind::Remote => {
            let remote_port = parse_port(&input.local_port, "remote port")?;
            let target_port = parse_port(&input.remote_port, "target port")?;
            let target_host = require_host(&input.remote_host)?;
            Ok(PortForwardKind::Remote(RemotePortForward {
                remote_port,
                target_host,
                target_port,
                remote_bind_address: None,
            }))
        }
        ForwardFormKind::Dynamic => {
            let local_port = parse_port(&input.local_port, "port")?;
            Ok(PortForwardKind::Dynamic(DynamicPortForward {
                local_port,
                bind_address: input.bind_address.trim().to_string(),
            }))
        }
    }
}

fn parse_port(raw: &str, field: &str) -> Result<u16, String> {
    raw.trim()
        .parse()
        .map_err(|_| format!("invalid {field}"))
}

fn require_host(raw: &str) -> Result<String, String> {
    let host = raw.trim();
    if host.is_empty() {
        Err("target host required".into())
    } else {
        Ok(host.to_string())
    }
}

/// 底栏摘要，例如 `转发 ×3`。
pub fn status_bar_summary(count: usize, locale_en: bool) -> Option<String> {
    if count == 0 {
        return None;
    }
    Some(if locale_en {
        format!("Forwards ×{count}")
    } else {
        format!("转发 ×{count}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_label_format() {
        let k = PortForwardKind::Local(LocalPortForward {
            local_port: 8080,
            remote_host: "db.internal".into(),
            remote_port: 5432,
            bind_address: String::new(),
        });
        assert!(k.display_label().contains("8080"));
        assert!(k.display_label().contains("db.internal"));
    }

    #[test]
    fn parse_local_form_ok() {
        let input = ForwardFormInput {
            local_port: "9000".into(),
            remote_host: "10.0.0.5".into(),
            remote_port: "22".into(),
            ..Default::default()
        };
        let k = parse_forward_form(ForwardFormKind::Local, &input).unwrap();
        assert!(matches!(k, PortForwardKind::Local(_)));
    }

    #[test]
    fn parse_form_rejects_empty_host() {
        let input = ForwardFormInput {
            local_port: "8080".into(),
            remote_host: "  ".into(),
            remote_port: "80".into(),
            ..Default::default()
        };
        assert!(parse_forward_form(ForwardFormKind::Local, &input).is_err());
    }

    #[test]
    fn status_bar_summary_zero_is_none() {
        assert!(status_bar_summary(0, true).is_none());
        assert_eq!(status_bar_summary(2, false).as_deref(), Some("转发 ×2"));
    }
}
