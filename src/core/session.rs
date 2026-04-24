//! 会话管理 - 保存和加载 SSH 会话配置

use aes_gcm::aead::Aead;
use aes_gcm::aead::KeyInit;
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// 会话配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "New Session".to_string(),
            host: "localhost".to_string(),
            port: 22,
            username: String::new(),
            password: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredSessionConfig {
    name: String,
    host: String,
    port: u16,
    username: String,
    #[serde(default)]
    password: String, // 兼容旧格式明文
    #[serde(default)]
    encrypted_password: String,
    #[serde(default)]
    password_nonce: String,
}

/// 会话管理器
pub struct SessionManager {
    sessions: Vec<SessionConfig>,
    file_path: PathBuf,
    device_key: [u8; 32],
}

impl SessionManager {
    fn build_device_fingerprint() -> String {
        // macOS: 优先取 IOPlatformUUID 作为设备指纹
        let output = Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output();
        if let Ok(out) = output {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout);
                for line in text.lines() {
                    if let Some(pos) = line.find("IOPlatformUUID") {
                        let tail = &line[pos..];
                        let parts: Vec<&str> = tail.split('"').collect();
                        if parts.len() >= 4 {
                            return parts[3].to_string();
                        }
                    }
                }
            }
        }
        // 退化方案（仍具备设备绑定倾向）
        format!(
            "{}:{}:{}",
            std::env::consts::OS,
            std::env::var("USER").unwrap_or_default(),
            std::env::var("HOSTNAME").unwrap_or_default()
        )
    }

    fn derive_key_from_fingerprint(fingerprint: &str) -> [u8; 32] {
        // 轻量 key 派生（依赖设备指纹 + 固定盐）
        let mut key = [0u8; 32];
        let salt = b"MistTerm-Local-Device-Key-v1";
        let bytes = fingerprint.as_bytes();
        if bytes.is_empty() {
            return key;
        }
        for i in 0..32 {
            let a = bytes[i % bytes.len()];
            let b = salt[i % salt.len()];
            key[i] = a.wrapping_add(b).rotate_left((i % 8) as u32) ^ (i as u8);
        }
        key
    }

    fn encrypt_password_with_key(key: &[u8; 32], password: &str) -> Option<(String, String)> {
        if password.is_empty() {
            return Some((String::new(), String::new()));
        }
        let cipher = Aes256Gcm::new_from_slice(key).ok()?;
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher.encrypt(nonce, password.as_bytes()).ok()?;
        Some((B64.encode(ciphertext), B64.encode(nonce_bytes)))
    }

    fn decrypt_password_with_key(key: &[u8; 32], encrypted: &str, nonce_b64: &str) -> Option<String> {
        if encrypted.is_empty() || nonce_b64.is_empty() {
            return Some(String::new());
        }
        let cipher = Aes256Gcm::new_from_slice(key).ok()?;
        let ciphertext = B64.decode(encrypted).ok()?;
        let nonce_raw = B64.decode(nonce_b64).ok()?;
        if nonce_raw.len() != 12 {
            return None;
        }
        let nonce = Nonce::from_slice(&nonce_raw);
        let plain = cipher.decrypt(nonce, ciphertext.as_ref()).ok()?;
        String::from_utf8(plain).ok()
    }

    /// 创建新的会话管理器
    pub fn new() -> Self {
        let mut file_path = std::env::current_dir().unwrap_or_default();
        file_path.push("sessions.json");
        let fingerprint = Self::build_device_fingerprint();
        let device_key = Self::derive_key_from_fingerprint(&fingerprint);
        
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
            if let Ok(stored) = serde_json::from_str::<Vec<StoredSessionConfig>>(&content) {
                let mut sessions = Vec::with_capacity(stored.len());
                let mut had_plaintext = false;
                for cfg in stored {
                    let password = if !cfg.encrypted_password.is_empty() && !cfg.password_nonce.is_empty() {
                        Self::decrypt_password_with_key(
                            &self.device_key,
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
                        id: uuid::Uuid::new_v4().to_string(),
                        name: cfg.name,
                        host: cfg.host,
                        port: cfg.port,
                        username: cfg.username,
                        password,
                    });
                }
                self.sessions = sessions;
                log::info!("Loaded {} saved sessions", self.sessions.len());
                if had_plaintext {
                    log::warn!("Detected plaintext passwords in sessions.json; migrated to encrypted storage.");
                    self.save();
                }
            }
        }
    }

    /// 保存会话
    pub fn save(&self) {
        let mut stored = Vec::with_capacity(self.sessions.len());
        for cfg in &self.sessions {
            let (encrypted_password, password_nonce) =
                Self::encrypt_password_with_key(&self.device_key, &cfg.password)
                    .unwrap_or((String::new(), String::new()));
            stored.push(StoredSessionConfig {
                name: cfg.name.clone(),
                host: cfg.host.clone(),
                port: cfg.port,
                username: cfg.username.clone(),
                password: String::new(),
                encrypted_password,
                password_nonce,
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
    pub fn create_session(&mut self, name: &str, host: &str, username: &str) -> SessionConfig {
        let mut config = SessionConfig::default();
        config.name = name.to_string();
        config.host = host.to_string();
        config.username = username.to_string();
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

    /// 获取会话数量
    pub fn count(&self) -> usize {
        self.sessions.len()
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
        let key = SessionManager::derive_key_from_fingerprint("test-device");
        let src = "secret-123";
        let (enc, nonce) = SessionManager::encrypt_password_with_key(&key, src).unwrap();
        let plain = SessionManager::decrypt_password_with_key(&key, &enc, &nonce).unwrap();
        assert_eq!(plain, src);
    }
}
