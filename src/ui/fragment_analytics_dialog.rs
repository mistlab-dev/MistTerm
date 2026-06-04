//! 命令片段分析大盘（个人 + 团队）。

use eframe::egui;

use crate::core::{FragmentAnalyticsDashboard, FragmentAnalyticsTimeRange};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FragmentAnalyticsUiAction {
    #[default]
    None,
    Refresh,
    ExportJson,
    ExportEfficiencyReport,
    ExportEfficiencyReportPdf,
    AddRecommendation(usize),
}

pub fn show_fragment_analytics_modal(
    ctx: &egui::Context,
    theme: &Theme,
    open: &mut bool,
    range: &mut FragmentAnalyticsTimeRange,
    dash: &FragmentAnalyticsDashboard,
    recommendations: &[crate::core::FragmentRecommendation],
    action: &mut FragmentAnalyticsUiAction,
) {
    if !*open {
        return;
    }
    *action = FragmentAnalyticsUiAction::None;
    let mut dialog_open = *open;
    let mut should_close = false;
    let title = crate::i18n::tr(ctx, "Snippet analytics", "命令片段分析");
    let modal_sz = egui::vec2(640.0, 540.0);
    crate::ui::chrome::modal_window("fragment_analytics", theme, ctx)
        .open(&mut dialog_open)
        .default_pos(crate::ui::layout_util::modal_center_pos(ctx, modal_sz))
        .movable(true)
        .resizable(true)
        .default_size(modal_sz)
        .show(ctx, |ui| {
            crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                crate::ui::chrome::modal_header_title_only(
                    ui,
                    theme,
                    &title,
                    theme.font_size_modal_title(),
                );

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(crate::i18n::tr(ctx, "Time range", "时间范围"))
                            .size(theme.font_size_caption())
                            .color(theme.text_tertiary()),
                    );
                    let lang = crate::i18n::language(ctx);
                    egui::ComboBox::from_id_source("fragment_analytics_range")
                        .selected_text(match lang {
                            crate::i18n::UiLanguage::Zh => range.label_zh(),
                            crate::i18n::UiLanguage::En => range.label_en(),
                        })
                        .show_ui(ui, |ui| {
                            for candidate in [
                                FragmentAnalyticsTimeRange::AllTime,
                                FragmentAnalyticsTimeRange::Last7Days,
                                FragmentAnalyticsTimeRange::Last30Days,
                                FragmentAnalyticsTimeRange::Last90Days,
                            ] {
                                let label = match lang {
                                    crate::i18n::UiLanguage::Zh => candidate.label_zh(),
                                    crate::i18n::UiLanguage::En => candidate.label_en(),
                                };
                                if ui.selectable_value(range, candidate, label).changed() {
                                    *action = FragmentAnalyticsUiAction::Refresh;
                                }
                            }
                        });
                    if ui
                        .button(crate::i18n::tr(ctx, "Refresh", "刷新"))
                        .clicked()
                    {
                        *action = FragmentAnalyticsUiAction::Refresh;
                    }
                });
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ctx,
                        "Filters snippets by last used time; counts are lifetime totals.",
                        "按最近使用时间筛选片段；次数与成功率为累计统计。",
                    ))
                    .size(theme.font_size_caption())
                    .color(theme.text_tertiary()),
                );

            if dash.period_stats_from_events {
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ctx,
                        "Period counts from local execution log (incremental).",
                        "区间内次数来自本机执行日志（增量统计）。",
                    ))
                    .size(theme.font_size_caption())
                    .color(theme.text_tertiary()),
                );
            } else if dash.team_api_available {
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ctx,
                        "Team stats merged from server analytics API.",
                        "团队数据已与服务端分析 API 合并。",
                    ))
                    .size(theme.font_size_caption())
                    .color(theme.text_tertiary()),
                );
            }
            if !dash.member_rows.is_empty() {
                ui.add_space(theme.spacing_xs());
                ui.collapsing(
                    crate::i18n::tr(ctx, "Team members (this period)", "团队成员（本区间）"),
                    |ui| {
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(
                                ctx,
                                "Based on snippets run on this device; server-wide member stats pending API.",
                                "仅统计本机执行的团队片段；全团队数据待服务端接口。",
                            ))
                            .size(theme.font_size_caption())
                            .color(theme.text_tertiary()),
                        );
                        for m in &dash.member_rows {
                            let rate = if m.run_count == 0 {
                                0.0
                            } else {
                                (m.success_count as f32 / m.run_count as f32) * 100.0
                            };
                            ui.label(format!(
                                "· {} — {}× · {:.0}% OK",
                                m.display_name, m.run_count, rate
                            ));
                        }
                    },
                );
            }
            ui.add_space(theme.spacing_sm());

                ui.columns(3, |cols| {
                    let p_sub = format!(
                        "{} · {}ms",
                        format_rate(dash.personal_success_rate),
                        dash.personal_avg_ms
                    );
                    kpi_card(
                        &mut cols[0],
                        theme,
                        &crate::i18n::tr(ctx, "Personal runs", "个人执行"),
                        &format!("{}", dash.personal_total_usage),
                        &p_sub,
                    );
                    let t_sub = format!(
                        "{} · {}ms",
                        format_rate(dash.team_success_rate),
                        dash.team_avg_ms
                    );
                    kpi_card(
                        &mut cols[1],
                        theme,
                        &crate::i18n::tr(ctx, "Team runs", "团队执行"),
                        &format!("{}", dash.team_total_usage),
                        &t_sub,
                    );
                    kpi_card(
                        &mut cols[2],
                        theme,
                        &crate::i18n::tr(ctx, "Tracked snippets", "有统计片段"),
                        &format!("{}", dash.personal_top.len() + dash.team_top.len()),
                        &crate::i18n::tr(ctx, "Top lists below", "见下方排行"),
                    );
                });

                ui.add_space(theme.spacing_md());
                ui.columns(2, |cols| {
                    top_list(
                        &mut cols[0],
                        theme,
                        ctx,
                        &crate::i18n::tr(ctx, "Personal Top 5", "个人 Top 5"),
                        &dash.personal_top,
                    );
                    top_list(
                        &mut cols[1],
                        theme,
                        ctx,
                        &crate::i18n::tr(ctx, "Team Top 5", "团队 Top 5"),
                        &dash.team_top,
                    );
                });

                ui.add_space(theme.spacing_sm());
                ui.collapsing(
                    crate::i18n::tr(ctx, "Slowest & highest error rate", "最慢 / 高错误率"),
                    |ui| {
                        ui.label(crate::i18n::tr(ctx, "Slowest (avg)", "平均最慢"));
                        for f in &dash.slowest {
                            ui.label(format!(
                                "· {} — {:.1}s ({}×)",
                                f.title,
                                f.avg_time_ms() as f32 / 1000.0,
                                f.usage_count
                            ));
                        }
                        ui.add_space(theme.spacing_xs());
                        ui.label(crate::i18n::tr(ctx, "Highest error rate", "错误率最高"));
                        for f in &dash.highest_error {
                            ui.label(format!(
                                "· {} — {:.0}% fail ({}×)",
                                f.title,
                                100.0 - f.success_rate(),
                                f.usage_count
                            ));
                        }
                    },
                );

                if !recommendations.is_empty() {
                    ui.add_space(theme.spacing_sm());
                    ui.collapsing(
                        crate::i18n::tr(ctx, "Suggested snippets", "智能推荐片段"),
                        |ui| {
                            ui.label(
                                egui::RichText::new(crate::i18n::tr(
                                    ctx,
                                    "Frequent commands not in your library (from command history).",
                                    "命令历史中出现频繁、尚未收录为片段的命令。",
                                ))
                                .size(theme.font_size_caption())
                                .color(theme.text_tertiary()),
                            );
                            for (i, r) in recommendations.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.label(format!("· `{}` — {}×", r.command, r.count));
                                    if ui
                                        .small_button(crate::i18n::tr(
                                            ctx,
                                            "Add",
                                            "添加",
                                        ))
                                        .clicked()
                                    {
                                        *action = FragmentAnalyticsUiAction::AddRecommendation(i);
                                    }
                                });
                            }
                        },
                    );
                }

                ui.add_space(theme.spacing_md());
                ui.horizontal(|ui| {
                    if ui
                        .button(crate::i18n::tr(ctx, "Export JSON", "导出 JSON"))
                        .clicked()
                    {
                        *action = FragmentAnalyticsUiAction::ExportJson;
                    }
                    if ui
                        .button(crate::i18n::tr(
                            ctx,
                            "Efficiency report",
                            "效率报告",
                        ))
                        .clicked()
                    {
                        *action = FragmentAnalyticsUiAction::ExportEfficiencyReport;
                    }
                    if ui
                        .button(crate::i18n::tr(
                            ctx,
                            "Export PDF",
                            "导出 PDF",
                        ))
                        .clicked()
                    {
                        *action = FragmentAnalyticsUiAction::ExportEfficiencyReportPdf;
                    }
                    if ui
                        .button(crate::i18n::tr(ctx, "Close", "关闭"))
                        .clicked()
                    {
                        should_close = true;
                    }
                });
            });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        should_close = true;
    }
    if should_close {
        dialog_open = false;
    }
    *open = dialog_open;
}

