//! audit 单元测试
//!
//! 测试审计事件的创建和结构。

use mistterm::core::audit::{AuditCategory, AuditEvent, AuditOutcome};
use serde_json::Value;

#[test]
fn audit_event_new() {
    let event = AuditEvent::new(AuditCategory::Auth, "login", AuditOutcome::Success);

    assert!(!event.ts.is_empty());
    assert!(!event.event_id.is_empty());
    assert_eq!(event.category, AuditCategory::Auth);
    assert_eq!(event.action, "login");
    assert_eq!(event.outcome, AuditOutcome::Success);
    assert!(event.session_id.is_none());
    assert!(event.host.is_none());
    assert!(event.resource.is_none());
    assert_eq!(event.detail, Value::Null);
}

#[test]
fn audit_event_with_session() {
    let event = AuditEvent::new(AuditCategory::Session, "connect", AuditOutcome::Success)
        .with_session("session-123");

    assert_eq!(event.session_id, Some("session-123".into()));
}

#[test]
fn audit_event_with_host() {
    let event = AuditEvent::new(AuditCategory::Session, "connect", AuditOutcome::Success)
        .with_host("192.168.1.1");

    assert_eq!(event.host, Some("192.168.1.1".into()));
}

#[test]
fn audit_event_with_resource() {
    let event = AuditEvent::new(AuditCategory::Credential, "read", AuditOutcome::Success)
        .with_resource("credential-id-456");

    assert_eq!(event.resource, Some("credential-id-456".into()));
}

#[test]
fn audit_event_with_detail() {
    let detail = serde_json::json!({"key": "value", "count": 42});
    let event = AuditEvent::new(AuditCategory::Config, "update", AuditOutcome::Success)
        .with_detail(detail.clone());

    assert_eq!(event.detail, detail);
}

#[test]
fn audit_event_chain_all_methods() {
    let event = AuditEvent::new(AuditCategory::Command, "execute", AuditOutcome::Success)
        .with_session("sess-1")
        .with_host("10.0.0.1")
        .with_resource("cmd-1")
        .with_detail(serde_json::json!({"status": "ok"}));

    assert_eq!(event.session_id, Some("sess-1".into()));
    assert_eq!(event.host, Some("10.0.0.1".into()));
    assert_eq!(event.resource, Some("cmd-1".into()));
    assert_eq!(event.detail["status"], "ok");
}

#[test]
fn audit_category_serde() {
    let categories = vec![
        AuditCategory::Auth,
        AuditCategory::Session,
        AuditCategory::Credential,
        AuditCategory::Vault,
        AuditCategory::Config,
        AuditCategory::Fragment,
        AuditCategory::Command,
    ];

    for cat in categories {
        let json = serde_json::to_string(&cat).unwrap();
        let deserialized: AuditCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(cat, deserialized);
    }
}

#[test]
fn audit_outcome_serde() {
    let outcomes = vec![
        AuditOutcome::Success,
        AuditOutcome::Failure,
        AuditOutcome::Denied,
    ];

    for outcome in outcomes {
        let json = serde_json::to_string(&outcome).unwrap();
        let deserialized: AuditOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(outcome, deserialized);
    }
}

#[test]
fn audit_event_has_timestamp_format() {
    let event = AuditEvent::new(AuditCategory::Auth, "login", AuditOutcome::Success);
    assert!(event.ts.contains("T"));
    assert!(event.ts.contains("Z"));
}