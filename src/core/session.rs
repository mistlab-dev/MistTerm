//! 会话管理 - 保存和加载 SSH 会话配置
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use crate::core::credential::SecretBackend;
use crate::security::device_key;
use crate::security::encrypted_file;

/// 会话配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub id: String,
    pub name: String,
    pub group: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    /// SSH 私钥文件路径（空表示用密码或系统默认密钥）
    pub private_key_path: String,
    /// 尝试 ssh-agent / Pageant 认证（在指定密钥之后、密码之前）。
    #[serde(default = "default_use_ssh_agent")]
    pub use_ssh_agent: bool,
    pub last_connected_at: Option<i64>,
    /// 创建时间（排序用）
    #[serde(default)]
    pub created_at: Option<i64>,
    /// 已从 ~/.ssh/config 导入的标记 `Host|HostName|Port`
    #[serde(default)]
    pub ssh_config_marker: Option<String>,
    /// OpenSSH ProxyJump（完整跳板链见 P1）
    #[serde(default)]
    pub proxy_jump: String,
    /// OpenSSH ProxyCommand
    #[serde(default)]
    pub proxy_command: String,
    /// 环境色标：空 / red / yellow / green / blue / purple / gray
    #[serde(default)]
    pub color_tag: String,
    #[serde(default = "default_keepalive_enabled")]
    pub keepalive_enabled: bool,
    #[serde(default = "default_keepalive_interval")]
    pub keepalive_interval_secs: u32,
    #[serde(default = "default_keepalive_count_max")]
    pub keepalive_count_max: u8,
    #[serde(default = "default_keepalive_auto_reconnect")]
    pub keepalive_auto_reconnect: bool,
    /// 密码/密钥来源（Vault 引用时不落盘明文）
    #[serde(default)]
    pub secret_backend: SecretBackend,
    /// 本地端口转发（每行 `local_port:remote_host:remote_port`，例 `8080:127.0.0.1:80`）
    #[serde(default)]
    pub local_forwards_text: String,
    /// 远程端口转发（每行 `remote_port:target_host:target_port`，例 `8080:127.0.0.1:3000`）
    #[serde(default)]
    pub remote_forwards_text: String,
    /// 动态 SOCKS 转发（每行 `port` 或 `bind:port`，例 `1080`）
    #[serde(default)]
    pub dynamic_forwards_text: String,
}

fn default_keepalive_enabled() -> bool {
    true
}
fn default_keepalive_interval() -> u32 {
    30
}
fn default_keepalive_count_max() -> u8 {
    3
}
fn default_keepalive_auto_reconnect() -> bool {
    true
}

fn default_use_ssh_agent() -> bool {
    true
}

/// 侧栏颜色标签 → egui 色（由 UI 层调用）
pub fn session_color_tag_rgb(tag: &str) -> Option<(u8, u8, u8)> {
    match tag {
        "red" => Some((239, 68, 68)),
        "yellow" => Some((234, 179, 8)),
        "green" => Some((34, 197, 94)),
        "blue" => Some((59, 130, 246)),
        "purple" => Some((168, 85, 247)),
        "gray" => Some((158, 158, 158)),
        _ => None,
    }
}

pub const SESSION_COLOR_TAGS: &[(&str, &str)] = &[
    ("", "无"),
    ("red", "红"),
    ("yellow", "黄"),
    ("green", "绿"),
    ("blue", "蓝"),
    ("purple", "紫"),
    ("gray", "灰"),
];

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "New Session".to_string(),
            group: "默认".to_string(),
            host: "localhost".to_string(),
            port: 22,
            username: String::new(),
            password: String::new(),
            private_key_path: String::new(),
            use_ssh_agent: default_use_ssh_agent(),
            last_connected_at: None,
            created_at: None,
            ssh_config_marker: None,
            proxy_jump: String::new(),
            proxy_command: String::new(),
            color_tag: String::new(),
            keepalive_enabled: default_keepalive_enabled(),
            keepalive_interval_secs: default_keepalive_interval(),
            keepalive_count_max: default_keepalive_count_max(),
            keepalive_auto_reconnect: default_keepalive_auto_reconnect(),
            secret_backend: SecretBackend::default(),
            local_forwards_text: String::new(),
            remote_forwards_text: String::new(),
            dynamic_forwards_text: String::new(),
        }
    }
}

