//! command_history 单元测试

use mistterm::core::command_history::*;

#[test]
fn history_entry_executed_at_local() {
    let entry = HistoryEntry {
        command: "ls".into(),
        executed_at: 0,
        session_id: None,
        session_name: None,
        success: true,
    };

    let local = entry.executed_at_local();
    assert!(local.is_some());
}

#[test]
fn history_entry_display_command_short() {
    let entry = HistoryEntry {
        command: "ls -la".into(),
        executed_at: 0,
        session_id: None,
        session_name: None,
        success: true,
    };

    assert_eq!(entry.display_command(), "ls -la");
}

#[test]
fn history_entry_display_command_truncated() {
    let long_command = "a".repeat(501);
    let entry = HistoryEntry {
        command: long_command.clone(),
        executed_at: 0,
        session_id: None,
        session_name: None,
        success: true,
    };

    let display = entry.display_command();
    assert!(display.ends_with('…'));
}

#[test]
fn history_entry_display_command_exact_max() {
    let exact_command = "a".repeat(500);
    let entry = HistoryEntry {
        command: exact_command.clone(),
        executed_at: 0,
        session_id: None,
        session_name: None,
        success: true,
    };

    assert_eq!(entry.display_command(), exact_command);
}

#[test]
fn history_entry_debug() {
    let entry = HistoryEntry {
        command: "ls".into(),
        executed_at: 1000,
        session_id: Some("sess1".into()),
        session_name: Some("prod".into()),
        success: true,
    };

    let debug_str = format!("{:?}", entry);
    assert!(debug_str.contains("ls"));
    assert!(debug_str.contains("sess1"));
}

#[test]
fn history_entry_clone() {
    let entry = HistoryEntry {
        command: "ls".into(),
        executed_at: 1000,
        session_id: None,
        session_name: None,
        success: true,
    };

    let cloned = entry.clone();
    assert_eq!(cloned.command, entry.command);
    assert_eq!(cloned.executed_at, entry.executed_at);
    assert_eq!(cloned.success, entry.success);
}