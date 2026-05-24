//! 凭证库：整文件 `device_key` AES-GCM（`mistterm-aes-v1`），条目内 `secret` 明文仅存于加密信封内。

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::security::device_key;
use crate::security::encrypted_file::{self, ENVELOPE_FORMAT};

/// 凭证分类（侧栏分组）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CredentialCategory {
    Server,
    Database,
    SshKey,
    Api,
    Other,
}

impl CredentialCategory {
    pub fn label_zh(&self) -> &'static str {
        match self {
            CredentialCategory::Server => "服务器账号",
            CredentialCategory::Database => "数据库",
            CredentialCategory::SshKey => "SSH 密钥",
            CredentialCategory::Api => "API / 令牌",
            CredentialCategory::Other => "其他",
        }
    }
}

/// 机密存储后端
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretBackend {
    #[serde(rename = "local")]
    LocalEncrypted,
    VaultKv {
        mount: String,
        path: String,
        field: String,
        #[serde(default)]
        version: Option<u32>,
    },
}

impl Default for SecretBackend {
    fn default() -> Self {
        SecretBackend::LocalEncrypted
    }
}

impl SecretBackend {
    pub fn is_vault(&self) -> bool {
        matches!(self, SecretBackend::VaultKv { .. })
    }
}

/// 认证方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CredentialAuthKind {
    #[default]
    Password,
    SshKey,
    Token,
}

impl CredentialAuthKind {
    pub fn label_zh(&self) -> &'static str {
        match self {
            CredentialAuthKind::Password => "密码",
            CredentialAuthKind::SshKey => "SSH 密钥",
            CredentialAuthKind::Token => "令牌 / API Key",
        }
    }
}

/// 单条凭证（解密后的工作副本）
#[derive(Debug, Clone)]
pub struct Credential {
    pub id: String,
    pub name: String,
    pub category: CredentialCategory,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: CredentialAuthKind,
    /// 密码、PEM 或 token 明文（仅内存）
    pub secret: String,
    pub notes: String,
    pub tags: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub secret_backend: SecretBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredCredential {
    id: String,
    name: String,
    category: CredentialCategory,
    #[serde(default)]
    host: String,
    #[serde(default)]
    port: u16,
    #[serde(default)]
    username: String,
    auth: CredentialAuthKind,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    tags: Vec<String>,
    /// 加密信封内的明文 secret（整文件已由 device_key 保护）
    #[serde(default)]
    secret: String,
    #[serde(default)]
    secret_enc: String,
    #[serde(default)]
    secret_nonce: String,
    #[serde(default)]
    secret_backend: SecretBackend,
    created_at: i64,
    updated_at: i64,
}

impl StoredCredential {
    fn from_credential(c: &Credential) -> Self {
        StoredCredential {
            id: c.id.clone(),
            name: c.name.clone(),
            category: c.category,
            host: c.host.clone(),
            port: c.port,
            username: c.username.clone(),
            auth: c.auth,
            notes: c.notes.clone(),
            tags: c.tags.clone(),
            secret: if c.secret_backend.is_vault() {
                String::new()
            } else {
                c.secret.clone()
            },
            secret_enc: String::new(),
            secret_nonce: String::new(),
            secret_backend: c.secret_backend.clone(),
            created_at: c.created_at,
            updated_at: c.updated_at,
        }
    }

    fn resolve_secret(&self, key: &[u8; 32]) -> String {
        if self.secret_backend.is_vault() {
            return String::new();
        }
        if !self.secret.is_empty() {
            return self.secret.clone();
        }
        if self.secret_enc.is_empty() || self.secret_nonce.is_empty() {
            return String::new();
        }
        device_key::decrypt_secret(key, &self.secret_enc, &self.secret_nonce).unwrap_or_default()
    }

    fn to_credential(&self, key: &[u8; 32]) -> Option<Credential> {
        Some(Credential {
            id: self.id.clone(),
            name: self.name.clone(),
            category: self.category,
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            auth: self.auth,
            secret: self.resolve_secret(key),
            notes: self.notes.clone(),
            tags: self.tags.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            secret_backend: self.secret_backend.clone(),
        })
    }