/// 解析 `local_forwards_text`（每行 `local_port:remote_host:remote_port`）。
pub fn parse_local_forwards_text(text: &str) -> Vec<crate::ssh::LocalPortForward> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let parts: Vec<&str> = line.split(':').collect();
            match parts.len() {
                3 => {
                    let local_port: u16 = parts[0].trim().parse().ok()?;
                    let remote_host = parts[1].trim().to_string();
                    let remote_port: u16 = parts[2].trim().parse().ok()?;
                    if remote_host.is_empty() {
                        return None;
                    }
                    Some(crate::ssh::LocalPortForward {
                        local_port,
                        remote_host,
                        remote_port,
                        bind_address: "127.0.0.1".into(),
                    })
                }
                4 => {
                    let bind_address = parts[0].trim().to_string();
                    let local_port: u16 = parts[1].trim().parse().ok()?;
                    let remote_host = parts[2].trim().to_string();
                    let remote_port: u16 = parts[3].trim().parse().ok()?;
                    if bind_address.is_empty() || remote_host.is_empty() {
                        return None;
                    }
                    Some(crate::ssh::LocalPortForward {
                        local_port,
                        remote_host,
                        remote_port,
                        bind_address,
                    })
                }
                _ => None,
            }
        })
        .collect()
}

/// 解析 `remote_forwards_text`（每行 `remote_port:target_host:target_port`）。
pub fn parse_remote_forwards_text(text: &str) -> Vec<crate::ssh::RemotePortForward> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() != 3 {
                return None;
            }
            let remote_port: u16 = parts[0].trim().parse().ok()?;
            let target_host = parts[1].trim().to_string();
            let target_port: u16 = parts[2].trim().parse().ok()?;
            if target_host.is_empty() {
                return None;
            }
            Some(crate::ssh::RemotePortForward {
                remote_port,
                target_host,
                target_port,
                remote_bind_address: None,
            })
        })
        .collect()
}

pub fn format_local_forward_line(fwd: &crate::ssh::LocalPortForward) -> String {
    format!(
        "{}:{}:{}",
        fwd.local_port, fwd.remote_host, fwd.remote_port
    )
}

pub fn format_remote_forward_line(fwd: &crate::ssh::RemotePortForward) -> String {
    format!(
        "{}:{}:{}",
        fwd.remote_port, fwd.target_host, fwd.target_port
    )
}

pub fn format_dynamic_forward_line(fwd: &crate::ssh::DynamicPortForward) -> String {
    if fwd.bind_address.is_empty() || fwd.bind_address == "127.0.0.1" {
        fwd.local_port.to_string()
    } else {
        format!("{}:{}", fwd.bind_address, fwd.local_port)
    }
}

pub fn append_local_forward_line(text: &mut String, fwd: &crate::ssh::LocalPortForward) {
    let line = format_local_forward_line(fwd);
    if text.lines().any(|l| l.trim() == line) {
        return;
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&line);
    text.push('\n');
}

pub fn append_remote_forward_line(text: &mut String, fwd: &crate::ssh::RemotePortForward) {
    let line = format_remote_forward_line(fwd);
    if text.lines().any(|l| l.trim() == line) {
        return;
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&line);
    text.push('\n');
}

pub fn append_dynamic_forward_line(text: &mut String, fwd: &crate::ssh::DynamicPortForward) {
    let line = format_dynamic_forward_line(fwd);
    if text.lines().any(|l| l.trim() == line) {
        return;
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&line);
    text.push('\n');
}

