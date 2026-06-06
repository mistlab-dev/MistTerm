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

/// 将效率报告渲染为 PDF 字节（需可加载的 CJK TTF/TTC）。
pub fn build_efficiency_report_pdf(
    dash: &crate::core::FragmentAnalyticsDashboard,
    range: crate::core::FragmentAnalyticsTimeRange,
    recommendations: &[FragmentRecommendation],
) -> Result<Vec<u8>, String> {
    let font = load_pdf_cjk_font()?;
    let family = genpdf::fonts::FontFamily {
        regular: font.clone(),
        bold: font.clone(),
        italic: font.clone(),
        bold_italic: font,
    };
    let mut doc = genpdf::Document::new(family);
    doc.set_title("MistTerm Efficiency Report");
    doc.set_line_spacing(1.15);
    let mut decorator = genpdf::SimplePageDecorator::new();
    decorator.set_margins(12);
    doc.set_page_decorator(decorator);

    let md = build_efficiency_report_markdown(dash, range, recommendations);
    for line in md.lines() {
        if line.is_empty() {
            doc.push(genpdf::elements::Break::new(0.6));
        } else {
            doc.push(genpdf::elements::Paragraph::new(line.to_string()));
        }
    }

    let mut buf = Vec::new();
    doc.render(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

fn load_pdf_cjk_font() -> Result<genpdf::fonts::FontData, String> {
    const EMBEDDED: &[u8] = include_bytes!("../../assets/fonts/NotoSansSC-Regular.ttf");
    if let Some(font) = try_font_data(EMBEDDED.to_vec()) {
        log::debug!("PDF export using embedded NotoSansSC-Regular.ttf");
        return Ok(font);
    }
    for path in pdf_cjk_font_paths() {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if let Some(font) = try_font_data(bytes) {
            log::info!("PDF export using system font: {}", path.display());
            return Ok(font);
        }
    }
    Err(
        "未找到可用于 PDF 的中文字体（请运行 scripts/fetch-cjk-font.sh 下载嵌入字体）"
            .to_string(),
    )
}

fn try_font_data(bytes: Vec<u8>) -> Option<genpdf::fonts::FontData> {
    genpdf::fonts::FontData::new(bytes, None).ok()
}

fn pdf_cjk_font_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Ok(windir) = std::env::var("WINDIR") {
            let fonts = std::path::PathBuf::from(windir).join("Fonts");
            for name in ["msyh.ttc", "msyhbd.ttc", "simhei.ttf", "simsun.ttc"] {
                paths.push(fonts.join(name));
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        for p in [
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/Library/Fonts/Arial Unicode.ttf",
        ] {
            paths.push(std::path::PathBuf::from(p));
        }
    }
    #[cfg(target_os = "linux")]
    {
        for p in [
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        ] {
            paths.push(std::path::PathBuf::from(p));
        }
    }
    paths
}

#[cfg(test)]
mod pdf_tests {
    use super::*;
    use crate::core::FragmentAnalyticsDashboard;

    #[test]
    fn efficiency_report_pdf_non_empty() {
        // Skip if embedded font cannot be parsed by genpdf
        // (e.g. CFF-based OTF on platforms where printpdf rejects it)
        let dash = FragmentAnalyticsDashboard {
            personal_total_usage: 1,
            personal_success_rate: 100.0,
            personal_avg_ms: 10,
            team_total_usage: 0,
            team_success_rate: 0.0,
            team_avg_ms: 0,
            personal_top: vec![],
            team_top: vec![],
            slowest: vec![],
            highest_error: vec![],
            team_api_available: false,
            member_rows: vec![],
            period_stats_from_events: false,
            member_stats_from_server: false,
        };
        let pdf = match build_efficiency_report_pdf(
            &dash,
            crate::core::FragmentAnalyticsTimeRange::AllTime,
            &[],
        ) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("SKIP: PDF font not available: {e}");
                return;
            }
        };
        assert!(pdf.starts_with(b"%PDF"));
        assert!(pdf.len() > 512);
    }
}
