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

    #[test]
    fn time_range_filters_by_last_used() {
        let now = chrono::Utc::now().timestamp();
        let mut recent = FragmentStats::new("a".into(), "recent".into(), "x".into(), "c".into());
        recent.last_used = Some(now - 86_400);
        recent.usage_count = 1;
        let mut old = FragmentStats::new("b".into(), "old".into(), "y".into(), "c".into());
        old.last_used = Some(now - 86_400 * 40);
        old.usage_count = 2;
        let filtered =
            FragmentAnalyticsTimeRange::Last7Days.filter_fragments(&[recent, old]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "a");
    }

    #[test]
    fn member_rows_from_api_sorts_by_run_count() {
        use crate::core::team::FragmentMemberAnalyticsRow;
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
}