/// 解析 `dynamic_forwards_text`（每行 `port` 或 `bind_address:port`）。
pub fn parse_dynamic_forwards_text(text: &str) -> Vec<crate::ssh::DynamicPortForward> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let parts: Vec<&str> = line.split(':').collect();
            match parts.len() {
                1 => {
                    let local_port: u16 = parts[0].trim().parse().ok()?;
                    Some(crate::ssh::DynamicPortForward {
                        local_port,
                        bind_address: "127.0.0.1".into(),
                    })
                }
                2 => {
                    let bind_address = parts[0].trim().to_string();
                    let local_port: u16 = parts[1].trim().parse().ok()?;
                    if bind_address.is_empty() {
                        return None;
                    }
                    Some(crate::ssh::DynamicPortForward {
                        local_port,
                        bind_address,
                    })
                }
                _ => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod forward_parse_tests {
    use super::*;

    #[test]
    fn parse_local_forwards_with_bind_prefix() {
        let text = "127.0.0.1:8080:127.0.0.1:80\n8080:db.internal:5432";
        let fwd = parse_local_forwards_text(text);
        assert_eq!(fwd.len(), 2);
        assert_eq!(fwd[0].bind_address, "127.0.0.1");
        assert_eq!(fwd[0].local_port, 8080);
        assert_eq!(fwd[1].local_port, 8080);
        assert_eq!(fwd[1].remote_host, "db.internal");
    }

    #[test]
    fn parse_remote_forwards_skips_bad_lines() {
        let text = "8080:127.0.0.1:3000\n# comment\nbad\n9000:localhost:22";
        let fwd = parse_remote_forwards_text(text);
        assert_eq!(fwd.len(), 2);
        assert_eq!(fwd[0].remote_port, 8080);
        assert_eq!(fwd[1].target_port, 22);
    }

    #[test]
    fn parse_dynamic_forwards_port_and_bind() {
        let text = "1080\n0.0.0.0:1081\n# x";
        let fwd = parse_dynamic_forwards_text(text);
        assert_eq!(fwd.len(), 2);
        assert_eq!(fwd[0].local_port, 1080);
        assert_eq!(fwd[1].bind_address, "0.0.0.0");
        assert_eq!(fwd[1].local_port, 1081);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSessionConfig {
    #[serde(default)]
    id: String,
    name: String,
    #[serde(default = "default_group")]
    group: String,
    host: String,
    port: u16,
    username: String,
    #[serde(default)]
    password: String, // 兼容旧格式明文
    #[serde(default)]
    encrypted_password: String,
    #[serde(default)]
    password_nonce: String,
    #[serde(default)]
    private_key_path: String,
    #[serde(default = "default_use_ssh_agent")]
    use_ssh_agent: bool,
    #[serde(default)]
    last_connected_at: Option<i64>,
    #[serde(default)]
    created_at: Option<i64>,
    #[serde(default)]
    ssh_config_marker: Option<String>,
    #[serde(default)]
    proxy_jump: String,
    #[serde(default)]
    proxy_command: String,
    #[serde(default)]
    color_tag: String,
    #[serde(default = "default_keepalive_enabled")]
    keepalive_enabled: bool,
    #[serde(default = "default_keepalive_interval")]
    keepalive_interval_secs: u32,
    #[serde(default = "default_keepalive_count_max")]
    keepalive_count_max: u8,
    #[serde(default = "default_keepalive_auto_reconnect")]
    keepalive_auto_reconnect: bool,
    #[serde(default)]
    secret_backend: SecretBackend,
    #[serde(default)]
    local_forwards_text: String,
    #[serde(default)]
    remote_forwards_text: String,
    #[serde(default)]
    dynamic_forwards_text: String,
}

fn default_group() -> String {
    "默认".to_string()
}

/// 会话管理器
pub struct SessionManager {
    sessions: Vec<SessionConfig>,
    file_path: PathBuf,
    device_key: [u8; 32],
    /// 最近一次 `load` / `import` 产生的提示（启动时由 UI 取走展示）
    load_diagnostics: Vec<String>,
}

impl SessionManager {
    pub fn parse_stored_sessions_json(
        device_key_bytes: &[u8; 32],
        content: &str,
    ) -> Option<(Vec<SessionConfig>, bool, Vec<String>)> {
        let stored: Vec<StoredSessionConfig> = serde_json::from_str(content).ok()?;
        let mut sessions = Vec::with_capacity(stored.len());
        let mut had_plaintext = false;
        let mut warnings = Vec::new();
        for cfg in stored {
            let password = if !cfg.encrypted_password.is_empty() && !cfg.password_nonce.is_empty() {
                match device_key::decrypt_secret(
                    device_key_bytes,
                    &cfg.encrypted_password,
                    &cfg.password_nonce,
                ) {
                    Some(p) => p,
                    None => {
                        warnings.push(format!(
                            "会话「{}」({}) 密码数据损坏，请重新编辑会话并保存密码",
                            cfg.name, cfg.host
                        ));
                        String::new()
                    }
                }
            } else if !cfg.password.is_empty() {
                had_plaintext = true;
                cfg.password
            } else {
                String::new()
            };
            sessions.push(SessionConfig {
                id: if cfg.id.is_empty() {
                    uuid::Uuid::new_v4().to_string()
                } else {
                    cfg.id
                },
                name: cfg.name,
                group: cfg.group,
                host: cfg.host,
                port: cfg.port,
                username: cfg.username,
                password,
                private_key_path: cfg.private_key_path,
                use_ssh_agent: cfg.use_ssh_agent,
                last_connected_at: cfg.last_connected_at,
                created_at: cfg.created_at,
                ssh_config_marker: cfg.ssh_config_marker,
                proxy_jump: cfg.proxy_jump,
                proxy_command: cfg.proxy_command,
                color_tag: cfg.color_tag,
                keepalive_enabled: cfg.keepalive_enabled,
                keepalive_interval_secs: cfg.keepalive_interval_secs,
                keepalive_count_max: cfg.keepalive_count_max,
                keepalive_auto_reconnect: cfg.keepalive_auto_reconnect,
                secret_backend: cfg.secret_backend,
                local_forwards_text: cfg.local_forwards_text,
                remote_forwards_text: cfg.remote_forwards_text,
                dynamic_forwards_text: cfg.dynamic_forwards_text,
            });
        }
        Some((sessions, had_plaintext, warnings))
    }

    /// 从会话备份 JSON 替换当前会话（路径可为同步包内的 `sessions.json`）
    pub fn import_sessions_from_file_path(&mut self, path: &std::path::Path) -> io::Result<()> {
        let content = fs::read_to_string(path)?;
        let Some((sessions, had_plaintext, warnings)) =
            Self::parse_stored_sessions_json(&self.device_key, &content)
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "无法解析会话备份文件（JSON 格式或字段无效）",
            ));
        };
        self.load_diagnostics.extend(warnings);
        self.sessions = sessions;
        self.save();
        if had_plaintext {
            log::warn!(
                "Imported package contained plaintext session passwords; loaded and re-encrypted locally"
            );
        }
        Ok(())
    }

    /// 从指定路径创建会话管理器
    pub fn with_sessions_path<P: Into<PathBuf>>(path: P) -> Self {
        let file_path = path.into();
        let device_key = device_key::device_key();
        
        let mut manager = Self {
            sessions: Vec::new(),
            file_path,
            device_key,
            load_diagnostics: Vec::new(),
        };
        manager.load();
        manager
    }

    /// 创建新的会话管理器
    pub fn new() -> Self {
        let mut file_path = std::env::current_dir().unwrap_or_default();
        file_path.push("sessions.json");
        Self::with_sessions_path(file_path)
    }

    /// 取走并清空最近一次加载产生的诊断信息（供状态栏一次性展示）。
    pub fn take_load_diagnostics(&mut self) -> Vec<String> {
        std::mem::take(&mut self.load_diagnostics)
    }

    fn read_sessions_file_text(&mut self) -> Option<String> {
        for attempt in 0..3 {
            match fs::read_to_string(&self.file_path) {
                Ok(c) => return Some(c),
                Err(e) if attempt < 2 => {
                    log::warn!(
                        "Failed to read sessions (attempt {}): {}; retrying in 100ms",
                        attempt + 1,
                        e
                    );
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    self.load_diagnostics.push(format!(
                        "读取会话文件失败（已重试 3 次）：{}",
                        e
                    ));
                    return None;
                }
            }
        }
        None
    }

    /// 解密 `mistterm-aes-v1` 信封，或返回原始明文 JSON（旧格式）。
    fn unwrap_sessions_json_text(text: &str) -> Option<String> {
        if let Ok(env) = serde_json::from_str::<crate::security::encrypted_file::ConfigEnvelope>(text)
        {
            if env.format == crate::security::encrypted_file::ENVELOPE_FORMAT {
                return device_key::decrypt_secret(
                    &device_key::device_key(),
                    &env.ciphertext_b64,
                    &env.nonce_b64,
                );
            }
        }
        Some(text.to_string())
    }

    /// 加载已保存的会话
    fn load(&mut self) {
        if !self.file_path.exists() {
            return;
        }

        let Some(content) = self.read_sessions_file_text() else {
            return;
        };

        let Some(inner) = Self::unwrap_sessions_json_text(&content) else {
            self.load_diagnostics
                .push("会话文件解密失败（设备密钥可能已变更）".to_string());
            return;
        };

        let Some((sessions, had_plaintext, mut warnings)) =
            Self::parse_stored_sessions_json(&self.device_key, &inner)
        else {
            self.load_diagnostics.push(
                "无法解析会话文件（JSON 损坏或格式错误）".to_string(),
            );
            return;
        };
        self.load_diagnostics.append(&mut warnings);
        self.sessions = sessions;
        log::info!("Loaded {} saved sessions", self.sessions.len());
        let needs_migrate = had_plaintext || !content.contains(encrypted_file::ENVELOPE_FORMAT);
        if needs_migrate {
            log::info!("Migrating sessions.json to device_key file encryption");
            self.save();
        }
    }

    /// 保存会话（整文件 device_key 加密）
    pub fn save(&self) {
        let mut stored = Vec::with_capacity(self.sessions.len());
        for cfg in &self.sessions {
            stored.push(StoredSessionConfig {
                id: cfg.id.clone(),
                name: cfg.name.clone(),
                group: cfg.group.clone(),
                host: cfg.host.clone(),
                port: cfg.port,
                username: cfg.username.clone(),
                password: if cfg.secret_backend.is_vault() {
                    String::new()
                } else {
                    cfg.password.clone()
                },
                encrypted_password: String::new(),
                password_nonce: String::new(),
                private_key_path: cfg.private_key_path.clone(),
                use_ssh_agent: cfg.use_ssh_agent,
                last_connected_at: cfg.last_connected_at,
                created_at: cfg.created_at,
                ssh_config_marker: cfg.ssh_config_marker.clone(),
                proxy_jump: cfg.proxy_jump.clone(),
                proxy_command: cfg.proxy_command.clone(),
                color_tag: cfg.color_tag.clone(),
                keepalive_enabled: cfg.keepalive_enabled,
                keepalive_interval_secs: cfg.keepalive_interval_secs,
                keepalive_count_max: cfg.keepalive_count_max,
                keepalive_auto_reconnect: cfg.keepalive_auto_reconnect,
                secret_backend: cfg.secret_backend.clone(),
                local_forwards_text: cfg.local_forwards_text.clone(),
                remote_forwards_text: cfg.remote_forwards_text.clone(),
                dynamic_forwards_text: cfg.dynamic_forwards_text.clone(),
            });
        }

        if let Err(e) = encrypted_file::save_encrypted_json(&self.file_path, &stored) {
            log::error!("Failed to save sessions: {}", e);
        } else {
            log::info!("Saved {} sessions", self.sessions.len());
        }
    }

    /// 添加会话
    pub fn add_session(&mut self, config: SessionConfig) {
        self.sessions.push(config);
        self.save();
    }

    /// 删除会话
    pub fn remove_session(&mut self, idx: usize) {
        if idx < self.sessions.len() {
            self.sessions.remove(idx);
            self.save();
        }
    }

    /// 获取所有会话
    pub fn get_sessions(&self) -> &[SessionConfig] {
        &self.sessions
    }

    /// 获取会话列表（UI 层使用）
    pub fn list_sessions(&self) -> &[SessionConfig] {
        &self.sessions
    }

    /// 根据 ID 获取会话
    pub fn get_session(&self, id: &str) -> Option<&SessionConfig> {
        self.sessions.iter().find(|s| s.id == id)
    }

    /// 按 ProxyJump token 匹配已保存会话（名称或主机）。
    pub fn find_session_for_jump_token(&self, token: &str) -> Option<&SessionConfig> {
        let token = token.trim();
        self.sessions
            .iter()
            .find(|s| s.name == token || s.host == token)
    }

    /// 创建新会话
    pub fn create_session(
        &mut self,
        name: &str,
        host: &str,
        port: u16,
        username: &str,
        password: &str,
        group: &str,
        private_key_path: &str,
    ) -> SessionConfig {
        let mut config = SessionConfig::default();
        config.id = uuid::Uuid::new_v4().to_string();
        config.name = name.to_string();
        config.host = host.to_string();
        config.port = port;
        config.username = username.to_string();
        config.password = password.to_string();
        config.group = if group.trim().is_empty() { "默认".to_string() } else { group.trim().to_string() };
        config.private_key_path = private_key_path.to_string();
        config.created_at = Some(chrono::Utc::now().timestamp());
        self.sessions.push(config.clone());
        self.save();
        config
    }

    /// 删除会话
    pub fn delete_session(&mut self, id: &str) {
        if let Some(pos) = self.sessions.iter().position(|s| s.id == id) {
            self.sessions.remove(pos);
            self.save();
        }
    }

    /// 更新会话
    pub fn update_session(
        &mut self,
        id: &str,
        name: &str,
        host: &str,
        port: u16,
        username: &str,
        password: &str,
        group: &str,
        private_key_path: &str,
    ) -> bool {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
            session.name = name.to_string();
            session.host = host.to_string();
            session.port = port;
            session.username = username.to_string();
            session.password = password.to_string();
            session.group = if group.trim().is_empty() { "默认".to_string() } else { group.trim().to_string() };
            session.private_key_path = private_key_path.to_string();
            self.save();
            return true;
        }
        false
    }

    pub fn mark_session_connected(&mut self, id: &str) {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
            session.last_connected_at = Some(chrono::Utc::now().timestamp());
            self.save();
        }
    }

    /// 就地更新会话扩展字段（色标、KeepAlive 等）
    pub fn patch_session(&mut self, id: &str, patch: impl FnOnce(&mut SessionConfig)) -> bool {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
            patch(session);
            self.save();
            return true;
        }
        false
    }

    /// 获取会话数量
    pub fn count(&self) -> usize {
        self.sessions.len()
    }

    /// 会话存储文件路径（用于备份/导出）
    pub fn storage_path(&self) -> &PathBuf {
        &self.file_path
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_config_default() {
        let config = SessionConfig::default();
        assert_eq!(config.name, "New Session");
        assert_eq!(config.port, 22);
    }

    #[test]
    fn test_session_manager_creation() {
        // 使用临时目录，避免加载当前目录下的 sessions.json
        let temp_dir = tempfile::tempdir().unwrap();
        let mut path = temp_dir.path().to_path_buf();
        path.push("sessions.json");
        let manager = SessionManager::with_sessions_path(path);
        // 应该能正常创建，即使没有 sessions.json 文件
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = crate::security::device_key::derive_key_from_fingerprint("test-device");
        let src = "secret-123";
        let (enc, nonce) = crate::security::device_key::encrypt_secret(&key, src).unwrap();
        let plain = crate::security::device_key::decrypt_secret(&key, &enc, &nonce).unwrap();
        assert_eq!(plain, src);
    }
}
