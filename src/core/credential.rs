//! 凭证库：本地加密存储服务器账号、密钥、令牌等
//!
//! 文件格式 v2：使用设备根密钥（`device_key`）经 HKDF-SHA256 与文件内随机盐派生独立数据密钥，
//! 再对每条目的 `secret` 做 AES-256-GCM。旧版为纯 JSON 数组（密文直接用设备根密钥加密），首次加载时自动迁移。

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufWriter};
use std::path::PathBuf;

use crate::security::device_key;

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

    pub fn emoji(&self) -> &'static str {
        match self {
            CredentialCategory::Server => "🖥️",
            CredentialCategory::Database => "🗄️",
            CredentialCategory::SshKey => "🔑",
            CredentialCategory::Api => "🔐",
            CredentialCategory::Other => "📎",
        }
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
    secret_enc: String,
    secret_nonce: String,
    created_at: i64,
    updated_at: i64,
}

impl StoredCredential {
    fn from_credential(key: &[u8; 32], c: &Credential) -> Option<Self> {
        let (secret_enc, secret_nonce) = device_key::encrypt_secret(key, &c.secret)?;
        Some(StoredCredential {
            id: c.id.clone(),
            name: c.name.clone(),
            category: c.category,
            host: c.host.clone(),
            port: c.port,
            username: c.username.clone(),
            auth: c.auth,
            notes: c.notes.clone(),
            tags: c.tags.clone(),
            secret_enc,
            secret_nonce,
            created_at: c.created_at,
            updated_at: c.updated_at,
        })
    }

