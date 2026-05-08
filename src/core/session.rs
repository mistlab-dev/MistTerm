//! 会话管理 - 保存和加载 SSH 会话配置
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::security::device_key;

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
    pub last_connected_at: Option<i64>,
}

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
            last_connected_at: None,
        }
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
    last_connected_at: Option<i64>,
}

fn default_group() -> String {
    "默认".to_string()
}

/// 会话管理器
pub struct SessionManager {
    sessions: Vec<SessionConfig>,
    file_path: PathBuf,
    device_key: [u8; 32],
}

impl SessionManager {
    pub fn parse_stored_sessions_json(
        device_key_bytes: &[u8; 32],
        content: &str,
    ) -> Option<(Vec<SessionConfig>, bool)> {
        let stored: Vec<StoredSessionConfig> = serde_json::from_str(content).ok()?;
        let mut sessions = Vec::with_capacity(stored.len());
        let mut had_plaintext = false;
        for cfg in stored {
            let password =
                if !cfg.encrypted_password.is_empty() && !cfg.password_nonce.is_empty() {
                    device_key::decrypt_secret(
                        device_key_bytes,
                        &cfg.encrypted_password,
                        &cfg.password_nonce,
                    )
                    .unwrap_or_default()
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
                last_connected_at: cfg.last_connected_at,
            });
        }
        Some((sessions, had_plaintext))
    }

    /// 从会话备份 JSON 替换当前会话（路径可为同步包内的 `sessions.json`）
    pub fn import_sessions_from_file_path(&mut self, path: &std::path::Path) -> io::Result<()> {
        let content = fs::read_to_string(path)?;
        let Some((sessions, had_plaintext)) =
            Self::parse_stored_sessions_json(&self.device_key, &content)
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "无法解析会话备份文件（JSON 格式或字段无效）",
            ));
        };
        self.sessions = sessions;
        self.save();
        if had_plaintext {
            log::warn!("导入的包中含明文会话密码，已载入并重新加密写入本地文件");
        }
        Ok(())
    }

    /// 创建新的会话管理器
    pub fn new() -> Self {
        let mut file_path = std::env::current_dir().unwrap_or_default();
        file_path.push("sessions.json");
        let device_key = device_key::device_key();
        
        let mut manager = Self {
            sessions: Vec::new(),
            file_path,
            device_key,
        };
        manager.load();
        manager
    }

    /// 加载已保存的会话
    fn load(&mut self) {
        if !self.file_path.exists() {
            return;
        }

        if let Ok(content) = fs::read_to_string(&self.file_path) {
            let Some((sessions, had_plaintext)) =
                Self::parse_stored_sessions_json(&self.device_key, &content)
            else {
                return;
            };
            self.sessions = sessions;
            log::info!("Loaded {} saved sessions", self.sessions.len());
            if had_plaintext {
                log::warn!("Detected plaintext passwords in sessions.json; migrated to encrypted storage.");
                self.save();
            }
        }
    }

    /// 保存会话
    pub fn save(&self) {
        let mut stored = Vec::with_capacity(self.sessions.len());
        for cfg in &self.sessions {
            let (encrypted_password, password_nonce) =
                device_key::encrypt_secret(&self.device_key, &cfg.password)
                    .unwrap_or((String::new(), String::new()));
            stored.push(StoredSessionConfig {
                id: cfg.id.clone(),
                name: cfg.name.clone(),
                group: cfg.group.clone(),
                host: cfg.host.clone(),
                port: cfg.port,
                username: cfg.username.clone(),
                password: String::new(),
                encrypted_password,
                password_nonce,
                last_connected_at: cfg.last_connected_at,
            });
        }

        if let Ok(content) = serde_json::to_string_pretty(&stored) {
            let _ = fs::write(&self.file_path, content);
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

    /// 创建新会话
    pub fn create_session(
        &mut self,
        name: &str,
        host: &str,
        port: u16,
        username: &str,
        password: &str,
        group: &str,
    ) -> SessionConfig {
        let mut config = SessionConfig::default();
        config.id = uuid::Uuid::new_v4().to_string();
        config.name = name.to_string();
        config.host = host.to_string();
        config.port = port;
        config.username = username.to_string();
        config.password = password.to_string();
        config.group = if group.trim().is_empty() { "默认".to_string() } else { group.trim().to_string() };
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
    ) -> bool {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
            session.name = name.to_string();
            session.host = host.to_string();
            session.port = port;
            session.username = username.to_string();
            session.password = password.to_string();
            session.group = if group.trim().is_empty() { "默认".to_string() } else { group.trim().to_string() };
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
        let manager = SessionManager::new();
        // 应该能正常创建，即使没有 sessions.json 文件
        assert!(manager.count() >= 0);
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
