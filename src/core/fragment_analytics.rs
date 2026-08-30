//! 命令片段分析聚合（个人库 + 团队缓存/API）。

use serde::Serialize;

use crate::core::fragment_usage_log::{self, FragmentUsageEvent, MemberPeriodStats};
use crate::core::FragmentStats;
use crate::core::team::TeamMember;

/// 按 `last_used` 筛选参与聚合的片段（次数/成功率为累计值，非区间内增量）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FragmentAnalyticsTimeRange {
    #[default]
    AllTime,
    Last7Days,
    Last30Days,
    Last90Days,
}

impl FragmentAnalyticsTimeRange {
    pub fn cutoff_unix(self) -> Option<i64> {
        let days = match self {
            Self::AllTime => return None,
            Self::Last7Days => 7,
            Self::Last30Days => 30,
            Self::Last90Days => 90,
        };
        let now = chrono::Utc::now().timestamp();
        Some(now - i64::from(days) * 86_400)
    }

    pub fn filter_fragments(&self, items: &[FragmentStats]) -> Vec<FragmentStats> {
        let Some(cutoff) = self.cutoff_unix() else {
            return items.to_vec();
        };
        items
            .iter()
            .filter(|f| f.last_used.is_some_and(|t| t >= cutoff))
            .cloned()
            .collect()
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::AllTime => "All time",
            Self::Last7Days => "Last 7 days",
            Self::Last30Days => "Last 30 days",
            Self::Last90Days => "Last 90 days",
        }
    }

    pub fn label_zh(self) -> &'static str {
        match self {
            Self::AllTime => "全部时间",
            Self::Last7Days => "近 7 天",
            Self::Last30Days => "近 30 天",
            Self::Last90Days => "近 90 天",
        }
    }

    pub fn since_days(self) -> Option<u32> {
        match self {
            Self::AllTime => None,
            Self::Last7Days => Some(7),
            Self::Last30Days => Some(30),
            Self::Last90Days => Some(90),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FragmentAnalyticsDashboard {
    pub personal_total_usage: u64,
    pub personal_success_rate: f32,
    pub personal_avg_ms: u32,
    pub team_total_usage: u64,
    pub team_success_rate: f32,
    pub team_avg_ms: u32,
    pub personal_top: Vec<FragmentStats>,
    pub team_top: Vec<FragmentStats>,
    pub slowest: Vec<FragmentStats>,
    pub highest_error: Vec<FragmentStats>,
    pub team_api_available: bool,
    /// 时间范围内本机记录的团队执行（按成员）；服务端成员 API 可用时为全团队数据。
    pub member_rows: Vec<MemberPeriodStats>,
    pub period_stats_from_events: bool,
    pub member_stats_from_server: bool,
}

pub fn build_dashboard(
    personal: &[FragmentStats],
    team: &[FragmentStats],
    team_api_available: bool,
) -> FragmentAnalyticsDashboard {
    build_dashboard_inner(
        personal,
        team,
        team_api_available,
        Vec::new(),
        false,
        false,
    )
}

pub fn build_dashboard_with_events(
    personal_all: &[FragmentStats],
    team_all: &[FragmentStats],
    events: &[FragmentUsageEvent],
    range: FragmentAnalyticsTimeRange,
    team_api_available: bool,
    team_id: Option<&str>,
    members: &[TeamMember],
) -> FragmentAnalyticsDashboard {
    let Some(cutoff) = range.cutoff_unix() else {
        let personal = range.filter_fragments(personal_all);
        let team = range.filter_fragments(team_all);
        return build_dashboard_inner(&personal, &team, team_api_available, Vec::new(), false, false);
    };

    let personal = fragment_usage_log::apply_period_stats_to_fragments(
        personal_all,
        events,
        cutoff,
        "personal",
    );
    let team = fragment_usage_log::apply_period_stats_to_fragments(team_all, events, cutoff, "team");
    let member_rows = team_id
        .map(|tid| fragment_usage_log::member_stats_in_range(events, cutoff, tid, members))
        .unwrap_or_default();
    build_dashboard_inner(
        &personal,
        &team,
        team_api_available,
        member_rows,
        true,
        false,
    )
}

pub fn member_rows_from_api(
    rows: &[crate::core::team::FragmentMemberAnalyticsRow],
) -> Vec<MemberPeriodStats> {
    let mut out: Vec<MemberPeriodStats> = rows
        .iter()
        .map(|r| MemberPeriodStats {
            user_id: r.user_id.clone(),
            display_name: if r.display_name.is_empty() {
                r.user_id.clone()
            } else {
                r.display_name.clone()
            },
            run_count: r.run_count,
            success_count: r.success_count,
        })
        .collect();
    out.sort_by(|a, b| b.run_count.cmp(&a.run_count));
    out
}

fn build_dashboard_inner(
    personal: &[FragmentStats],
    team: &[FragmentStats],
    team_api_available: bool,
    member_rows: Vec<MemberPeriodStats>,
    period_stats_from_events: bool,
    member_stats_from_server: bool,
) -> FragmentAnalyticsDashboard {
    let mut dash = FragmentAnalyticsDashboard {
        team_api_available,
        member_rows,
        period_stats_from_events,
        member_stats_from_server,
        ..Default::default()
    };

    let (pu, ps, pa) = aggregate_slice(personal);
    dash.personal_total_usage = pu;
    dash.personal_success_rate = ps;
    dash.personal_avg_ms = pa;
    dash.personal_top = top_n(personal, 5);

    let (tu, ts, ta) = aggregate_slice(team);
    dash.team_total_usage = tu;
    dash.team_success_rate = ts;
    dash.team_avg_ms = ta;
    dash.team_top = top_n(team, 5);

    let mut slow_pool: Vec<FragmentStats> = personal
        .iter()
        .chain(team.iter())
        .filter(|f| f.usage_count > 0)
        .cloned()
        .collect();
    slow_pool.sort_by(|a, b| b.avg_time_ms().cmp(&a.avg_time_ms()));
    dash.slowest = slow_pool.into_iter().take(5).collect();

    let mut err_pool: Vec<FragmentStats> = personal
        .iter()
        .chain(team.iter())
        .filter(|f| f.usage_count >= 3)
        .cloned()
        .collect();
    err_pool.sort_by(|a, b| {
        let ea = 100.0 - a.success_rate();
        let eb = 100.0 - b.success_rate();
        eb.partial_cmp(&ea).unwrap_or(std::cmp::Ordering::Equal)
    });
    dash.highest_error = err_pool.into_iter().take(5).collect();

    dash
}

fn aggregate_slice(items: &[FragmentStats]) -> (u64, f32, u32) {
    let mut usage: u64 = 0;
    let mut success: u64 = 0;
    let mut total_ms: u64 = 0;
    for f in items {
        usage += f.usage_count as u64;
        success += f.success_count as u64;
        total_ms += f.total_time_ms;
    }
    let rate = if usage == 0 {
        0.0
    } else {
        (success as f32 / usage as f32) * 100.0
    };
    let avg = if usage == 0 {
        0
    } else {
        (total_ms / usage) as u32
    };
    (usage, rate, avg)
}

fn top_n(items: &[FragmentStats], n: usize) -> Vec<FragmentStats> {
    let mut v: Vec<FragmentStats> = items
        .iter()
        .filter(|f| f.usage_count > 0)
        .cloned()
        .collect();
    v.sort_by(|a, b| b.usage_count.cmp(&a.usage_count));
    v.truncate(n);
    v
}

#[derive(Serialize)]
struct DashboardExport<'a> {
    time_range: &'a str,
    exported_at: String,
    team_api_available: bool,
    personal_total_usage: u64,
    personal_success_rate: f32,
    personal_avg_ms: u32,
    team_total_usage: u64,
    team_success_rate: f32,
    team_avg_ms: u32,
    personal_top: Vec<ExportSnippetRow>,
    team_top: Vec<ExportSnippetRow>,
    slowest: Vec<ExportSnippetRow>,
    highest_error: Vec<ExportSnippetRow>,
    #[serde(default)]
    member_rows: Vec<ExportMemberRow>,
    period_stats_from_events: bool,
    #[serde(default)]
    member_stats_from_server: bool,
}

#[derive(Serialize)]
struct ExportMemberRow {
    user_id: String,
    display_name: String,
    run_count: u64,
    success_count: u64,
}

#[derive(Serialize)]
struct ExportSnippetRow {
    id: String,
    title: String,
    usage_count: u32,
    success_rate: f32,
    avg_time_ms: u32,
    last_used: Option<i64>,
}

pub fn export_dashboard_json(
    dash: &FragmentAnalyticsDashboard,
    range: FragmentAnalyticsTimeRange,
) -> Result<String, serde_json::Error> {
    let payload = DashboardExport {
        time_range: range.label_en(),
        exported_at: chrono::Utc::now().to_rfc3339(),
        team_api_available: dash.team_api_available,
        personal_total_usage: dash.personal_total_usage,
        personal_success_rate: dash.personal_success_rate,
        personal_avg_ms: dash.personal_avg_ms,
        team_total_usage: dash.team_total_usage,
        team_success_rate: dash.team_success_rate,
        team_avg_ms: dash.team_avg_ms,
        personal_top: dash.personal_top.iter().map(export_row).collect(),
        team_top: dash.team_top.iter().map(export_row).collect(),
        slowest: dash.slowest.iter().map(export_row).collect(),
        highest_error: dash.highest_error.iter().map(export_row).collect(),
        member_rows: dash
            .member_rows
            .iter()
            .map(|m| ExportMemberRow {
                user_id: m.user_id.clone(),
                display_name: m.display_name.clone(),
                run_count: m.run_count,
                success_count: m.success_count,
            })
            .collect(),
        period_stats_from_events: dash.period_stats_from_events,
        member_stats_from_server: dash.member_stats_from_server,
    };
    serde_json::to_string_pretty(&payload)
}

fn export_row(f: &FragmentStats) -> ExportSnippetRow {
    ExportSnippetRow {
        id: f.id.clone(),
        title: f.title.clone(),
        usage_count: f.usage_count,
        success_rate: f.success_rate(),
        avg_time_ms: f.avg_time_ms(),
        last_used: f.last_used,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::FragmentStats;
    use crate::core::team::FragmentMemberAnalyticsRow;

    fn fs(id: &str, usage: u32, success: u32, total_ms: u64, last_used: Option<i64>) -> FragmentStats {
        let mut f = FragmentStats::new(
            id.to_string(),
            id.to_string(),
            "echo x".into(),
            "ops".into(),
        );
        f.usage_count = usage;
        f.success_count = success;
        f.total_time_ms = total_ms;
        f.last_used = last_used;
        f
    }

    #[test]
    fn time_range_filters_by_last_used() {
        let now = chrono::Utc::now().timestamp();
        let mut recent = fs("a", 1, 1, 100, Some(now - 86_400));
        recent.usage_count = 1;
        let mut old = fs("b", 2, 2, 200, Some(now - 86_400 * 40));
        old.usage_count = 2;
        let filtered =
            FragmentAnalyticsTimeRange::Last7Days.filter_fragments(&[recent, old]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "a");
    }

    #[test]
    fn member_rows_from_api_sorts_by_run_count() {
        let rows = member_rows_from_api(&[
            FragmentMemberAnalyticsRow {
                user_id: "u1".into(),
                display_name: "Alice".into(),
                run_count: 5,
                success_count: 4,
            },
            FragmentMemberAnalyticsRow {
                user_id: "u2".into(),
                display_name: "Bob".into(),
                run_count: 12,
                success_count: 10,
            },
        ]);
        assert_eq!(rows[0].display_name, "Bob");
        assert_eq!(rows[0].run_count, 12);
    }

    // ------------------------------------------------ TimeRange accessors
    #[test]
    fn time_range_default_is_all_time() {
        assert_eq!(
            FragmentAnalyticsTimeRange::default(),
            FragmentAnalyticsTimeRange::AllTime
        );
    }

    #[test]
    fn time_range_since_days_map_to_variants() {
        assert_eq!(FragmentAnalyticsTimeRange::AllTime.since_days(), None);
        assert_eq!(FragmentAnalyticsTimeRange::Last7Days.since_days(), Some(7));
        assert_eq!(FragmentAnalyticsTimeRange::Last30Days.since_days(), Some(30));
        assert_eq!(FragmentAnalyticsTimeRange::Last90Days.since_days(), Some(90));
    }

    #[test]
    fn time_range_cutoff_all_time_is_none_others_are_reasonable() {
        assert!(FragmentAnalyticsTimeRange::AllTime.cutoff_unix().is_none());
        let now = chrono::Utc::now().timestamp();
        // Last7 days cutoff should be ~7 days ago (within ±2 min of drift).
        let c7 = FragmentAnalyticsTimeRange::Last7Days.cutoff_unix().unwrap();
        assert!((now - 7 * 86400 - c7).abs() < 120);
        let c30 = FragmentAnalyticsTimeRange::Last30Days.cutoff_unix().unwrap();
        assert!((now - 30 * 86400 - c30).abs() < 120);
        let c90 = FragmentAnalyticsTimeRange::Last90Days.cutoff_unix().unwrap();
        assert!((now - 90 * 86400 - c90).abs() < 120);
    }

    #[test]
    fn time_range_labels_all_deterministic() {
        use FragmentAnalyticsTimeRange::*;
        assert_eq!(AllTime.label_en(), "All time");
        assert_eq!(Last7Days.label_en(), "Last 7 days");
        assert_eq!(Last30Days.label_en(), "Last 30 days");
        assert_eq!(Last90Days.label_en(), "Last 90 days");
        assert_eq!(AllTime.label_zh(), "全部时间");
        assert_eq!(Last7Days.label_zh(), "近 7 天");
        assert_eq!(Last30Days.label_zh(), "近 30 天");
        assert_eq!(Last90Days.label_zh(), "近 90 天");
    }

    #[test]
    fn filter_fragments_all_time_returns_cloned_list_including_none_last_used() {
        let list = [
            fs("a", 1, 1, 100, None),
            fs("b", 2, 2, 200, Some(1)),
        ];
        let got = FragmentAnalyticsTimeRange::AllTime.filter_fragments(&list);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].id, "a");
        assert_eq!(got[1].id, "b");
    }

    #[test]
    fn filter_fragments_last_30_days_drops_ancient_and_none_last_used() {
        let now = chrono::Utc::now().timestamp();
        let list = [
            fs("a", 1, 1, 100, None),
            fs("recent", 2, 2, 200, Some(now - 86400)),
            fs("ancient", 3, 3, 300, Some(now - 400 * 86400)),
        ];
        let got = FragmentAnalyticsTimeRange::Last30Days.filter_fragments(&list);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "recent");
    }

    // ------------------------------------------------ member_rows_from_api
    #[test]
    fn member_rows_from_api_uses_user_id_when_display_name_empty() {
        let rows = member_rows_from_api(&[FragmentMemberAnalyticsRow {
            user_id: "u-only".into(),
            display_name: String::new(),
            run_count: 2,
            success_count: 1,
        }]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].display_name, "u-only");
        assert_eq!(rows[0].run_count, 2);
        assert_eq!(rows[0].success_count, 1);
        assert_eq!(rows[0].user_id, "u-only");
    }

    #[test]
    fn member_rows_from_api_empty_input_is_empty() {
        assert!(member_rows_from_api(&[]).is_empty());
    }

    // ------------------------------------------------ build_dashboard aggregates
    #[test]
    fn dashboard_empty_input_gives_zeroes_and_empty_lists() {
        let d = build_dashboard(&[], &[], false);
        assert_eq!(d.personal_total_usage, 0);
        assert_eq!(d.team_total_usage, 0);
        assert_eq!(d.personal_success_rate, 0.0);
        assert_eq!(d.team_success_rate, 0.0);
        assert_eq!(d.personal_avg_ms, 0);
        assert_eq!(d.team_avg_ms, 0);
        assert!(d.personal_top.is_empty());
        assert!(d.team_top.is_empty());
        assert!(d.slowest.is_empty());
        assert!(d.highest_error.is_empty());
        assert!(!d.team_api_available);
        assert!(!d.period_stats_from_events);
        assert!(!d.member_stats_from_server);
        assert!(d.member_rows.is_empty());
    }

    #[test]
    fn dashboard_aggregates_personal_and_team() {
        let personal = vec![
            fs("p1", 10, 9, 1000, Some(1)),  // avg=100ms, 90%
            fs("p2", 0, 0, 0, None),          // ignored in top
            fs("p3", 2, 0, 20, Some(2)),      // 2nd top
        ];
        let team = vec![
            fs("t1", 5, 5, 500, Some(10)),    // 100% avg 100ms; 1st top
            fs("t2", 1, 0, 50, Some(9)),      // 2nd top
        ];
        let d = build_dashboard(&personal, &team, true);

        assert_eq!(d.personal_total_usage, 12); // 10+0+2
        assert_eq!(d.team_total_usage, 6);
        // personal rate: 9/12 = 75%
        assert!((d.personal_success_rate - 75.0).abs() < 0.1);
        // personal avg: (1000+0+20)/12 = 1020/12 = 85
        assert_eq!(d.personal_avg_ms, 85);
        // team rate: 5/6 ≈ 83.33%
        assert!((d.team_success_rate - (5.0 / 6.0 * 100.0)).abs() < 0.1);
        // team avg: 550/6 = 91
        assert_eq!(d.team_avg_ms, 91);

        // Personal top sorted desc by usage: p1=10, p3=2 (p2=0 excluded).
        let pids: Vec<&str> = d.personal_top.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(pids, vec!["p1", "p3"]);
        // Team top: t1=5, t2=1.
        let tids: Vec<&str> = d.team_top.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(tids, vec!["t1", "t2"]);

        assert!(d.team_api_available);
    }

    #[test]
    fn dashboard_slowest_uses_avg_time_and_skips_usage_zero() {
        let personal = vec![
            fs("p1", 2, 2, 200, None),   // avg=100
            fs("p2", 0, 0, 9000, None),  // skip, usage_count==0
        ];
        let team = vec![
            fs("t1", 1, 1, 500, None),   // avg=500 — slowest #1
            fs("t2", 4, 4, 800, None),   // avg=200 — slowest #2
        ];
        let d = build_dashboard(&personal, &team, false);
        let ids: Vec<&str> = d.slowest.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["t1", "t2", "p1"]);
    }

    #[test]
    fn dashboard_highest_error_filter_requires_min_3_usage() {
        let personal = vec![
            fs("bad", 5, 0, 100, None),   // 100% error, 5≥3 — qualify
            fs("med", 10, 5, 200, None),  // 50% error — qualify
            fs("small", 2, 0, 10, None),  // 100% error, but 2<3 — excluded
        ];
        let team = vec![
            fs("tgood", 10, 10, 200, None), // 0% error
        ];
        let d = build_dashboard(&personal, &team, false);
        let ids: Vec<&str> = d.highest_error.iter().map(|s| s.id.as_str()).collect();
        // error rate desc: bad (100) → med (50) → tgood (0). small excluded.
        assert_eq!(ids, vec!["bad", "med", "tgood"]);
    }

    #[test]
    fn dashboard_top_n_truncates_at_five_with_sort_desc_usage() {
        let team: Vec<FragmentStats> = (0..10u32)
            .map(|i| {
                let mut f = fs(
                    &format!("t{}", i),
                    100 + i,
                    50,
                    100,
                    Some(1),
                );
                f.usage_count = 100 + i;
                f
            })
            .collect();
        let d = build_dashboard(&[], &team, false);
        assert_eq!(d.team_top.len(), 5);
        // Sorted desc usage: t9 (109) → t5 (105)
        for (rank, expect_id) in ["t9", "t8", "t7", "t6", "t5"].iter().enumerate() {
            assert_eq!(d.team_top[rank].id, *expect_id, "rank {} mismatch", rank);
        }
    }

    // ------------------------------------------------ build_dashboard_with_events (AllTime path)
    #[test]
    fn dashboard_with_events_all_time_shortcut_calls_filter_not_apply_events() {
        let now = chrono::Utc::now().timestamp();
        let mut p1 = fs("p1", 5, 5, 100, Some(now - 86400));
        p1.usage_count = 5;
        let mut p2 = fs("p2", 1, 0, 20, Some(now - 365 * 86400));
        p2.usage_count = 1;
        let personal = [p1, p2];
        let d = build_dashboard_with_events(
            &personal,
            &[],
            &[],
            FragmentAnalyticsTimeRange::AllTime,
            false,
            None,
            &[],
        );
        // All-time: uses filter (last_used isn't bounded), so total 6, top includes both
        assert_eq!(d.personal_total_usage, 6);
        assert_eq!(d.personal_top.len(), 2);
        assert!(!d.period_stats_from_events);
    }

    // ------------------------------------------------ export_dashboard_json
    #[test]
    fn export_json_contains_field_level_values_and_time_range() {
        let d = build_dashboard(
            &[fs("p1", 10, 10, 500, Some(123))],
            &[fs("t1", 4, 2, 400, Some(456))],
            true,
        );
        let json =
            export_dashboard_json(&d, FragmentAnalyticsTimeRange::Last30Days).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["time_range"], "Last 30 days");
        assert_eq!(v["team_api_available"], true);
        assert_eq!(v["personal_total_usage"], 10);
        assert_eq!(v["team_total_usage"], 4);
        assert_eq!(v["personal_top"][0]["id"], "p1");
        assert_eq!(v["team_top"][0]["id"], "t1");
        let rate_p: f32 = v["personal_success_rate"].as_number().unwrap().as_f64().unwrap() as f32;
        assert!((rate_p - 100.0).abs() < 0.1);
        assert_eq!(v["personal_avg_ms"], 50);
        let rate_t: f32 = v["team_success_rate"].as_number().unwrap().as_f64().unwrap() as f32;
        assert!((rate_t - 50.0).abs() < 0.1);
        assert_eq!(v["team_avg_ms"], 100);
        // exported_at is RFC3339 (T and Z presence)
        let ea = v["exported_at"].as_str().unwrap();
        assert!(ea.contains('T'), "no T in exported_at: {ea}");
    }
}
