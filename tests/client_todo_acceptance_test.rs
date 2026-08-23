//! CLIENT-TODO 文档场景的可自动化验收（无需实机 SSH / agent）。

use mistterm::core::cmd_audit::{CmdAuditAction, CmdAuditEngine, CmdAuditPolicy, CmdAuditSyncPayload, ServerAuditProbe};
use mistterm::core::team::{
    cmd_audit_agent_available_for_host, cmd_audit_host_matches, CmdAuditAgent, StorageUsageResponse,
};

/// §5：本地 `rm -rf /` → 本地 block（本地检查语义由 UI 展示，引擎层验证拦截）。
#[test]
fn doc_local_rm_rf_blocked_by_engine() {
    let mut engine = CmdAuditEngine::new();
    engine.apply_sync(CmdAuditSyncPayload {
        enabled: true,
        policy: Some(CmdAuditPolicy {
            team_id: "t1".into(),
            enabled: true,
            dangerous_action: CmdAuditAction::Block,
            sensitive_action: CmdAuditAction::Confirm,
            unknown_action: CmdAuditAction::Allow,
            confirm_timeout: 300,
        }),
        rules: vec![],
        sync_interval_sec: 300,
    });
    let r = engine.check("rm -rf /");
    assert!(!r.allowed);
    assert_eq!(r.action, CmdAuditAction::Block);
}

/// §5：`ls` → 放行。
#[test]
fn doc_ls_allowed_by_engine() {
    let mut engine = CmdAuditEngine::new();
    engine.apply_sync(CmdAuditSyncPayload {
        enabled: true,
        policy: Some(CmdAuditPolicy {
            team_id: "t1".into(),
            enabled: true,
            dangerous_action: CmdAuditAction::Block,
            sensitive_action: CmdAuditAction::Confirm,
            unknown_action: CmdAuditAction::Allow,
            confirm_timeout: 300,
        }),
        rules: vec![],
        sync_interval_sec: 300,
    });
    let r = engine.check("ls");
    assert!(r.allowed);
}

/// §5：服务器 `cat /etc/shadow` → confirm + token。
#[test]
fn doc_server_confirm_from_mist_audit_line() {
    let mut probe = ServerAuditProbe::new();
    let line = br#"MIST_AUDIT	{"action":"confirm","message":"sensitive","command":"cat /etc/shadow","token":"approve-42"}"#;
    let mut buf = Vec::new();
    buf.extend_from_slice(line);
    buf.push(b'\n');
    let (_, events) = probe.feed(&buf);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].action, CmdAuditAction::Confirm);
    assert_eq!(events[0].command, "cat /etc/shadow");
    assert_eq!(events[0].token, "approve-42");
}

/// §3：agent 停止（离线）→ 对 host 不可用。
#[test]
fn doc_agent_offline_shows_degraded() {
    let now = chrono::Utc::now();
    let stale = (now - chrono::Duration::seconds(600)).to_rfc3339();
    let agents = vec![CmdAuditAgent {
        id: "a1".into(),
        host: "prod-1".into(),
        status: "active".into(),
        last_seen_at: Some(stale),
        enabled: true,
    }];
    assert!(!cmd_audit_agent_available_for_host(&agents, "prod-1", now, 300));
}

/// §3：agent 恢复在线 → 可用。
#[test]
fn doc_agent_online_available() {
    let now = chrono::Utc::now();
    let agents = vec![CmdAuditAgent {
        id: "a1".into(),
        host: "prod-1".into(),
        status: "active".into(),
        last_seen_at: Some(now.to_rfc3339()),
        enabled: true,
    }];
    assert!(cmd_audit_agent_available_for_host(&agents, "prod-1", now, 300));
    assert!(cmd_audit_host_matches("prod-1", "prod-1"));
}

/// §7：存储用量 JSON 契约。
#[test]
fn doc_storage_usage_serde() {
    let json = r#"{
        "total_bytes": 12345678,
        "quota_bytes": 1073741824,
        "fragments": {"count": 42, "bytes": 5000000},
        "recordings": {"count": 5, "bytes": 3000000},
        "documents": {"count": 10, "bytes": 4000000},
        "versions": {"count": 120, "bytes": 345678}
    }"#;
    let u: StorageUsageResponse = serde_json::from_str(json).unwrap();
    assert_eq!(u.total_bytes, 12345678);
    assert_eq!(u.fragments.count, 42);
    assert_eq!(u.quota_bytes, Some(1073741824));
}
