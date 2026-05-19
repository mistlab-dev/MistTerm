//! HashiCorp Vault HTTP API（KV v2 优先，v1 只读回退）

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde_json::{json, Value};
use std::time::Duration;

use super::VaultSettings;

const KEYRING_VAULT_TOKEN: &str = "vault_token";
const KEYRING_VAULT_ROLE_ID: &str = "vault_role_id";
const KEYRING_VAULT_SECRET_ID: &str = "vault_secret_id";

#[derive(Debug, Clone)]
pub struct VaultKvRef {
    pub mount: String,
    pub path: String,
    pub field: String,
    pub version: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct VaultListEntry {
    pub path: String,
    pub is_dir: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("Vault 未配置地址")]
    NoAddress,
    #[error("未配置 Vault 认证")]
    NoAuth,
    #[error("HTTP: {0}")]
    Http(String),
    #[error("API: {0}")]
    Api(String),
    #[error("字段不存在: {0}")]
    FieldMissing(String),
    #[error("密钥链: {0}")]
    Keyring(String),
}

#[derive(Debug, Clone)]
pub enum VaultAuth {
    Token(String),
    AppRole { role_id: String, secret_id: String },
}

pub struct HashiCorpVaultClient {
    settings: VaultSettings,
    http: reqwest::blocking::Client,
}

impl HashiCorpVaultClient {
    pub fn new(settings: VaultSettings) -> Result<Self, VaultError> {
        if settings.address.is_empty() {
            return Err(VaultError::NoAddress);
        }
        let mut builder = reqwest::blocking::Client::builder().timeout(Duration::from_secs(30));
        if settings.tls_skip_verify {
            builder = builder.danger_accept_invalid_certs(true);
        }
        let http = builder
            .build()
            .map_err(|e| VaultError::Http(e.to_string()))?;
        Ok(Self { settings, http })
    }

    pub fn resolve_auth(&self) -> Result<VaultAuth, VaultError> {
        let km = crate::security::CredentialManager::new();
        match self.settings.auth {
            super::VaultAuthSettings::Token => {
                let token = km
                    .get_password(KEYRING_VAULT_TOKEN)
                    .map_err(|e| VaultError::Keyring(e.to_string()))?;
                Ok(VaultAuth::Token(token))
            }
            super::VaultAuthSettings::AppRole => {
                let role_id = km
                    .get_password(KEYRING_VAULT_ROLE_ID)
                    .map_err(|e| VaultError::Keyring(e.to_string()))?;
                let secret_id = km
                    .get_password(KEYRING_VAULT_SECRET_ID)
                    .map_err(|e| VaultError::Keyring(e.to_string()))?;
                Ok(VaultAuth::AppRole { role_id, secret_id })
            }
            super::VaultAuthSettings::None => Err(VaultError::NoAuth),
        }
    }

    pub fn save_token_to_keyring(token: &str) -> Result<(), VaultError> {
        crate::security::CredentialManager::new()
            .save_password(KEYRING_VAULT_TOKEN, token)
            .map_err(|e| VaultError::Keyring(e.to_string()))
    }

    pub fn save_approle_to_keyring(role_id: &str, secret_id: &str) -> Result<(), VaultError> {
        let km = crate::security::CredentialManager::new();
        km.save_password(KEYRING_VAULT_ROLE_ID, role_id)
            .map_err(|e| VaultError::Keyring(e.to_string()))?;
        km.save_password(KEYRING_VAULT_SECRET_ID, secret_id)
            .map_err(|e| VaultError::Keyring(e.to_string()))
    }

    fn token(&self) -> Result<String, VaultError> {
        let auth = self.resolve_auth()?;
        match auth {
            VaultAuth::Token(t) => Ok(t),
            VaultAuth::AppRole { role_id, secret_id } => self.login_approle(&role_id, &secret_id),
        }
    }

    fn login_approle(&self, role_id: &str, secret_id: &str) -> Result<String, VaultError> {
        let url = format!("{}/v1/auth/approle/login", self.settings.address.trim_end_matches('/'));
        let body = json!({ "role_id": role_id, "secret_id": secret_id });
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .map_err(|e| VaultError::Http(e.to_string()))?;
        let status = resp.status();
        let text = resp.text().map_err(|e| VaultError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(VaultError::Api(format!("approle login {status}: {text}")));
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| VaultError::Api(e.to_string()))?;
        v["auth"]["client_token"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| VaultError::Api("missing client_token".into()))
    }

    fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, VaultError> {
        let token = self.token()?;
        let url = format!(
            "{}{}",
            self.settings.address.trim_end_matches('/'),
            path
        );
        let mut req = self.http.request(method, &url).header("X-Vault-Token", token);
        if !self.settings.namespace.is_empty() {
            req = req.header("X-Vault-Namespace", &self.settings.namespace);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().map_err(|e| VaultError::Http(e.to_string()))?;
        let status = resp.status();
        let text = resp.text().map_err(|e| VaultError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(VaultError::Api(format!("{status}: {text}")));
        }
        serde_json::from_str(&text).map_err(|e| VaultError::Api(e.to_string()))
    }

    pub fn read_kv(&self, reference: &VaultKvRef) -> Result<String, VaultError> {
        let mount = if reference.mount.is_empty() {
            self.settings.default_mount.clone()
        } else {
            reference.mount.clone()
        };
        let path = reference.path.trim_start_matches('/');
        let v2_path = format!("/v1/{mount}/data/{path}");
        match self.request(reqwest::Method::GET, &v2_path, None) {
            Ok(v) => extract_kv_field(&v, &reference.field, true),
            Err(_) => {
                let v1_path = format!("/v1/{mount}/{path}");
                let v = self.request(reqwest::Method::GET, &v1_path, None)?;
                extract_kv_field(&v, &reference.field, false)
            }
        }
    }

    pub fn write_kv(&self, reference: &VaultKvRef, value: &str) -> Result<(), VaultError> {
        let mount = if reference.mount.is_empty() {
            self.settings.default_mount.clone()
        } else {
            reference.mount.clone()
        };
        let path = reference.path.trim_start_matches('/');
        let v2_path = format!("/v1/{mount}/data/{path}");
        let field = reference.field.clone();
        let body = json!({ "data": { field: value } });
        self.request(reqwest::Method::POST, &v2_path, Some(body))?;
        Ok(())
    }

    pub fn list_kv(&self, mount: &str, prefix: &str) -> Result<Vec<VaultListEntry>, VaultError> {
        let mount = if mount.is_empty() {
            self.settings.default_mount.as_str()
        } else {
            mount
        };
        let prefix = prefix.trim_matches('/');
        let list_path = if prefix.is_empty() {
            format!("/v1/{mount}/metadata?list=true")
        } else {
            format!("/v1/{mount}/metadata/{prefix}?list=true")
        };
        let v = self.request(reqwest::Method::GET, &list_path, None)?;
        let keys = v["data"]["keys"]
            .as_array()
            .or_else(|| v["data"]["keys"].as_array());
        let keys = keys.cloned().unwrap_or_default();
        let mut out = Vec::new();
        for k in keys {
            if let Some(s) = k.as_str() {
                let is_dir = s.ends_with('/');
                out.push(VaultListEntry {
                    path: if prefix.is_empty() {
                        s.trim_end_matches('/').to_string()
                    } else {
                        format!("{prefix}/{}", s.trim_end_matches('/'))
                    },
                    is_dir,
                });
            }
        }
        Ok(out)
    }

    pub fn test_connection(&self) -> Result<(), VaultError> {
        let _ = self.token()?;
        let _ = self.request(reqwest::Method::GET, "/v1/sys/health", None)?;
        Ok(())
    }
}

fn extract_kv_field(v: &Value, field: &str, v2: bool) -> Result<String, VaultError> {
    let data = if v2 {
        &v["data"]["data"]
    } else {
        &v["data"]
    };
    if let Some(s) = data[field].as_str() {
        return Ok(s.to_string());
    }
    if let Some(n) = data[field].as_number() {
        return Ok(n.to_string());
    }
    if let Some(b) = data[field].as_bool() {
        return Ok(b.to_string());
    }
    Err(VaultError::FieldMissing(field.to_string()))
}

#[allow(dead_code)]
fn decode_b64(s: &str) -> Option<String> {
    B64.decode(s).ok().and_then(|b| String::from_utf8(b).ok())
}
