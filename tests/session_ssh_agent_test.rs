//! SessionConfig SSH Agent 字段：序列化兼容与默认值。

use mistterm::core::SessionConfig;

#[test]
fn use_ssh_agent_defaults_true() {
    assert!(SessionConfig::default().use_ssh_agent);
}

#[test]
fn use_ssh_agent_legacy_json_without_field_defaults_true() {
    let json = r#"{
        "id": "s1",
        "name": "prod",
        "group": "默认",
        "host": "10.0.0.1",
        "port": 22,
        "username": "root",
        "password": "",
        "private_key_path": ""
    }"#;
    let s: SessionConfig = serde_json::from_str(json).expect("parse legacy session");
    assert!(s.use_ssh_agent);
}

#[test]
fn auth_mode_defaults_to_standard_and_roundtrips_fido2() {
    use mistterm::core::SessionAuthMode;
    assert_eq!(
        SessionConfig::default().auth_mode,
        SessionAuthMode::Standard
    );
    let json = r#"{
        "id": "s1",
        "name": "prod",
        "group": "默认",
        "host": "10.0.0.1",
        "port": 22,
        "username": "root",
        "password": "",
        "private_key_path": ""
    }"#;
    let legacy: SessionConfig = serde_json::from_str(json).unwrap();
    assert_eq!(legacy.auth_mode, SessionAuthMode::Standard);
    assert!(legacy.use_ssh_agent);

    let mut s = SessionConfig::default();
    s.auth_mode = SessionAuthMode::Fido2SecurityKey;
    let saved = serde_json::to_string(&s).unwrap();
    assert!(saved.contains("fido2_security_key"));
    let back: SessionConfig = serde_json::from_str(&saved).unwrap();
    assert_eq!(back.auth_mode, SessionAuthMode::Fido2SecurityKey);
    assert!(back.use_ssh_agent, "FIDO2 does not clear the plain agent flag");
}

#[test]
fn use_ssh_agent_roundtrip_json() {
    let mut s = SessionConfig::default();
    s.use_ssh_agent = false;
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("use_ssh_agent"));
    let back: SessionConfig = serde_json::from_str(&json).unwrap();
    assert!(!back.use_ssh_agent);
}

#[test]
fn stored_sessions_keep_fido2_auth_mode_and_legacy_stays_standard() {
    use mistterm::core::{SessionAuthMode, SessionManager};
    let json = r#"[{
        "id": "x",
        "name": "sk",
        "host": "h",
        "port": 22,
        "username": "u",
        "auth_mode": "fido2_security_key"
    }]"#;
    let (sessions, _, _) =
        SessionManager::parse_stored_sessions_json(&[0u8; 32], json).expect("stored fido2");
    assert_eq!(sessions[0].auth_mode, SessionAuthMode::Fido2SecurityKey);

    let legacy = r#"[{
        "id": "y",
        "name": "old",
        "host": "h",
        "port": 22,
        "username": "u"
    }]"#;
    let (old, _, _) =
        SessionManager::parse_stored_sessions_json(&[0u8; 32], legacy).expect("stored legacy");
    assert_eq!(old[0].auth_mode, SessionAuthMode::Standard);
    assert!(old[0].use_ssh_agent);
}

#[test]
fn stored_session_json_without_use_ssh_agent_loads_as_true() {
    // 模拟 sessions.json 内层条目（StoredSessionConfig 字段子集）
    let json = r#"[
        {
            "id": "x",
            "name": "hop",
            "group": "默认",
            "host": "bastion",
            "port": 22,
            "username": "admin",
            "password": "",
            "private_key_path": "/home/.ssh/id_ed25519"
        }
    ]"#;
    #[derive(serde::Deserialize)]
    struct Row {
        #[serde(default = "default_true")]
        use_ssh_agent: bool,
        private_key_path: String,
    }
    fn default_true() -> bool {
        true
    }
    let rows: Vec<Row> = serde_json::from_str(json).unwrap();
    assert!(rows[0].use_ssh_agent);
    assert_eq!(rows[0].private_key_path, "/home/.ssh/id_ed25519");
}