    fn to_credential(&self, key: &[u8; 32]) -> Option<Credential> {
        let secret = if self.secret_enc.is_empty() && self.secret_nonce.is_empty() {
            String::new()
        } else {
            device_key::decrypt_secret(key, &self.secret_enc, &self.secret_nonce)?
        };
        Some(Credential {
            id: self.id.clone(),
            name: self.name.clone(),
            category: self.category,
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            auth: self.auth,
            secret,
            notes: self.notes.clone(),
            tags: self.tags.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

const VAULT_FORMAT_V2: i32 = 2;

#[derive(Debug, Serialize, Deserialize)]
struct CredentialVaultFileV2 {
    #[serde(default)]
    vault_format: i32,
    salt_b64: String,
    entries: Vec<StoredCredential>,
}

fn random_vault_salt() -> [u8; 16] {
    let mut s = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut s);
    s
}

/// 凭证库（加密文件，v2 封装格式）
pub struct CredentialVault {
    entries: Vec<StoredCredential>,
    file_path: PathBuf,
    /// 与 `sessions.json` 相同的设备根密钥（仅用于 HKDF 派生）
    device_root_key: [u8; 32],
    /// 与磁盘 `salt_b64` 对应
    vault_salt: [u8; 16],
    /// 实际用于加解密 `secret_*` 字段
    vault_data_key: [u8; 32],
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

    /// 使用指定路径创建或加载凭证库（测试与自定义目录用）
    pub fn new_at(file_path: PathBuf) -> Self {
        let device_root_key = device_key::device_key();
        let mut vault = Self::new_empty_salted(file_path, device_root_key);
        if vault.file_path.exists() {
            if let Err(e) = vault.load_from_disk() {
                tracing::warn!("凭证库加载失败（将使用空库）：{}", e);
            }
        }
        vault
    }

    fn new_empty_salted(file_path: PathBuf, device_root_key: [u8; 32]) -> Self {
        let vault_salt = random_vault_salt();
        let vault_data_key =
            device_key::derive_credential_vault_data_key(&device_root_key, &vault_salt)
                .expect("salt len > 0");
        Self {
            entries: Vec::new(),
            file_path,
            device_root_key,
            vault_salt,
            vault_data_key,
        }
    }

    pub fn path(&self) -> &PathBuf {
        &self.file_path
    }

    fn load_from_disk(&mut self) -> io::Result<()> {
        let text = fs::read_to_string(&self.file_path)?;
        let val: Value = serde_json::from_str(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        match val {
            Value::Array(_) => {
                let list: Vec<StoredCredential> = serde_json::from_value(val).unwrap_or_default();
                self.migrate_legacy_array_to_v2(list)?;
            }
            Value::Object(_) => {
                let file: CredentialVaultFileV2 = serde_json::from_value(val).map_err(|e| {
                    io::Error::new(io::ErrorKind::InvalidData, e)
                })?;
                if file.vault_format != VAULT_FORMAT_V2 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "未知凭证库格式版本",
                    ));
                }
                let raw = B64
                    .decode(file.salt_b64.as_bytes())
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                if raw.len() != 16 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "凭证库盐长度无效",
                    ));
                }
                let mut salt = [0u8; 16];
                salt.copy_from_slice(&raw);
                let vk = device_key::derive_credential_vault_data_key(&self.device_root_key, &salt)
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "无法派生凭证库密钥")
                    })?;
                self.vault_salt = salt;
                self.vault_data_key = vk;
                self.entries = file.entries;
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

    /// 旧版：密文由设备根密钥直接加密；迁移为 v2 并写回磁盘
    fn migrate_legacy_array_to_v2(&mut self, legacy: Vec<StoredCredential>) -> io::Result<()> {
        let salt = random_vault_salt();
        let vk = device_key::derive_credential_vault_data_key(&self.device_root_key, &salt)
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "派生凭证库密钥失败"))?;

        let mut new_entries = Vec::with_capacity(legacy.len());
        for s in legacy {
            match s.to_credential(&self.device_root_key) {
                Some(c) => {
                    let stored = StoredCredential::from_credential(&vk, &c).ok_or_else(|| {
                        io::Error::new(io::ErrorKind::Other, "迁移时重加密失败")
                    })?;
                    new_entries.push(stored);
                }
                None => {
                    tracing::warn!("凭证「{}」解密失败，已跳过（可能密钥变更或数据损坏）", s.id);
                }
            }
        }
        self.vault_salt = salt;
        self.vault_data_key = vk;
        self.entries = new_entries;
        self.save()?;
        tracing::info!("凭证库已从旧版数组格式迁移到 v2（HKDF + 文件盐）");
        Ok(())
    }

    pub fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let file = fs::File::create(&self.file_path)?;
        let writer = BufWriter::new(file);
        let envelope = CredentialVaultFileV2 {
            vault_format: VAULT_FORMAT_V2,
            salt_b64: B64.encode(self.vault_salt),
            entries: self.entries.clone(),
        };
        serde_json::to_writer_pretty(writer, &envelope)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    pub fn list(&self) -> Vec<Credential> {
        self.entries
            .iter()
            .filter_map(|s| s.to_credential(&self.vault_data_key))
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<Credential> {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .and_then(|s| s.to_credential(&self.vault_data_key))
    }

    pub fn upsert(&mut self, mut c: Credential) -> io::Result<()> {
        let now = chrono::Utc::now().timestamp();
        if c.created_at == 0 {
            c.created_at = now;
        }
        c.updated_at = now;
        let stored = StoredCredential::from_credential(&self.vault_data_key, &c).ok_or_else(|| {
            io::Error::new(io::ErrorKind::Other, "加密凭证失败")
        })?;
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

    /// 导出为按 id 映射（用于同步包）
    pub fn export_indexed(&self) -> HashMap<String, Credential> {
        self.list()
            .into_iter()
            .map(|c| (c.id.clone(), c))
            .collect()
    }

    /// 用备份文件覆盖默认路径下的凭证库（同步包与本机同源设备密钥可读）
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
        };
        vault.upsert(c).unwrap();
        drop(vault);

        let v2 = CredentialVault::new_at(path);
        assert_eq!(v2.count(), 1);
        let got = v2.get("id1").unwrap();
        assert_eq!(got.secret, "secret");
        assert_eq!(got.host, "1.2.3.4");
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
        };
        let stored = StoredCredential::from_credential(&dk, &c).unwrap();
        fs::write(&path, serde_json::to_string(&vec![stored]).unwrap()).unwrap();

        let vault = CredentialVault::new_at(path.clone());
        assert_eq!(vault.get("legacy1").unwrap().secret, "pw");

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"vault_format\""));
        assert!(text.contains("\"salt_b64\""));
    }
}
