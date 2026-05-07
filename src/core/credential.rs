//! 凭证库：本地加密存储服务器账号、密钥、令牌等
//!
//! 密文使用与会话相同的设备绑定密钥（`security::device_key`）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufReader, BufWriter};
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
        let secret =
            device_key::decrypt_secret(key, &self.secret_enc, &self.secret_nonce).unwrap_or_default();
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

/// 凭证库（加密文件）
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
        let file_path = Self::default_path();
        let device_key = device_key::device_key();
        let mut vault = Self {
            entries: Vec::new(),
            file_path,
            device_key,
        };
        if let Err(e) = vault.load() {
            tracing::warn!("凭证库加载失败（将使用空库）：{}", e);
        }
        vault
    }

    pub fn path(&self) -> &PathBuf {
        &self.file_path
    }

    fn load(&mut self) -> io::Result<()> {
        if !self.file_path.exists() {
            return Ok(());
        }
        let file = fs::File::open(&self.file_path)?;
        let reader = BufReader::new(file);
        let list: Vec<StoredCredential> = serde_json::from_reader(reader).unwrap_or_default();
        self.entries = list;
        Ok(())
    }

    pub fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.file_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let file = fs::File::create(&self.file_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &self.entries)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
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
        let stored = StoredCredential::from_credential(&self.device_key, &c).ok_or_else(|| {
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
        let key = device_key::device_key();
        let mut vault = CredentialVault {
            entries: vec![],
            file_path: path.clone(),
            device_key: key,
        };
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

        let mut v2 = CredentialVault {
            entries: vec![],
            file_path: path,
            device_key: key,
        };
        v2.load().unwrap();
        assert_eq!(v2.count(), 1);
        let got = v2.get("id1").unwrap();
        assert_eq!(got.secret, "secret");
        assert_eq!(got.host, "1.2.3.4");
    }
}
