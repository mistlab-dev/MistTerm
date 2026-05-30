//! Unit tests for secret_resolver
//!
//! Tests secret resolver error types and temporary file handling.

use mistterm::core::secret_resolver::*;

#[test]
fn resolve_error_message() {
    let err = ResolveError::Message("test error".into());
    assert!(err.to_string().contains("test error"));
}

#[test]
fn resolve_error_debug() {
    let err = ResolveError::Message("debug error".into());
    let debug_str = format!("{:?}", err);
    assert!(debug_str.contains("Message"));
}

#[test]
fn temp_key_file_path() {
    let pem_content = "-----BEGIN OPENSSH PRIVATE KEY-----\ntest\n-----END OPENSSH PRIVATE KEY-----";
    let result = TempKeyFile::write_pem(pem_content);

    if result.is_ok() {
        let key_file = result.unwrap();
        assert!(key_file.path().to_string_lossy().ends_with(".pem"));
    }
}

#[test]
fn temp_key_file_write_and_read() {
    let pem_content = "-----BEGIN OPENSSH PRIVATE KEY-----\ntest content\n-----END OPENSSH PRIVATE KEY-----";
    let key_file = TempKeyFile::write_pem(pem_content).unwrap();

    let path = key_file.path();
    assert!(path.exists());
    drop(key_file);
}

#[test]
fn temp_key_file_drop_removes_file() {
    let pem_content = "-----BEGIN OPENSSH PRIVATE KEY-----\ntest\n-----END OPENSSH PRIVATE KEY-----";
    let key_file = TempKeyFile::write_pem(pem_content).unwrap();
    let path = key_file.path().clone();

    drop(key_file);
    assert!(!path.exists());
}

#[test]
fn resolved_ssh_secrets_fields() {
    let secrets = ResolvedSshSecrets {
        password: "secret123".into(),
        private_key_path: "/path/to/key".into(),
        temp_key_file: None,
    };

    assert_eq!(secrets.password, "secret123");
    assert_eq!(secrets.private_key_path, "/path/to/key");
    assert!(secrets.temp_key_file.is_none());
}

#[test]
fn resolved_ssh_secrets_with_temp_key() {
    let pem_content = "-----BEGIN OPENSSH PRIVATE KEY-----\ntest\n-----END OPENSSH PRIVATE KEY-----";
    let temp_key = TempKeyFile::write_pem(pem_content).ok();

    let secrets = ResolvedSshSecrets {
        password: String::new(),
        private_key_path: "/path/to/key".into(),
        temp_key_file: temp_key,
    };

    assert!(secrets.temp_key_file.is_some());
}