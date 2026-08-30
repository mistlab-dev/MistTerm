//! HashiCorp Vault HTTP API（KV v2 优先，v1 只读回退）

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde_json::{json, Value};
use std::time::Duration;

use super::VaultSettings;

const KEYRING_VAULT_TOKEN: &str = "vault_token";
const KEYRING_VAULT_ROLE_ID: &str = "vault_role_id";
const KEYRING_VAULT_SECRET_ID: &str = "vault_secret_id";

/// 对 Vault KV 存储中某一条秘密的定位引用（mount + path + field + 可选版本）。
///
/// 由 [`parse_vault_credential_path`] 从 `secret/data/ssh/db-master` 这类路径解析得到，
/// 或在 UI 中手动填写 mount/path/field 后构造。
#[derive(Debug, Clone)]
pub struct VaultKvRef {
    /// KV 挂载点名称（例如 `secret`，对应 Vault CLI 的 `-mount=secret`）。
    pub mount: String,
    /// KV 内部相对路径（不含 mount 与 `/data/` 前缀，例如 `ssh/db-master`）。
    pub path: String,
    /// 读取的字段名（例如 `password` / `private_key`）。
    pub field: String,
    /// KV v2 的版本号；`None` 表示读取最新版本。v1 忽略此字段。
    pub version: Option<u32>,
}

/// Vault 列表接口返回的单条条目（文件或子目录）。
#[derive(Debug, Clone)]
pub struct VaultListEntry {
    /// 条目路径片段（父目录 + 名称，不含 mount）。
    pub path: String,
    /// `true` 表示这是子目录（以 `/` 结尾的条目），需再次调用 list 展开。
    pub is_dir: bool,
}

/// Vault 操作的统一错误枚举：配置缺失、HTTP 层、API 层、字段缺失、系统密钥链。
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    /// [`VaultSettings::address`] 为空，未配置 Vault 服务地址。
    #[error("Vault 未配置地址")]
    NoAddress,
    /// 既没有 Token 也没有 AppRole，缺少认证信息。
    #[error("未配置 Vault 认证")]
    NoAuth,
    /// HTTP 传输层错误（超时、DNS 失败、TLS 握手失败等，底层 reqwest 错误）。
    #[error("HTTP: {0}")]
    Http(String),
    /// Vault API 返回的业务错误（权限不足、路径不存在、令牌过期等）。
    #[error("API: {0}")]
    Api(String),
    /// KV 对象存在，但未找到期望读取的 [`VaultKvRef::field`] 字段名。
    #[error("字段不存在: {0}")]
    FieldMissing(String),
    /// 从操作系统 Keyring 存取 Token / AppRole 凭证时失败。
    #[error("密钥链: {0}")]
    Keyring(String),
}

/// Vault 认证方式。当前支持静态 Token 与 AppRole（role_id + secret_id）两种。
#[derive(Debug, Clone)]
pub enum VaultAuth {
    /// 直接使用 `X-Vault-Token` 头携带的 Bearer Token。
    Token(String),
    /// AppRole 工作流：使用 `role_id` 与 `secret_id` 先 `/auth/approle/login` 换取临时 Token。
    AppRole {
        /// 绑定了角色权限的 Role ID（通常是稳定的长 UUID）。
        role_id: String,
        /// 一次性或短期有效的 Secret ID（敏感，建议与 role_id 分开保管）。
        secret_id: String,
    },
}

/// 同步阻塞式 HashiCorp Vault 客户端。
///
/// 优先使用 KV v2（`/v1/{mount}/data/{path}`）；读取 KV v1 时会回退到
/// `/v1/{mount}/{path}` 并跳过 `/metadata` 调用。构造通过 [`Self::new`] 并携带
/// [`VaultSettings`]，创建时会立即验证地址非空。
#[derive(Debug)]
pub struct HashiCorpVaultClient {
    settings: VaultSettings,
    http: reqwest::blocking::Client,
}

