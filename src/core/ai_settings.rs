//! 用户自配的 OpenAI 兼容 AI 设置（明文仅存于 device_key 加密后的 `settings.json` 内）。

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

fn default_request_retries() -> u32 {
    2
}

fn default_system_prompt() -> String {
    String::new()
}

fn default_true() -> bool {
    true
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
    /// 网络错误时的额外重试次数（不含首次请求）。
    #[serde(default = "default_request_retries")]
    pub request_retries: u32,
    /// 空字符串时使用内置默认 system prompt。
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    /// 发送时附带 host / 会话名等元信息。
    #[serde(default = "default_true")]
    pub attach_session_meta: bool,
    /// 使用 SSE 流式输出（不支持时回退整段响应）。
    #[serde(default = "default_true")]
    pub stream_responses: bool,
    /// 按会话持久化 AI 对话。
    #[serde(default = "default_true")]
    pub persist_chats: bool,
    /// 仅存在于加密后的 settings 内层 JSON，勿写入审计日志。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    api_key: String,
    /// 旧版字段：加载后迁移到 [`api_key`]。
    #[serde(default, skip_serializing, rename = "encrypted_api_key")]
    legacy_encrypted_api_key: String,
    #[serde(default, skip_serializing, rename = "api_key_nonce")]
    legacy_api_key_nonce: String,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: default_base_url(),
            model: default_model(),
            timeout_secs: default_timeout_secs(),
            max_tokens: default_max_tokens(),
            request_retries: default_request_retries(),
            system_prompt: default_system_prompt(),
            attach_session_meta: true,
            stream_responses: true,
            persist_chats: true,
            api_key: String::new(),
            legacy_encrypted_api_key: String::new(),
            legacy_api_key_nonce: String::new(),
        }
    }
}

impl AiSettings {
    pub fn chat_completions_url(&self) -> String {
        let base = self.base_url.trim().trim_end_matches('/');
        format!("{base}/chat/completions")
    }

    pub fn models_url(&self) -> String {
        let base = self.base_url.trim().trim_end_matches('/');
        format!("{base}/models")
    }

    pub fn has_api_key(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    pub fn load_api_key(&self) -> Option<String> {
        let k = self.api_key.trim();
        if k.is_empty() {
            None
        } else {
            Some(k.to_string())
        }
    }

    pub fn set_api_key(&mut self, key: &str) -> Result<(), String> {
        self.api_key = key.trim().to_string();
        Ok(())
    }

    pub fn clear_api_key(&mut self) {
        self.api_key.clear();
    }

    /// 旧版 per-field 加密或钥匙串 → 明文 `api_key`（在 `AppSettings::load` 内调用）。
    pub fn migrate_legacy_secrets(&mut self) -> bool {
        let mut changed = false;
        if self.api_key.is_empty()
            && !self.legacy_encrypted_api_key.is_empty()
            && !self.legacy_api_key_nonce.is_empty()
        {
            let dk = crate::security::device_key::device_key();
            if let Some(plain) = crate::security::device_key::decrypt_secret(
                &dk,
                &self.legacy_encrypted_api_key,
                &self.legacy_api_key_nonce,
            ) {
                self.api_key = plain;
                changed = true;
            }
            self.legacy_encrypted_api_key.clear();
            self.legacy_api_key_nonce.clear();
        }
        if self.api_key.is_empty() {
            if self.migrate_keyring_to_local() {
                changed = true;
            }
        }
        changed
    }

    /// 从旧版系统钥匙串迁入（一次性）。
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
        self.api_key = legacy;
        let _ = mgr.delete_password(KEYRING_LEGACY_USER);
        true
    }

    pub fn ready(&self) -> bool {
        self.enabled && self.has_api_key() && !self.model.trim().is_empty()
    }
}
