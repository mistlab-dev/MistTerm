//! 命令片段智能推荐（命令历史 + 执行日志，纯本地）。

use std::collections::HashMap;

use crate::core::command_history::CommandHistory;
use crate::core::FragmentStats;

#[derive(Debug, Clone)]
pub struct FragmentRecommendation {
    pub command: String,
    pub count: u32,
    pub source: &'static str,
}

fn normalize_command(cmd: &str) -> String {
    cmd.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_trivial_command(cmd: &str) -> bool {
    let c = cmd.trim();
    if c.len() < 4 {
        return true;
    }
    let first = c.split_whitespace().next().unwrap_or("");
    matches!(
        first,
        "cd"
            | "ls"
            | "pwd"
            | "clear"
            | "exit"
            | "logout"
            | ":"
            | "history"
            | "echo"
            | "true"
            | "false"
    ) && c.split_whitespace().count() <= 2
}

fn covered_by_library(cmd: &str, fragments: &[FragmentStats]) -> bool {
    let n = normalize_command(cmd);
    fragments.iter().any(|f| {
        let fc = normalize_command(&f.command);
        fc == n || fc.contains(&n) || n.contains(&fc)
    })
}

pub fn recommend_from_history(
    history: &CommandHistory,
    personal_fragments: &[FragmentStats],
    cutoff: Option<i64>,
    limit: usize,
) -> Vec<FragmentRecommendation> {
    let mut counts: HashMap<String, u32> = HashMap::new();
    for entry in history.entries_newest_first() {
        if let Some(c) = cutoff {
            if entry.executed_at < c {
                continue;
            }
        }
        let n = normalize_command(&entry.command);
        if n.is_empty() || is_trivial_command(&n) {
            continue;
        }
        if covered_by_library(&n, personal_fragments) {
            continue;
        }
        *counts.entry(n).or_insert(0) += 1;
    }
    let mut rows: Vec<FragmentRecommendation> = counts
        .into_iter()
        .filter(|(_, c)| *c >= 3)
        .map(|(command, count)| FragmentRecommendation {
            command,
            count,
            source: "history",
        })
        .collect();
    rows.sort_by(|a, b| b.count.cmp(&a.count));
    rows.truncate(limit);
    rows
}

pub fn merge_recommendations(
    mut a: Vec<FragmentRecommendation>,
    b: Vec<FragmentRecommendation>,
    limit: usize,
) -> Vec<FragmentRecommendation> {
    for item in b {
        if let Some(existing) = a.iter_mut().find(|x| x.command == item.command) {
            existing.count = existing.count.max(item.count);
        } else {
            a.push(item);
        }
    }
    a.sort_by(|x, y| y.count.cmp(&x.count));
    a.truncate(limit);
    a
}

pub fn build_efficiency_report_markdown(
    dash: &crate::core::FragmentAnalyticsDashboard,
    range: crate::core::FragmentAnalyticsTimeRange,
    recommendations: &[FragmentRecommendation],
) -> String {
    let mut out = String::from("# MistTerm 效率报告\n\n");
    out.push_str(&format!(
        "- 时间范围: {}\n- 导出时间: {}\n\n",
        match range {
            crate::core::FragmentAnalyticsTimeRange::AllTime => "全部",
            crate::core::FragmentAnalyticsTimeRange::Last7Days => "近 7 天",
            crate::core::FragmentAnalyticsTimeRange::Last30Days => "近 30 天",
            crate::core::FragmentAnalyticsTimeRange::Last90Days => "近 90 天",
        },
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    ));
    out.push_str("## 汇总\n\n");
    out.push_str(&format!(
        "| 维度 | 执行次数 | 成功率 | 平均耗时 |\n|------|----------|--------|----------|\n| 个人 | {} | {:.0}% | {}ms |\n| 团队 | {} | {:.0}% | {}ms |\n\n",
        dash.personal_total_usage,
        dash.personal_success_rate,
        dash.personal_avg_ms,
        dash.team_total_usage,
        dash.team_success_rate,
        dash.team_avg_ms,
    ));
    if dash.period_stats_from_events {
        out.push_str("> 区间内次数来自本机执行日志。\n\n");
    }
    out.push_str("## 个人 Top 5\n\n");
    for (i, f) in dash.personal_top.iter().enumerate() {
        out.push_str(&format!(
            "{}. {} — {}× · {:.0}% · {}ms\n",
            i + 1,
            f.title,
            f.usage_count,
            f.success_rate(),
            f.avg_time_ms()
        ));
    }
    out.push_str("\n## 团队 Top 5\n\n");
    for (i, f) in dash.team_top.iter().enumerate() {
        out.push_str(&format!(
            "{}. {} — {}× · {:.0}% · {}ms\n",
            i + 1,
            f.title,
            f.usage_count,
            f.success_rate(),
            f.avg_time_ms()
        ));
    }
    if !dash.member_rows.is_empty() {
        out.push_str("\n## 团队成员（本机）\n\n");
        for m in &dash.member_rows {
            let rate = if m.run_count == 0 {
                0.0
            } else {
                (m.success_count as f32 / m.run_count as f32) * 100.0
            };
            out.push_str(&format!(
                "- {} — {}× · {:.0}% OK\n",
                m.display_name, m.run_count, rate
            ));
        }
    }
    if !recommendations.is_empty() {
        out.push_str("\n## 建议添加到片段库\n\n");
        for r in recommendations {
            out.push_str(&format!(
                "- `{}`（{} 次，来源：{}）\n",
                r.command, r.count, r.source
            ));
        }
    }
    out
}
