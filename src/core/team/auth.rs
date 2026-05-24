//! 团队 access/refresh token：device_key 加密整文件 `team_tokens.json`。

use std::path::PathBuf;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const KEYRING_LEGACY_SERVICE: &str = "MistTerm-Team";
const KEY_ACCESS: &str = "team_access_token";
const KEY_REFRESH: &str = "team_refresh_token";

#[derive(Debug, Error)]
pub enum KeyringError {
    #[error("保存 token 失败：{0}")]
    SaveError(String),

    #[error("读取 token 失败：{0}")]
    GetError(String),

    #[error("token 未找到")]
    NotFound,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TeamTokenFile {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    refresh_token: String,
}

/// 无状态；读写加密配置文件。
#[derive(Debug, Default)]
pub struct TeamTokenStore;

impl TeamTokenStore {
    fn token_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("team_tokens.json");
        p
    }

    fn load_plain(&self) -> TeamTokenFile {
        migrate_keyring_to_file_once();
        crate::security::encrypted_file::load_encrypted_json(&Self::token_path())
    }

    fn save_plain(&self, file: &TeamTokenFile) -> Result<(), KeyringError> {
        crate::security::encrypted_file::save_encrypted_json(&Self::token_path(), file)
            .map_err(|e| KeyringError::SaveError(e.to_string()))
    }

    pub fn save_tokens(&self, access: &str, refresh: &str) -> Result<(), KeyringError> {
        self.save_plain(&TeamTokenFile {
            access_token: access.to_string(),
            refresh_token: refresh.to_string(),
        })
    }

    pub fn load_access_token(&self) -> Result<String, KeyringError> {
        let file = self.load_plain();
        if file.access_token.is_empty() {
            return Err(KeyringError::NotFound);
        }
        Ok(file.access_token)
    }

    pub fn load_refresh_token(&self) -> Result<String, KeyringError> {
        let file = self.load_plain();
        if file.refresh_token.is_empty() {
            return Err(KeyringError::NotFound);
        }
        Ok(file.refresh_token)
    }

    pub fn clear(&self) {
        let path = Self::token_path();
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
        clear_legacy_keyring_entries();
    }

    pub fn has_tokens(&self) -> bool {
        let file = self.load_plain();
        !file.access_token.is_empty() && !file.refresh_token.is_empty()
    }
}

#[derive(Debug, Deserialize)]
struct JwtClaims {
    exp: Option<i64>,
}

/// JWT `exp` 为 Unix 秒；解析失败视为已过期。
pub fn jwt_exp_unix(token: &str) -> Option<i64> {
    let payload_b64 = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
    let claims: JwtClaims = serde_json::from_slice(&bytes).ok()?;
    claims.exp
}

/// 距过期不足 `skew_secs` 秒则视为需要刷新。
pub fn token_needs_refresh(token: &str, skew_secs: i64) -> bool {
    let Some(exp) = jwt_exp_unix(token) else {
        return true;
    };
    let now = chrono::Utc::now().timestamp();
    exp.saturating_sub(skew_secs) <= now
}

fn migrate_keyring_to_file_once() {
    let path = TeamTokenStore::token_path();
    if path.exists() {
        let file: TeamTokenFile = crate::security::encrypted_file::load_encrypted_json(&path);
        if !file.access_token.is_empty() {
            return;
        }
        // 可能是旧版 per-field 明文信封，load_encrypted_json 已处理迁移
    }
    let mgr = crate::security::CredentialManager::with_service(KEYRING_LEGACY_SERVICE);
    let Ok(access) = mgr.get_password(KEY_ACCESS) else {
        return;
    };
    let Ok(refresh) = mgr.get_password(KEY_REFRESH) else {
        return;
    };
    if access.trim().is_empty() || refresh.trim().is_empty() {
        return;
    }
    if TeamTokenStore::default()
        .save_tokens(&access, &refresh)
        .is_err()
    {
        return;
    }
    clear_legacy_keyring_entries();
    log::info!("Migrated team tokens to device_key encrypted config file");
}

fn clear_legacy_keyring_entries() {
    let mgr = crate::security::CredentialManager::with_service(KEYRING_LEGACY_SERVICE);
    let _ = mgr.delete_password(KEY_ACCESS);
    let _ = mgr.delete_password(KEY_REFRESH);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn team_token_encrypted_file_roundtrip() {
        let path = std::env::temp_dir().join("mistterm_team_tokens_test.json");
        let _ = fs::remove_file(&path);
        crate::security::encrypted_file::save_encrypted_json(
            &path,
            &TeamTokenFile {
                access_token: "a".into(),
                refresh_token: "r".into(),
            },
        )
        .unwrap();
        let loaded: TeamTokenFile = crate::security::encrypted_file::load_encrypted_json(&path);
        assert_eq!(loaded.access_token, "a");
        assert_eq!(loaded.refresh_token, "r");
        let _ = fs::remove_file(&path);
    }
}