fn format_rate(rate: f32) -> String {
    format!("{rate:.0}% OK")
}

fn kpi_card(ui: &mut egui::Ui, theme: &Theme, label: &str, value: &str, sub: &str) {
    theme.frame_region_panel().show(ui, |ui| {
        ui.label(
            egui::RichText::new(label)
                .size(theme.font_size_caption())
                .color(theme.text_tertiary()),
        );
        ui.label(
            egui::RichText::new(value)
                .size(theme.font_size_panel_title())
                .strong(),
        );
        ui.label(
            egui::RichText::new(sub)
                .size(theme.font_size_small())
                .color(theme.color_body_text_muted()),
        );
    });
}

fn top_list(
    ui: &mut egui::Ui,
    theme: &Theme,
    ctx: &egui::Context,
    heading: &str,
    items: &[crate::core::FragmentStats],
) {
    ui.label(egui::RichText::new(heading).strong());
    if items.is_empty() {
        ui.label(
            egui::RichText::new(crate::i18n::tr(ctx, "No data yet", "暂无数据"))
                .color(theme.text_tertiary()),
        );
        return;
    }
    for (i, f) in items.iter().enumerate() {
        ui.label(format!(
            "{}. {} — {}× · {:.0}%",
            i + 1,
            f.title,
            f.usage_count,
            f.success_rate()
        ));
    }
}
