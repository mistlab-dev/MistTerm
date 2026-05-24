//! HashiCorp Vault 集成（KV 读写 + 认证）

mod hashicorp;

pub use hashicorp::{
    HashiCorpVaultClient, VaultAuth, VaultError, VaultKvRef, VaultListEntry,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultSettings {
    #[serde(default)]
    pub enabled: bool,
    /// 例如 `https://127.0.0.1:8200`
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub namespace: String,
    #[serde(default = "default_kv_mount")]
    pub default_mount: String,
    #[serde(default)]
    pub auth: VaultAuthSettings,
    #[serde(default)]
    pub tls_skip_verify: bool,
    /// 由团队 sync 写入时记录来源团队 id（偏好设置只读提示）
    #[serde(default)]
    pub managed_by_team_id: Option<String>,
    /// 为 false 时切换团队不再自动覆盖 Vault（用户已在偏好中改过）
    #[serde(default = "default_team_auto_apply")]
    pub team_auto_apply: bool,
}

fn default_team_auto_apply() -> bool {
    true
}

fn default_kv_mount() -> String {
    "secret".to_string()
}

impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            address: String::new(),
            namespace: String::new(),
            default_mount: default_kv_mount(),
            auth: VaultAuthSettings::default(),
            tls_skip_verify: false,
            managed_by_team_id: None,
            team_auto_apply: default_team_auto_apply(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VaultAuthSettings {
    #[default]
    None,
    Token,
    AppRole,
}
