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

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------ VaultSettings defaults

    #[test]
    fn vault_settings_default_values_match_contract() {
        let d = VaultSettings::default();
        assert!(!d.enabled);            // disabled by default for safety
        assert_eq!(d.address, "");
        assert_eq!(d.namespace, "");
        assert_eq!(d.default_mount, "secret");
        assert_eq!(d.auth, VaultAuthSettings::None);
        assert!(!d.tls_skip_verify);    // don't skip TLS verification by default
        assert_eq!(d.managed_by_team_id, None);
        assert!(d.team_auto_apply);     // team sync applies by default
    }

    #[test]
    fn default_helpers_match_struct_default() {
        assert_eq!(default_kv_mount(), "secret");
        assert!(default_team_auto_apply());
    }

    // ------------------------------------------------ VaultAuthSettings

    #[test]
    fn vault_auth_settings_default_is_none() {
        assert_eq!(VaultAuthSettings::default(), VaultAuthSettings::None);
    }

    #[test]
    fn vault_auth_settings_serde_snake_case() {
        for (variant, expected) in [
            (VaultAuthSettings::None, "none"),
            (VaultAuthSettings::Token, "token"),
            (VaultAuthSettings::AppRole, "app_role"),
        ] {
            let j = serde_json::to_string(&variant).unwrap();
            assert_eq!(j, format!("\"{}\"", expected));
            let parsed: VaultAuthSettings = serde_json::from_str(&j).unwrap();
            assert_eq!(parsed, variant);
        }
    }

    // ------------------------------------------------ VaultSettings serde

    #[test]
    fn vault_settings_serde_empty_object_equals_default() {
        let s: VaultSettings = serde_json::from_str("{}").unwrap();
        let d = VaultSettings::default();
        assert_eq!(s.enabled, d.enabled);
        assert_eq!(s.address, d.address);
        assert_eq!(s.namespace, d.namespace);
        assert_eq!(s.default_mount, d.default_mount);
        assert_eq!(s.auth, d.auth);
        assert_eq!(s.tls_skip_verify, d.tls_skip_verify);
        assert_eq!(s.managed_by_team_id, d.managed_by_team_id);
        assert_eq!(s.team_auto_apply, d.team_auto_apply);
    }

    #[test]
    fn vault_settings_serde_roundtrip_preserves_all_fields() {
        let s = VaultSettings {
            enabled: true,
            address: "https://vault.corp:8200".into(),
            namespace: "team/platform".into(),
            default_mount: "kv-platform".into(),
            auth: VaultAuthSettings::AppRole,
            tls_skip_verify: true,
            managed_by_team_id: Some("t-42".into()),
            team_auto_apply: false,
        };
        let json = serde_json::to_string(&s).unwrap();
        let r: VaultSettings = serde_json::from_str(&json).unwrap();
        assert!(r.enabled);
        assert_eq!(r.address, "https://vault.corp:8200");
        assert_eq!(r.namespace, "team/platform");
        assert_eq!(r.default_mount, "kv-platform");
        assert_eq!(r.auth, VaultAuthSettings::AppRole);
        assert!(r.tls_skip_verify);
        assert_eq!(r.managed_by_team_id.as_deref(), Some("t-42"));
        assert!(!r.team_auto_apply);
    }

    #[test]
    fn vault_settings_partial_json_applies_default_mount_and_auto_apply() {
        // Only set the address; default_mount and team_auto_apply use
        // `#[serde(default = "...")]` attributes which should be respected.
        let s: VaultSettings =
            serde_json::from_str(r#"{"address":"http://127.0.0.1:8200"}"#).unwrap();
        assert_eq!(s.address, "http://127.0.0.1:8200");
        assert_eq!(s.default_mount, "secret");
        assert!(s.team_auto_apply);
        assert_eq!(s.auth, VaultAuthSettings::None);
    }
}

// ---- hashicorp submodule tests (kept here because VaultSettings is mod-level)

#[cfg(test)]
mod hashicorp_constructor_tests {
    use super::*;
    use crate::core::vault::HashiCorpVaultClient;
    use crate::core::vault::VaultError;

    #[test]
    fn new_without_address_returns_no_address_error() {
        let settings = VaultSettings {
            address: String::new(),
            ..Default::default()
        };
        let err = HashiCorpVaultClient::new(settings).unwrap_err();
        // Exact display expected: "Vault 未配置地址"
        let msg = err.to_string();
        assert!(msg.contains("未配置地址") || msg.contains("NoAddress"),
                "unexpected err msg: {msg}");
    }

    #[test]
    fn new_with_address_proceeds_to_http_builder() {
        // Even with an invalid TLS address, the *constructor* only validates
        // the address is non-empty and builds the reqwest client; auth is
        // NOT checked here (that's resolve_auth / token). So build should
        // succeed on any reachable-looking string.
        let settings = VaultSettings {
            enabled: true,
            address: "https://127.0.0.1:1".into(), // unlikely to be listening, but OK
            ..Default::default()
        };
        let res = HashiCorpVaultClient::new(settings);
        // If reqwest build succeeds, `new` is fine. On weird CI environments
        // where building a blocking client fails, still assert the error is
        // HTTP-level (not our validation) so we've validated the code path
        // through new() that we wanted to test.
        match res {
            Ok(_) => {}
            Err(VaultError::Http(_)) => {}
            Err(other) => panic!("unexpected new() error variant: {other:?}"),
        }
    }

    #[test]
    fn vault_error_display_variants() {
        // Just confirm the thiserror-derived messages don't crash.
        let cases: Vec<(VaultError, &str)> = vec![
            (VaultError::NoAddress, "未配置地址"),
            (VaultError::NoAuth, "未配置 Vault 认证"),
            (VaultError::FieldMissing("pass".into()), "pass"),
            (VaultError::Api("boom".into()), "boom"),
            (VaultError::Http("net".into()), "net"),
            (VaultError::Keyring("kr".into()), "kr"),
        ];
        for (err, needle) in cases {
            let msg = err.to_string();
            assert!(
                msg.contains(needle),
                "expected {msg:?} to contain {needle:?}"
            );
        }
    }
}
