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
        member_stats_from_server: false,
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

#[test]
fn suggest_compliant_after_rm_rf_prefers_team_cleanup() {
    use mistterm::core::FragmentStats;

    let mut team = FragmentStats::new(
        "t1".into(),
        "清理日志标准流程".into(),
        "find /var/log -name '*.log' -mtime +7 -delete".into(),
        "ops".into(),
    );
    team.usage_count = 12;
    team.tags = vec!["disk".into(), "log".into()];

    let personal = FragmentStats::new(
        "p1".into(),
        "清理日志标准流程".into(),
        "truncate -s 0 /var/log/app.log".into(),
        "ops".into(),
    );

    let hit = suggest_compliant_after_block("rm -rf /", &[team.clone()], &[personal])
        .expect("should suggest cleanup snippet");
    assert_eq!(hit.source, "team");
    assert_eq!(hit.fragment.id, "t1");
}

#[test]
fn suggest_compliant_skips_same_danger_command() {
    use mistterm::core::FragmentStats;

    let bad = FragmentStats::new(
        "bad".into(),
        "force wipe".into(),
        "rm -rf /".into(),
        "ops".into(),
    );
    let good = FragmentStats::new(
        "good".into(),
        "磁盘清理".into(),
        "du -sh /var/log/*".into(),
        "ops".into(),
    );
    let hit = suggest_compliant_after_block("rm -rf /var", &[bad, good.clone()], &[])
        .expect("safe alternative");
    assert_eq!(hit.fragment.id, "good");
}

#[test]
fn suggest_compliant_none_without_library() {
    assert!(suggest_compliant_after_block("rm -rf /", &[], &[]).is_none());
}

#[test]
fn suggest_compliant_env_filter_prefers_tagged_then_fallback() {
    use mistterm::core::{suggest_compliant_after_block_with_env, FragmentStats, SuggestionEnvContext};

    let mut prod = FragmentStats::new(
        "prod".into(),
        "生产清理日志".into(),
        "find /var/log -name '*.log' -mtime +7 -delete".into(),
        "ops".into(),
    );
    prod.tags = vec!["prod".into(), "log".into()];

    let mut staging = FragmentStats::new(
        "stg".into(),
        "预发清理".into(),
        "truncate -s 0 /var/log/app.log".into(),
        "ops".into(),
    );
    staging.tags = vec!["staging".into(), "log".into()];

    let env = SuggestionEnvContext::from_session("db.prod.example", "red", &["prod".into()]);
    let hit = suggest_compliant_after_block_with_env(
        "rm -rf /var/log",
        &[prod.clone(), staging.clone()],
        &[],
        Some(&env),
    )
    .expect("tagged hit");
    assert_eq!(hit.fragment.id, "prod");

    // 无匹配标签时回退全局（仍能命中 cleanup）
    let env_other = SuggestionEnvContext::from_session("other", "", &["canary".into()]);
    let hit2 = suggest_compliant_after_block_with_env(
        "rm -rf /",
        &[prod, staging],
        &[],
        Some(&env_other),
    );
    assert!(hit2.is_some());
}

#[test]
fn candidate_from_failed_skips_library_and_trivial() {
    use mistterm::core::{candidate_from_failed_command, FragmentStats};

    assert!(candidate_from_failed_command("ls", &[]).is_none());
    let personal = FragmentStats::new(
        "p".into(),
        "t".into(),
        "systemctl restart nginx".into(),
        "ops".into(),
    );
    assert!(candidate_from_failed_command("systemctl restart nginx", &[personal]).is_none());
    let c = candidate_from_failed_command("systemctl restart redis", &[]).unwrap();
    assert_eq!(c.reason.as_str(), "failed_path");
}