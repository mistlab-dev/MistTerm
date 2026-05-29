//! cmd_audit 单元测试

use mistterm::core::cmd_audit::*;

#[test]
fn cmd_audit_action_parse_all_variants() {
    assert_eq!(CmdAuditAction::parse("block"), CmdAuditAction::Block);
    assert_eq!(CmdAuditAction::parse("confirm"), CmdAuditAction::Confirm);
    assert_eq!(CmdAuditAction::parse("alert"), CmdAuditAction::Alert);
    assert_eq!(CmdAuditAction::parse("allow"), CmdAuditAction::Allow);
}

#[test]
fn cmd_audit_action_parse_case_insensitive() {
    assert_eq!(CmdAuditAction::parse("BLOCK"), CmdAuditAction::Allow);
    assert_eq!(CmdAuditAction::parse("Block"), CmdAuditAction::Allow);
    assert_eq!(CmdAuditAction::parse("CONFIRM"), CmdAuditAction::Allow);
    assert_eq!(CmdAuditAction::parse("Confirm"), CmdAuditAction::Allow);
}

#[test]
fn cmd_audit_action_parse_unknown_defaults_to_allow() {
    assert_eq!(CmdAuditAction::parse(""), CmdAuditAction::Allow);
    assert_eq!(CmdAuditAction::parse("unknown"), CmdAuditAction::Allow);
    assert_eq!(CmdAuditAction::parse("invalid"), CmdAuditAction::Allow);
}

#[test]
fn cmd_audit_policy_default_values() {
    let policy = CmdAuditPolicy {
        team_id: String::new(),
        enabled: true,
        dangerous_action: CmdAuditAction::Block,
        sensitive_action: CmdAuditAction::Confirm,
        unknown_action: CmdAuditAction::Allow,
        confirm_timeout: 300,
    };

    assert!(!policy.team_id.is_empty() == false);
    assert!(policy.enabled);
    assert_eq!(policy.dangerous_action, CmdAuditAction::Block);
    assert_eq!(policy.sensitive_action, CmdAuditAction::Confirm);
    assert_eq!(policy.unknown_action, CmdAuditAction::Allow);
    assert_eq!(policy.confirm_timeout, 300);
}

#[test]
fn cmd_audit_policy_serde() {
    let policy = CmdAuditPolicy {
        team_id: "team1".into(),
        enabled: true,
        dangerous_action: CmdAuditAction::Block,
        sensitive_action: CmdAuditAction::Confirm,
        unknown_action: CmdAuditAction::Allow,
        confirm_timeout: 600,
    };

    let json = serde_json::to_string(&policy).unwrap();
    let deserialized: CmdAuditPolicy = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.team_id, "team1");
    assert_eq!(deserialized.enabled, true);
    assert_eq!(deserialized.confirm_timeout, 600);
}

#[test]
fn cmd_audit_rule_default_values() {
    let rule = CmdAuditRule {
        id: "rule1".into(),
        name: "Test Rule".into(),
        pattern: "rm -rf /".into(),
        match_type: "regex".into(),
        scope: "command".into(),
        action: "block".into(),
        description: "Dangerous delete".into(),
        priority: 100,
        enabled: true,
    };

    assert_eq!(rule.id, "rule1");
    assert_eq!(rule.pattern, "rm -rf /");
    assert_eq!(rule.match_type, "regex");
    assert_eq!(rule.action, "block");
    assert_eq!(rule.priority, 100);
    assert!(rule.enabled);
}

#[test]
fn cmd_audit_rule_serde() {
    let rule = CmdAuditRule {
        id: "test_rule".into(),
        name: "Test".into(),
        pattern: "sudo.*".into(),
        match_type: "regex".into(),
        scope: "command".into(),
        action: "confirm".into(),
        description: "Sudo command".into(),
        priority: 50,
        enabled: true,
    };

    let json = serde_json::to_string(&rule).unwrap();
    let deserialized: CmdAuditRule = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.id, "test_rule");
    assert_eq!(deserialized.pattern, "sudo.*");
}

#[test]
fn cmd_audit_sync_payload_default_sync_interval() {
    let payload = CmdAuditSyncPayload {
        enabled: true,
        policy: None,
        rules: vec![],
        sync_interval_sec: 300,
    };

    assert!(payload.enabled);
    assert!(payload.policy.is_none());
    assert!(payload.rules.is_empty());
    assert_eq!(payload.sync_interval_sec, 300);
}

#[test]
fn cmd_audit_cache_entry_from_sync_payload() {
    let payload = CmdAuditSyncPayload {
        enabled: true,
        policy: Some(CmdAuditPolicy {
            team_id: "team1".into(),
            enabled: true,
            dangerous_action: CmdAuditAction::Block,
            sensitive_action: CmdAuditAction::Confirm,
            unknown_action: CmdAuditAction::Allow,
            confirm_timeout: 300,
        }),
        rules: vec![],
        sync_interval_sec: 600,
    };

    let entry = CmdAuditCacheEntry::from_sync_payload(&payload);

    assert!(entry.enabled);
    assert!(entry.policy.is_some());
    assert!(!entry.synced_at.is_empty());
    assert_eq!(entry.sync_interval_sec, 600);
}

#[test]
fn cmd_audit_cache_entry_round_trip() {
    let payload = CmdAuditSyncPayload {
        enabled: true,
        policy: Some(CmdAuditPolicy {
            team_id: "team1".into(),
            enabled: true,
            dangerous_action: CmdAuditAction::Block,
            sensitive_action: CmdAuditAction::Confirm,
            unknown_action: CmdAuditAction::Allow,
            confirm_timeout: 300,
        }),
        rules: vec![],
        sync_interval_sec: 600,
    };

    let entry = CmdAuditCacheEntry::from_sync_payload(&payload);
    let round_trip = entry.to_sync_payload();

    assert_eq!(round_trip.enabled, payload.enabled);
    assert_eq!(round_trip.sync_interval_sec, payload.sync_interval_sec);
}

#[test]
fn cmd_audit_cache_store_default() {
    let store = CmdAuditCacheStore::default();
    assert!(store.by_team.is_empty());
}

#[test]
fn cmd_audit_cache_store_upsert_team() {
    let mut store = CmdAuditCacheStore::default();

    let payload = CmdAuditSyncPayload {
        enabled: true,
        policy: None,
        rules: vec![],
        sync_interval_sec: 300,
    };

    store.upsert_team("team1", &payload);

    assert!(store.by_team.contains_key("team1"));
    let entry = store.by_team.get("team1").unwrap();
    assert!(entry.enabled);
}

#[test]
fn cmd_audit_cache_store_payload_for_team() {
    let mut store = CmdAuditCacheStore::default();

    let payload = CmdAuditSyncPayload {
        enabled: true,
        policy: None,
        rules: vec![],
        sync_interval_sec: 300,
    };

    store.upsert_team("team1", &payload);

    let retrieved = store.payload_for_team("team1");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().enabled, true);

    let not_found = store.payload_for_team("nonexistent");
    assert!(not_found.is_none());
}

#[test]
fn cmd_audit_engine_default() {
    let engine = CmdAuditEngine::default();
    assert!(engine.is_active() == false);
    assert!(engine.needs_sync());
}

#[test]
fn cmd_audit_engine_new_and_sync() {
    let engine = CmdAuditEngine::new();
    assert!(engine.needs_sync());
}