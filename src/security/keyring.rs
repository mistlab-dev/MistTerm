//! 系统密钥链管理
//!
//! 跨平台安全存储密码和敏感信息
//! - macOS: Keychain
//! - Windows: Credential Manager
//! - Linux: Secret Service (GNOME Keyring, KWallet)
//!
//! FUNCTIONAL_SPEC §5：无 GUI/密钥服务时 `keyring` 可能失败，上层应回退本地加密并提示用户；
//! 密钥材料丢失后需重新录入密码（见产品文档 §5.4）。

use keyring::Entry;
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
        let entry = Entry::new(&self.service, username).map_err(|e| {
            KeyringError::Unavailable(format!("创建条目失败：{}", e))
        })?;
        entry
            .set_password(password)
            .map_err(|e| KeyringError::SaveError(e.to_string()))
    }

    /// 获取密码
    pub fn get_password(&self, username: &str) -> Result<String, KeyringError> {
        let entry = Entry::new(&self.service, username).map_err(|e| {
            KeyringError::Unavailable(format!("创建条目失败：{}", e))
        })?;
        match entry.get_password() {
            Ok(p) => Ok(p),
            Err(keyring::Error::NoEntry) => Err(KeyringError::NotFound),
            Err(e) => Err(KeyringError::GetError(e.to_string())),
        }
    }

    /// 删除密码
    pub fn delete_password(&self, username: &str) -> Result<(), KeyringError> {
        let entry = Entry::new(&self.service, username).map_err(|e| {
            KeyringError::Unavailable(format!("创建条目失败：{}", e))
        })?;
        entry
            .delete_password()
            .map_err(|e| KeyringError::DeleteError(e.to_string()))
    }

    /// 检查密钥链是否可用（能否创建条目）
    pub fn is_available() -> bool {
        Entry::new("MistTerm", "healthcheck").is_ok()
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
