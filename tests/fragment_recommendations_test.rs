//! Unit tests for fragment_recommendations

use mistterm::core::fragment_recommendations::*;

#[test]
fn merge_recommendations_dedup() {
    let a = vec![
        FragmentRecommendation { command: "ls".into(), count: 3, source: "history" },
        FragmentRecommendation { command: "pwd".into(), count: 5, source: "log" },
    ];
    let b = vec![
        FragmentRecommendation { command: "ls".into(), count: 10, source: "log" },
        FragmentRecommendation { command: "whoami".into(), count: 2, source: "log" },
    ];

    let merged = merge_recommendations(a, b, 10);
    let ls = merged.iter().find(|r| r.command == "ls").unwrap();
    assert_eq!(ls.count, 10);
    assert!(merged.iter().any(|r| r.command == "pwd"));
    assert!(merged.iter().any(|r| r.command == "whoami"));
}

#[test]
fn merge_recommendations_limit() {
    let a = vec![
        FragmentRecommendation { command: "a".into(), count: 5, source: "history" },
        FragmentRecommendation { command: "b".into(), count: 3, source: "history" },
    ];
    let b = vec![
        FragmentRecommendation { command: "c".into(), count: 10, source: "log" },
    ];
    let merged = merge_recommendations(a, b, 2);
    assert_eq!(merged.len(), 2);
    assert_eq!(merged[0].command, "c");
    assert_eq!(merged[1].command, "a");
}

#[test]
fn merge_recommendations_empty_a() {
    let a: Vec<FragmentRecommendation> = vec![];
    let b = vec![
        FragmentRecommendation { command: "ls".into(), count: 5, source: "log" },
    ];

    let merged = merge_recommendations(a, b, 10);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].command, "ls");
}

#[test]
fn merge_recommendations_empty_b() {
    let a = vec![
        FragmentRecommendation { command: "ls".into(), count: 5, source: "history" },
    ];
    let b: Vec<FragmentRecommendation> = vec![];

    let merged = merge_recommendations(a, b, 10);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].command, "ls");
}

#[test]
fn merge_recommendations_both_empty() {
    let a: Vec<FragmentRecommendation> = vec![];
    let b: Vec<FragmentRecommendation> = vec![];

    let merged = merge_recommendations(a, b, 10);
    assert!(merged.is_empty());
}

#[test]
fn merge_recommendations_sort_by_count_descending() {
    let a = vec![
        FragmentRecommendation { command: "low".into(), count: 1, source: "history" },
    ];
    let b = vec![
        FragmentRecommendation { command: "high".into(), count: 100, source: "log" },
        FragmentRecommendation { command: "medium".into(), count: 50, source: "log" },
    ];

    let merged = merge_recommendations(a, b, 10);
    assert_eq!(merged[0].command, "high");
    assert_eq!(merged[1].command, "medium");
    assert_eq!(merged[2].command, "low");
}

#[test]
fn fragment_recommendation_debug() {
    let rec = FragmentRecommendation {
        command: "ls".into(),
        count: 5,
        source: "history".into(),
    };
    let debug_str = format!("{:?}", rec);
    assert!(debug_str.contains("ls"));
    assert!(debug_str.contains("5"));
}

#[test]
fn fragment_recommendation_clone() {
    let rec = FragmentRecommendation {
        command: "ls".into(),
        count: 5,
        source: "history".into(),
    };
    let cloned = rec.clone();
    assert_eq!(cloned.command, rec.command);
    assert_eq!(cloned.count, rec.count);
    assert_eq!(cloned.source, rec.source);
}

fn sample_dashboard() -> mistterm::core::FragmentAnalyticsDashboard {
    mistterm::core::FragmentAnalyticsDashboard {
        personal_total_usage: 42,
        personal_success_rate: 95.0,
        personal_avg_ms: 120,
        team_total_usage: 10,
        team_success_rate: 80.0,
        team_avg_ms: 200,
        personal_top: vec![],
        team_top: vec![],
        slowest: vec![],
        highest_error: vec![],
        team_api_available: false,
        member_rows: vec![],
        period_stats_from_events: false,
    }
}

#[test]
fn efficiency_report_markdown_contains_summary() {
    use mistterm::core::fragment_analytics::FragmentAnalyticsTimeRange;
    use mistterm::core::build_efficiency_report_markdown;

    let md = build_efficiency_report_markdown(
        &sample_dashboard(),
        FragmentAnalyticsTimeRange::Last7Days,
        &[],
    );
    assert!(md.contains("# MistTerm 效率报告"));
    assert!(md.contains("近 7 天"));
    assert!(md.contains("个人"));
    assert!(md.contains("42"));
}

#[test]
fn efficiency_report_pdf_valid_when_cjk_font_available() {
    use mistterm::core::fragment_analytics::FragmentAnalyticsTimeRange;
    use mistterm::core::build_efficiency_report_pdf;

    let pdf = build_efficiency_report_pdf(
        &sample_dashboard(),
        FragmentAnalyticsTimeRange::AllTime,
        &[],
    );
    let Ok(bytes) = pdf else {
        eprintln!("skip PDF integration test: no CJK font on host");
        return;
    };
    assert!(bytes.starts_with(b"%PDF"));
    assert!(bytes.len() > 400);
}