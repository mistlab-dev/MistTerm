//! MistTerm CLI：复用 GUI 同一份会话存储 / SSH / SFTP / 批量执行能力。
//!
//! 逻辑全部在此模块，`src/bin/mist.rs` 只做 clap 解析和分发。

pub mod context;
pub mod exec;
pub mod frag;
pub mod fwd;
pub mod import_ssh;
pub mod ls;
pub mod session_log;
pub mod sftp_cmds;
pub mod ssh_cmd;

use anyhow::{Context, Result};
use crate::core::secret_resolver::SecretResolver;
use crate::core::session::{SessionConfig, SessionManager};
use crate::core::ssh_build;
use crate::ssh::SshConfig;

/// CLI 运行上下文：会话存储 + 应用设置（Vault 等）。
pub struct CliContext {
    pub sessions: SessionManager,
    pub settings: crate::core::app_settings::AppSettings,
}

impl CliContext {
    pub fn load() -> Self {
        Self {
            sessions: SessionManager::new(),
            settings: crate::core::app_settings::AppSettings::load(),
        }
    }

    pub fn resolver(&self) -> SecretResolver {
        SecretResolver::new(self.settings.vault.clone())
    }

    /// SessionConfig → SshConfig（与 GUI 共用 `core::ssh_build`）。
    pub fn ssh_config(&self, session: &SessionConfig) -> Result<SshConfig> {
        ssh_build::session_to_ssh_config(session, &self.sessions, &self.resolver())
            .map_err(|e| anyhow::anyhow!("解析凭据失败 ({}): {}", session.name, e))
    }

    /// 按 name / id / host 匹配已保存会话。
    pub fn find_session(&self, target: &str) -> Option<SessionConfig> {
        let t = target.trim();
        self.sessions
            .get_sessions()
            .iter()
            .find(|s| s.name == t || s.id == t || s.host == t)
            .cloned()
    }

    /// target 解析：先查已保存会话，匹配不到按 `user@host[:port]` 临时构造。
    pub fn resolve_target(&self, target: &str) -> Result<SessionConfig> {
        if let Some(s) = self.find_session(target) {
            return Ok(s);
        }
        parse_adhoc_target(target)
    }

    /// 连接成功后更新 last_connected_at（与 GUI 行为一致）。
    pub fn mark_connected(&mut self, session: &SessionConfig) {
        // 只对已保存会话回写；临时构造的会话 id 不在列表里，patch 自动落空。
        self.sessions.mark_session_connected(&session.id);
    }
}

/// `user@host[:port]` / `host[:port]` 临时连接。
fn parse_adhoc_target(target: &str) -> Result<SessionConfig> {
    let t = target.trim();
    if t.is_empty() {
        anyhow::bail!("空目标");
    }
    let (user_part, host_part) = match t.split_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, t),
    };
    let (host, port) = match host_part.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() && !p.is_empty() => {
            let port: u16 = p
                .parse()
                .with_context(|| format!("非法端口: {p}"))?;
            (h.to_string(), port)
        }
        _ => (host_part.to_string(), 22),
    };
    if host.is_empty() {
        anyhow::bail!("目标缺少主机名: {target}");
    }
    let username = user_part
        .map(str::to_string)
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| {
            std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "root".to_string())
        });
    let mut s = SessionConfig::default();
    s.id = String::new(); // 非已保存会话
    s.name = t.to_string();
    s.host = host;
    s.port = port;
    s.username = username;
    Ok(s)
}
