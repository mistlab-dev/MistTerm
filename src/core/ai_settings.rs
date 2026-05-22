//! 用户自配的 OpenAI 兼容 AI 设置（API Key 本地 AES-GCM 加密存 settings.json）。

use serde::{Deserialize, Serialize};

const KEYRING_LEGACY_USER: &str = "openai-api-key";

fn default_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_model() -> String {
    "gpt-4o-mini".to_string()
}

fn default_timeout_secs() -> u64 {
    60
}

fn default_max_tokens() -> u32 {
    2048
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// AES-GCM 密文（base64），与 [`api_key_nonce`] 成对
    #[serde(default, skip_serializing_if = "String::is_empty")]
    encrypted_api_key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    api_key_nonce: String,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: default_base_url(),
            model: default_model(),
            timeout_secs: default_timeout_secs(),
            max_tokens: default_max_tokens(),
            encrypted_api_key: String::new(),
            api_key_nonce: String::new(),
        }
    }
}

impl AiSettings {
    pub fn chat_completions_url(&self) -> String {
        let base = self.base_url.trim().trim_end_matches('/');
        format!("{base}/chat/completions")
    }

    pub fn has_api_key(&self) -> bool {
        self.load_api_key()
            .map(|k| !k.trim().is_empty())
            .unwrap_or(false)
    }

    pub fn load_api_key(&self) -> Option<String> {
        let key = crate::security::device_key::device_key();
        let plain =
            crate::security::device_key::decrypt_secret(&key, &self.encrypted_api_key, &self.api_key_nonce)?;
        if plain.trim().is_empty() {
            None
        } else {
            Some(plain)
        }
    }

    /// 加密写入 API Key（仅存于 `settings.json`，不使用系统钥匙串）。
    pub fn set_api_key(&mut self, key: &str) -> Result<(), String> {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            self.clear_api_key();
            return Ok(());
        }
        let dk = crate::security::device_key::device_key();
        let (enc, nonce) = crate::security::device_key::encrypt_secret(&dk, trimmed)
            .ok_or_else(|| "加密 API Key 失败".to_string())?;
        self.encrypted_api_key = enc;
        self.api_key_nonce = nonce;
        Ok(())
    }

    pub fn clear_api_key(&mut self) {
        self.encrypted_api_key.clear();
        self.api_key_nonce.clear();
    }

    /// 从旧版系统钥匙串迁入本地加密存储（一次性，成功后删除钥匙串条目）。
    pub fn migrate_keyring_to_local(&mut self) -> bool {
        if self.has_api_key() {
            return false;
        }
        let mgr = crate::security::CredentialManager::new();
        let Ok(legacy) = mgr.get_password(KEYRING_LEGACY_USER) else {
            return false;
        };
        if legacy.trim().is_empty() {
            return false;
        }
        if self.set_api_key(&legacy).is_err() {
            return false;
        }
        let _ = mgr.delete_password(KEYRING_LEGACY_USER);
        true
    }

    pub fn ready(&self) -> bool {
        self.enabled && self.has_api_key() && !self.model.trim().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_encrypt_roundtrip() {
        let mut s = AiSettings::default();
        s.set_api_key("sk-test-123").unwrap();
        assert_eq!(s.load_api_key().as_deref(), Some("sk-test-123"));
        s.clear_api_key();
        assert!(!s.has_api_key());
    }
}
