//! 系统密钥链管理
#![allow(dead_code)]
//!
//! 跨平台安全存储密码和敏感信息
//!
//! - macOS: Keychain
//! - Windows: Credential Manager
//! - Linux: Secret Service (GNOME Keyring, KWallet)

use thiserror::Error;

/// 密钥链错误
#[derive(Error, Debug)]
pub enum KeyringError {
    #[error("保存密码失败：{0}")]
    SaveError(String),

    #[error("获取密码失败：{0}")]
    GetError(String),

    #[error("删除密码失败：{0}")]
    DeleteError(String),

    #[error("密码未找到")]
    NotFound,

    #[error("密钥链不可用：{0}")]
    Unavailable(String),
}

/// 凭证管理器
pub struct CredentialManager {
    service: String,
}

impl CredentialManager {
    /// 创建新的凭证管理器
    pub fn new() -> Self {
        CredentialManager {
            service: "MistTerm".to_string(),
        }
    }

    /// 创建带自定义服务名的凭证管理器
    pub fn with_service(service: &str) -> Self {
        CredentialManager {
            service: service.to_string(),
        }
    }

    /// 保存密码
    pub fn save_password(&self, username: &str, password: &str) -> Result<(), KeyringError> {
        // TODO: 使用 keyring crate 实现
        tracing::debug!("保存密码：service={}, username={}", self.service, username);
        let _ = password; // 临时使用
        Ok(())
    }

    /// 获取密码
    pub fn get_password(&self, username: &str) -> Result<String, KeyringError> {
        // TODO: 使用 keyring crate 实现
        tracing::debug!("获取密码：service={}, username={}", self.service, username);
        let _ = username;
        Err(KeyringError::NotFound)
    }

    /// 删除密码
    pub fn delete_password(&self, username: &str) -> Result<(), KeyringError> {
        // TODO: 使用 keyring crate 实现
        tracing::debug!("删除密码：service={}, username={}", self.service, username);
        let _ = username;
        Ok(())
    }

    /// 检查密钥链是否可用
    pub fn is_available() -> bool {
        true
    }

    /// 获取服务名
    pub fn service(&self) -> &str {
        &self.service
    }
}

impl Default for CredentialManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 凭证存储（高级 API）
pub struct CredentialStore {
    manager: CredentialManager,
}

impl CredentialStore {
    /// 创建新的凭证存储
    pub fn new() -> Self {
        CredentialStore {
            manager: CredentialManager::new(),
        }
    }

    /// 保存会话凭证
    pub fn save_session_credential(
        &mut self,
        session_id: &str,
        _host: &str,
        username: &str,
        password: &str,
    ) -> Result<(), KeyringError> {
        let entry_name = format!("{}:{}", session_id, username);
        self.manager.save_password(&entry_name, password)
    }

    /// 获取会话凭证
    pub fn get_session_credential(
        &mut self,
        session_id: &str,
        username: &str,
    ) -> Result<String, KeyringError> {
        let entry_name = format!("{}:{}", session_id, username);
        self.manager.get_password(&entry_name)
    }

    /// 删除会话凭证
    pub fn delete_session_credential(
        &mut self,
        session_id: &str,
        username: &str,
    ) -> Result<(), KeyringError> {
        let entry_name = format!("{}:{}", session_id, username);
        self.manager.delete_password(&entry_name)
    }
}

impl Default for CredentialStore {
    fn default() -> Self {
        Self::new()
    }
}
