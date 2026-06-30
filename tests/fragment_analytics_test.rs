//! Unit tests for fragment_analytics (public API only)

use mistterm::core::FragmentStats;
use mistterm::core::fragment_analytics::FragmentAnalyticsTimeRange as FR;

fn make_fragment_stats(id: &str, command: &str, last_used: Option<i64>) -> FragmentStats {
    FragmentStats {
        id: id.into(),
        title: command.into(),
        command: command.into(),
        category: "test".into(),
        tags: vec![],
        variables: vec![],
        usage_count: 1,
        success_count: 1,
        total_time_ms: 100,
        last_used,
        source_status: String::new(),
    }
}

#[test]
fn fragment_stats_new_works() {
    let s = make_fragment_stats("1", "ls", None);
    assert_eq!(s.id, "1");
    assert_eq!(s.command, "ls");
}

#[test]
fn fragment_analytics_time_range_all_time_no_cutoff() {
    assert_eq!(FR::AllTime.cutoff_unix(), None);
}

#[test]
fn fragment_analytics_time_range_label_en() {
    assert_eq!(FR::AllTime.label_en(), "All time");
    assert_eq!(FR::Last7Days.label_en(), "Last 7 days");
    assert_eq!(FR::Last30Days.label_en(), "Last 30 days");
    assert_eq!(FR::Last90Days.label_en(), "Last 90 days");
}

#[test]
fn fragment_analytics_filter_fragments() {
    let now = chrono::Utc::now().timestamp();
    let items = vec![
        make_fragment_stats("1", "ls", Some(now)),
        make_fragment_stats("2", "pwd", Some(1_000_000)),
    ];

    let filtered = FR::Last7Days.filter_fragments(&items);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "1");
}

#[test]
fn fragment_analytics_time_range_last_7_days_cutoff() {
    let cutoff = FR::Last7Days.cutoff_unix();
    assert!(cutoff.is_some());
    let now = chrono::Utc::now().timestamp();
    assert!(cutoff.unwrap() <= now);
    assert!(cutoff.unwrap() > now - 8 * 24 * 3600);
}

#[test]
fn fragment_analytics_time_range_last_30_days_cutoff() {
    let cutoff = FR::Last30Days.cutoff_unix();
    assert!(cutoff.is_some());
    let now = chrono::Utc::now().timestamp();
    assert!(cutoff.unwrap() <= now);
    assert!(cutoff.unwrap() > now - 31 * 24 * 3600);
}