impl HashiCorpVaultClient {
    /// 构造新的客户端，设置超时（默认 5s）与 TLS；若地址为空返回 [`VaultError::NoAddress`]。
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
        let v2_path = build_kv_v2_path(&self.settings.default_mount, reference);
        match self.request(reqwest::Method::GET, &v2_path, None) {
            Ok(v) => extract_kv_field(&v, &reference.field, true),
            Err(_) => {
                let v1_path = build_kv_v1_fallback_path(&self.settings.default_mount, reference);
                let v = self.request(reqwest::Method::GET, &v1_path, None)?;
                extract_kv_field(&v, &reference.field, false)
            }
        }
    }

    pub fn write_kv(&self, reference: &VaultKvRef, value: &str) -> Result<(), VaultError> {
        let v2_path = build_kv_v2_path(&self.settings.default_mount, reference);
        let body = build_write_kv_body(&reference.field, value);
        self.request(reqwest::Method::POST, &v2_path, Some(body))?;
        Ok(())
    }

    pub fn list_kv(&self, mount: &str, prefix: &str) -> Result<Vec<VaultListEntry>, VaultError> {
        let list_path = build_list_kv_path(&self.settings.default_mount, mount, prefix);
        let v = self.request(reqwest::Method::GET, &list_path, None)?;
        Ok(parse_list_kv_response(prefix, &v))
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

// ---- Pure path helpers (extracted from read_kv / write_kv / list_kv so we
//      can cover mount fallback, prefix normalization, slash stripping, and
//      JSON field extraction without firing a real HTTP client).

pub(crate) fn build_kv_v2_path(default_mount: &str, r: &VaultKvRef) -> String {
    let mount = if r.mount.is_empty() {
        default_mount.to_string()
    } else {
        r.mount.clone()
    };
    let path = r.path.trim_start_matches('/');
    format!("/v1/{mount}/data/{path}")
}

pub(crate) fn build_kv_v1_fallback_path(default_mount: &str, r: &VaultKvRef) -> String {
    let mount = if r.mount.is_empty() {
        default_mount.to_string()
    } else {
        r.mount.clone()
    };
    let path = r.path.trim_start_matches('/');
    format!("/v1/{mount}/{path}")
}

pub(crate) fn build_list_kv_path(default_mount: &str, mount: &str, prefix: &str) -> String {
    let mount = if mount.is_empty() { default_mount } else { mount };
    let prefix = prefix.trim_matches('/');
    if prefix.is_empty() {
        format!("/v1/{mount}/metadata?list=true")
    } else {
        format!("/v1/{mount}/metadata/{prefix}?list=true")
    }
}

pub(crate) fn build_write_kv_body(field: &str, value: &str) -> Value {
    json!({ "data": { field: value } })
}

pub(crate) fn build_approle_login_url(address: &str) -> String {
    format!(
        "{}/v1/auth/approle/login",
        address.trim_end_matches('/')
    )
}

pub(crate) fn parse_list_kv_response(prefix: &str, body: &Value) -> Vec<VaultListEntry> {
    let prefix = prefix.trim_matches('/');
    let keys = body["data"]["keys"]
        .as_array()
        .cloned()
        .unwrap_or_default();
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
    out
}

#[cfg(test)]
mod pure_logic_tests {
    use super::*;

    fn kv_ref(mount: &str, path: &str, field: &str, version: Option<u32>) -> VaultKvRef {
        VaultKvRef {
            mount: mount.into(),
            path: path.into(),
            field: field.into(),
            version,
        }
    }

    // ---------------- build_kv paths

    #[test]
    fn build_kv_v2_uses_explicit_mount_over_default() {
        let p = build_kv_v2_path("secret", &kv_ref("kv-platform", "app/db", "pass", None));
        assert_eq!(p, "/v1/kv-platform/data/app/db");
    }

    #[test]
    fn build_kv_v2_falls_back_to_default_mount() {
        let p = build_kv_v2_path("secret", &kv_ref("", "a/b", "k", None));
        assert_eq!(p, "/v1/secret/data/a/b");
    }

    #[test]
    fn build_kv_v2_strips_leading_slash_from_path() {
        let p = build_kv_v2_path("secret", &kv_ref("", "//leading//slashes", "k", None));
        // trim_start_matches('/') leaves no leading `/` in the middle segment.
        assert_eq!(p, "/v1/secret/data/leading//slashes");
    }

    #[test]
    fn build_kv_v1_same_normalization_as_v2() {
        let p = build_kv_v1_fallback_path("secret", &kv_ref("", "/env/app", "pwd", None));
        assert_eq!(p, "/v1/secret/env/app");
    }

    // ---------------- list_kv path

    #[test]
    fn build_list_root_without_prefix() {
        let p = build_list_kv_path("secret", "", "");
        assert_eq!(p, "/v1/secret/metadata?list=true");
    }

    #[test]
    fn build_list_with_prefix_trims_slashes() {
        let p = build_list_kv_path("secret", "", "/team/ops/");
        assert_eq!(p, "/v1/secret/metadata/team/ops?list=true");
    }

    #[test]
    fn build_list_uses_explicit_mount() {
        let p = build_list_kv_path("secret", "team-kv", "/");
        assert_eq!(p, "/v1/team-kv/metadata?list=true");
    }

    // ---------------- write_kv body

    #[test]
    fn write_body_nests_field_inside_data() {
        let v = build_write_kv_body("password", "hunter2");
        assert_eq!(v["data"]["password"].as_str(), Some("hunter2"));
        // No spurious other keys.
        assert_eq!(v.as_object().unwrap().len(), 1);
        assert_eq!(v["data"].as_object().unwrap().len(), 1);
    }

    // ---------------- approle login URL trimming

    #[test]
    fn approle_url_strips_trailing_slash() {
        let u = build_approle_login_url("https://vault.corp:8200/");
        assert_eq!(u, "https://vault.corp:8200/v1/auth/approle/login");
    }

    #[test]
    fn approle_url_works_without_trailing_slash() {
        let u = build_approle_login_url("https://vault.corp:8200");
        assert_eq!(u, "https://vault.corp:8200/v1/auth/approle/login");
    }

    // ---------------- extract_kv_field (v2 / v1 / number / bool / missing)

    #[test]
    fn extract_v2_reads_data_dot_data() {
        let body = json!({ "data": { "data": { "password": "top" } } });
        let out = extract_kv_field(&body, "password", true).unwrap();
        assert_eq!(out, "top");
    }

    #[test]
    fn extract_v1_reads_flat_data() {
        let body = json!({ "data": { "password": "top" } });
        let out = extract_kv_field(&body, "password", false).unwrap();
        assert_eq!(out, "top");
    }

    #[test]
    fn extract_coerces_numbers_and_bools_to_string() {
        let body_v1_num = json!({ "data": { "port": 5432 } });
        assert_eq!(
            extract_kv_field(&body_v1_num, "port", false).unwrap(),
            "5432"
        );
        let body_v2_bool = json!({ "data": { "data": { "tls": true } } });
        assert_eq!(
            extract_kv_field(&body_v2_bool, "tls", true).unwrap(),
            "true"
        );
    }

    #[test]
    fn extract_missing_returns_field_missing_variant() {
        let body = json!({ "data": {} });
        let err = extract_kv_field(&body, "nope", false).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("nope"));
    }

    // ---------------- parse_list_kv_response

    #[test]
    fn parse_list_combines_prefix_and_dir_vs_file_flags() {
        let body = json!({
            "data": {
                "keys": [ "apps/", "apps/web.conf", "db.conf" ]
            }
        });
        let entries = parse_list_kv_response("", &body);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].path, "apps");
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].path, "apps/web.conf");
        assert!(!entries[1].is_dir);
        assert_eq!(entries[2].path, "db.conf");
        assert!(!entries[2].is_dir);
    }

    #[test]
    fn parse_list_missing_keys_yields_empty_entries() {
        let empty_body = json!({"data": {}});
        assert!(parse_list_kv_response("", &empty_body).is_empty());
        let malformed = json!({"data": { "keys": "not an array" }});
        assert!(parse_list_kv_response("", &malformed).is_empty());
    }

    #[test]
    fn parse_list_with_prefix_prepends_it() {
        let body = json!({ "data": { "keys": ["service/", "creds.json"] } });
        let entries = parse_list_kv_response("/team/ops/", &body);
        assert_eq!(entries[0].path, "team/ops/service");
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].path, "team/ops/creds.json");
    }

    // ---------------- base64 decode helper

    #[test]
    fn decode_b64_standard_roundtrip_and_invalid() {
        use base64::Engine as _;
        let plain = "hello vault";
        let encoded = B64.encode(plain);
        assert_eq!(decode_b64(&encoded).as_deref(), Some(plain));
        assert_eq!(decode_b64("!!!not base64!!!"), None);
        assert_eq!(decode_b64("not-valid-b64=="), None); // non-alphabet
    }
}
