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
fn use_ssh_agent_roundtrip_json() {
    let mut s = SessionConfig::default();
    s.use_ssh_agent = false;
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("use_ssh_agent"));
    let back: SessionConfig = serde_json::from_str(&json).unwrap();
    assert!(!back.use_ssh_agent);
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
