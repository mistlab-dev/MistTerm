//! Unit tests for credential
//!
//! Tests credential library types, categories, and backends.

use mistterm::core::credential::{
    CredentialAuthKind, CredentialCategory, SecretBackend,
};

#[test]
fn credential_category_labels() {
    assert_eq!(CredentialCategory::Server.label_zh(), "服务器账号");
    assert_eq!(CredentialCategory::Database.label_zh(), "数据库");
    assert_eq!(CredentialCategory::SshKey.label_zh(), "SSH 密钥");
    assert_eq!(CredentialCategory::Api.label_zh(), "API / 令牌");
    assert_eq!(CredentialCategory::Other.label_zh(), "其他");
}

#[test]
fn credential_category_all_variants() {
    let categories = vec![
        CredentialCategory::Server,
        CredentialCategory::Database,
        CredentialCategory::SshKey,
        CredentialCategory::Api,
        CredentialCategory::Other,
    ];

    for cat in categories {
        assert!(!cat.label_zh().is_empty());
    }
}

#[test]
fn secret_backend_default() {
    let backend = SecretBackend::default();
    assert!(matches!(backend, SecretBackend::LocalEncrypted));
}

#[test]
fn secret_backend_is_vault() {
    let local = SecretBackend::LocalEncrypted;
    assert!(!local.is_vault());

    let vault = SecretBackend::VaultKv {
        mount: "secret".into(),
        path: "myapp/prod".into(),
        field: "password".into(),
        version: Some(1),
    };
    assert!(vault.is_vault());
}

#[test]
fn secret_backend_vault_kv_serde() {
    let vault = SecretBackend::VaultKv {
        mount: "secret".into(),
        path: "myapp/prod".into(),
        field: "password".into(),
        version: Some(1),
    };

    let json = serde_json::to_string(&vault).unwrap();
    assert!(json.contains("vault_kv"));
    let deserialized: SecretBackend = serde_json::from_str(&json).unwrap();
    assert!(deserialized.is_vault());
}

#[test]
fn secret_backend_local_encrypted_serde() {
    let local = SecretBackend::LocalEncrypted;

    let json = serde_json::to_string(&local).unwrap();
    assert!(json.contains("local"));
    let deserialized: SecretBackend = serde_json::from_str(&json).unwrap();
    assert!(!deserialized.is_vault());
}

#[test]
fn credential_auth_kind_labels() {
    assert_eq!(CredentialAuthKind::Password.label_zh(), "密码");
    assert_eq!(CredentialAuthKind::SshKey.label_zh(), "SSH 密钥");
    assert_eq!(CredentialAuthKind::Token.label_zh(), "令牌 / API Key");
}

#[test]
fn credential_auth_kind_default() {
    let auth = CredentialAuthKind::default();
    assert!(matches!(auth, CredentialAuthKind::Password));
}

#[test]
fn credential_auth_kind_all_variants() {
    let kinds = vec![
        CredentialAuthKind::Password,
        CredentialAuthKind::SshKey,
        CredentialAuthKind::Token,
    ];

    for kind in kinds {
        assert!(!kind.label_zh().is_empty());
    }
}

#[test]
fn credential_auth_kind_serde() {
    let kinds = vec![
        CredentialAuthKind::Password,
        CredentialAuthKind::SshKey,
        CredentialAuthKind::Token,
    ];

    for kind in kinds {
        let json = serde_json::to_string(&kind).unwrap();
        let deserialized: CredentialAuthKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, deserialized);
    }
}

#[test]
fn credential_category_serde() {
    let categories = vec![
        CredentialCategory::Server,
        CredentialCategory::Database,
        CredentialCategory::SshKey,
        CredentialCategory::Api,
        CredentialCategory::Other,
    ];

    for cat in categories {
        let json = serde_json::to_string(&cat).unwrap();
        let deserialized: CredentialCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(cat, deserialized);
    }
}