    fn normalize_for_encrypted_file(&mut self, key: &[u8; 32]) {
        if self.secret_backend.is_vault() {
            self.secret.clear();
            self.secret_enc.clear();
            self.secret_nonce.clear();
            return;
        }
        if self.secret.is_empty() {
            self.secret = self.resolve_secret(key);
        }
        self.secret_enc.clear();
        self.secret_nonce.clear();
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CredentialVaultFile {
    #[serde(default)]
    entries: Vec<StoredCredential>,
}

/// 旧版 v2：HKDF + 盐（迁移用）
#[derive(Debug, Serialize, Deserialize)]
struct CredentialVaultFileV2 {
    #[serde(default)]
    vault_format: i32,
    salt_b64: String,
    entries: Vec<StoredCredential>,
}

/// 凭证库
pub struct CredentialVault {
    entries: Vec<StoredCredential>,
    file_path: PathBuf,
    device_key: [u8; 32],
}

impl CredentialVault {
    pub fn default_path() -> PathBuf {
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("mistterm");
        let _ = fs::create_dir_all(&dir);
        dir.join("credentials.json")
    }

    pub fn new() -> Self {
        Self::new_at(Self::default_path())
    }

    pub fn new_at(file_path: PathBuf) -> Self {
        let device_key = device_key::device_key();
        let mut vault = Self {
            entries: Vec::new(),
            file_path,
            device_key,
        };
        if vault.file_path.exists() {
            if let Err(e) = vault.load_from_disk() {
                tracing::warn!("Failed to load credential vault (using empty vault): {}", e);
            }
        }
        vault
    }

    pub fn path(&self) -> &PathBuf {
        &self.file_path
    }

    fn load_from_disk(&mut self) -> io::Result<()> {
        let text = fs::read_to_string(&self.file_path)?;
        if let Ok(env) = serde_json::from_str::<encrypted_file::ConfigEnvelope>(&text) {
            if env.format == ENVELOPE_FORMAT {
                if let Some(plain) = device_key::decrypt_secret(
                    &self.device_key,
                    &env.ciphertext_b64,
                    &env.nonce_b64,
                ) {
                    if let Ok(file) = serde_json::from_str::<CredentialVaultFile>(&plain) {
                        self.entries = file.entries;
                        self.normalize_entries();
                        return Ok(());
                    }
                }
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "凭证库解密失败",
                ));
            }
        }

        let val: Value = serde_json::from_str(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        match val {
            Value::Array(_) => {
                let list: Vec<StoredCredential> = serde_json::from_value(val).unwrap_or_default();
                self.entries = list;
                self.normalize_entries();
                self.save()?;
                tracing::info!("Credential vault migrated from legacy array to device_key file encryption");
            }
            Value::Object(_) => {
                if let Ok(v2) = serde_json::from_value::<CredentialVaultFileV2>(val.clone()) {
                    if v2.vault_format == 2 && !v2.salt_b64.is_empty() {
                        let raw = B64.decode(v2.salt_b64.as_bytes())
                            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                        if raw.len() == 16 {
                            let mut salt = [0u8; 16];
                            salt.copy_from_slice(&raw);
                            let vk = device_key::derive_credential_vault_data_key(&self.device_key, &salt)
                                .ok_or_else(|| {
                                    io::Error::new(io::ErrorKind::InvalidData, "无法派生旧版凭证库密钥")
                                })?;
                            self.entries = v2.entries;
                            for e in &mut self.entries {
                                if e.secret.is_empty() && !e.secret_enc.is_empty() {
                                    e.secret = e.resolve_secret(&vk);
                                    e.secret_enc.clear();
                                    e.secret_nonce.clear();
                                }
                            }
                            self.save()?;
                            tracing::info!(
                                "Credential vault migrated from HKDF v2 to device_key file encryption"
                            );
                            return Ok(());
                        }
                    }
                }
                if let Ok(file) = serde_json::from_value::<CredentialVaultFile>(val) {
                    self.entries = file.entries;
                    self.normalize_entries();
                    self.save()?;
                    tracing::info!("Credential vault migrated to device_key file encryption");
                } else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "凭证库 JSON 格式无效",
                    ));
                }
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "凭证库 JSON 根类型无效",
                ));
            }
        }
        Ok(())
    }

    fn normalize_entries(&mut self) {
        for e in &mut self.entries {
            e.normalize_for_encrypted_file(&self.device_key);
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let file = CredentialVaultFile {
            entries: self.entries.clone(),
        };
        encrypted_file::save_encrypted_json(&self.file_path, &file)
    }

    pub fn list(&self) -> Vec<Credential> {
        self.entries
            .iter()
            .filter_map(|s| s.to_credential(&self.device_key))
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<Credential> {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .and_then(|s| s.to_credential(&self.device_key))
    }

    pub fn upsert(&mut self, mut c: Credential) -> io::Result<()> {
        let now = chrono::Utc::now().timestamp();
        if c.created_at == 0 {
            c.created_at = now;
        }
        c.updated_at = now;
        let stored = StoredCredential::from_credential(&c);
        if let Some(pos) = self.entries.iter().position(|e| e.id == c.id) {
            self.entries[pos] = stored;
        } else {
            self.entries.push(stored);
        }
        self.save()
    }

    pub fn remove(&mut self, id: &str) -> io::Result<bool> {
        let n = self.entries.len();
        self.entries.retain(|e| e.id != id);
        if self.entries.len() != n {
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    pub fn export_indexed(&self) -> HashMap<String, Credential> {
        self.list()
            .into_iter()
            .map(|c| (c.id.clone(), c))
            .collect()
    }

    pub fn restore_from_file_into_default_location(src: &std::path::Path) -> io::Result<()> {
        let dest = Self::default_path();
        fs::copy(src, dest)?;
        Ok(())
    }
}

impl Default for CredentialVault {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_roundtrip() {
        let path = std::env::temp_dir().join("mistterm-test-creds.json");
        let _ = fs::remove_file(&path);
        let mut vault = CredentialVault::new_at(path.clone());
        let c = Credential {
            id: "id1".to_string(),
            name: "测试".to_string(),
            category: CredentialCategory::Server,
            host: "1.2.3.4".to_string(),
            port: 22,
            username: "u".to_string(),
            auth: CredentialAuthKind::Password,
            secret: "secret".to_string(),
            notes: String::new(),
            tags: vec!["prod".to_string()],
            created_at: 0,
            updated_at: 0,
            secret_backend: SecretBackend::default(),
        };
        vault.upsert(c).unwrap();
        drop(vault);

        let v2 = CredentialVault::new_at(path.clone());
        assert_eq!(v2.count(), 1);
        let got = v2.get("id1").unwrap();
        assert_eq!(got.secret, "secret");

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(ENVELOPE_FORMAT));
    }

    #[test]
    fn vault_migrates_legacy_json_array() {
        let path = std::env::temp_dir().join("mistterm-test-creds-legacy.json");
        let _ = fs::remove_file(&path);
        let dk = device_key::device_key();
        let c = Credential {
            id: "legacy1".to_string(),
            name: "旧格式".to_string(),
            category: CredentialCategory::Server,
            host: "10.0.0.1".to_string(),
            port: 22,
            username: "root".to_string(),
            auth: CredentialAuthKind::Password,
            secret: "pw".to_string(),
            notes: String::new(),
            tags: vec![],
            created_at: 1,
            updated_at: 2,
            secret_backend: SecretBackend::default(),
        };
        let (enc, nonce) = device_key::encrypt_secret(&dk, "pw").unwrap();
        let stored = StoredCredential {
            id: c.id.clone(),
            name: c.name.clone(),
            category: c.category,
            host: c.host.clone(),
            port: c.port,
            username: c.username.clone(),
            auth: c.auth,
            notes: String::new(),
            tags: vec![],
            secret: String::new(),
            secret_enc: enc,
            secret_nonce: nonce,
            secret_backend: SecretBackend::default(),
            created_at: 1,
            updated_at: 2,
        };
        fs::write(&path, serde_json::to_string(&vec![stored]).unwrap()).unwrap();

        let vault = CredentialVault::new_at(path.clone());
        assert_eq!(vault.get("legacy1").unwrap().secret, "pw");

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(ENVELOPE_FORMAT));
    }
}
