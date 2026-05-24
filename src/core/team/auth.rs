//! 团队 access/refresh token：系统密钥链 + JWT 过期判断。

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Deserialize;

use crate::security::{CredentialManager, KeyringError};

const KEY_ACCESS: &str = "team_access_token";
const KEY_REFRESH: &str = "team_refresh_token";

#[derive(Debug, Deserialize)]
struct JwtClaims {
    exp: Option<i64>,
}

pub struct TeamTokenStore {
    keyring: CredentialManager,
}

impl Default for TeamTokenStore {
    fn default() -> Self {
        Self {
            keyring: CredentialManager::with_service("MistTerm-Team"),
        }
    }
}

impl TeamTokenStore {
    pub fn save_tokens(&self, access: &str, refresh: &str) -> Result<(), KeyringError> {
        self.keyring.save_password(KEY_ACCESS, access)?;
        self.keyring.save_password(KEY_REFRESH, refresh)?;
        Ok(())
    }

    pub fn load_access_token(&self) -> Result<String, KeyringError> {
        self.keyring.get_password(KEY_ACCESS)
    }

    pub fn load_refresh_token(&self) -> Result<String, KeyringError> {
        self.keyring.get_password(KEY_REFRESH)
    }

    pub fn clear(&self) {
        let _ = self.keyring.delete_password(KEY_ACCESS);
        let _ = self.keyring.delete_password(KEY_REFRESH);
    }

    pub fn has_tokens(&self) -> bool {
        self.load_access_token().is_ok() && self.load_refresh_token().is_ok()
    }
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
