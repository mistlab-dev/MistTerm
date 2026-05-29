//! fragment_usage_log 单元测试

use mistterm::core::fragment_usage_log::*;
use chrono::Utc;

fn make_event(ts: i64, fragment_id: &str, success: bool) -> FragmentUsageEvent {
    FragmentUsageEvent {
        ts,
        fragment_id: fragment_id.into(),
        scope: "personal".into(),
        team_id: None,
        user_id: None,
        display_name: None,
        success,
        duration_ms: 100,
    }
}

#[test]
fn usage_log_append_and_iter() {
    let mut log = FragmentUsageLog::default();
    let now = Utc::now().timestamp();
    log.append(make_event(now, "f1", true));
    log.append(make_event(now - 10_000, "f2", false));

    assert_eq!(log.all_events().len(), 2);
}

#[test]
fn usage_log_events_since() {
    let mut log = FragmentUsageLog::default();
    let now = Utc::now().timestamp();
    log.append(make_event(now, "f1", true));
    log.append(make_event(now - 200_000, "f2", true));

    let cutoff = now - 86_400;
    let recent: Vec<_> = log.events_since(cutoff).collect();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].fragment_id, "f1");
}

#[test]
fn usage_log_all_events_empty() {
    let log: FragmentUsageLog = FragmentUsageLog::default();
    assert!(log.all_events().is_empty());
}

#[test]
fn usage_log_events_since_cutoff_filter() {
    let mut log = FragmentUsageLog::default();
    let now = Utc::now().timestamp();
    log.append(make_event(now, "f1", true));
    log.append(make_event(now - 100_000, "f2", true));

    let cutoff = now - 50_000;
    let recent: Vec<_> = log.events_since(cutoff).collect();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].fragment_id, "f1");
}