//! Read-only in-memory audit timeline dialog.

use eframe::egui;

use crate::core::{AuditTimeline, TimelineOutcome};
use crate::ui::{chrome, layout_util};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuditTimelineUiAction {
    #[default]
    None,
    Clear,
}

pub fn show_audit_timeline_modal(
    ctx: &egui::Context,
    theme: &Theme,
    open: &mut bool,
    timeline: &AuditTimeline,
    host_filter: &mut String,
    outcome_filter: &mut Option<TimelineOutcome>,
    action: &mut AuditTimelineUiAction,
) {
    if !*open {
        return;
    }
    *action = AuditTimelineUiAction::None;
    let mut dialog_open = *open;
    let mut should_close = false;
    let modal_sz = egui::vec2(700.0, 560.0);
    let title = crate::i18n::tr(ctx, "Audit timeline", "审计时间线");

    chrome::modal_window("audit_timeline", theme, ctx)
        .open(&mut dialog_open)
        .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
        .movable(true)
        .resizable(true)
        .default_size(modal_sz)
        .show(ctx, |ui| {
            chrome::modal_content_frame(theme).show(ui, |ui| {
                chrome::modal_header_title_only(ui, theme, title, theme.font_size_modal_title());
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ctx,
                        "Read-only local cache; it is not a policy decision source and is never uploaded.",
                        "只读本地缓存；不参与策略判定，也不会上传。",
                    ))
                    .size(theme.font_size_caption())
                    .color(theme.text_tertiary()),
                );
                ui.add_space(theme.spacing_sm());

                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr(ctx, "Host", "主机"));
                    ui.add(
                        egui::TextEdit::singleline(host_filter)
                            .desired_width(190.0)
                            .hint_text(crate::i18n::tr(ctx, "Filter host…", "筛选主机…")),
                    );
                    ui.label(crate::i18n::tr(ctx, "Outcome", "结果"));
                    egui::ComboBox::from_id_source("audit_timeline_outcome")
                        .selected_text(outcome_label(ctx, *outcome_filter))
                        .show_ui(ui, |ui| {
                            chrome::apply_menu_popup_style(ui, theme);
                            ui.selectable_value(
                                outcome_filter,
                                None,
                                crate::i18n::tr(ctx, "All", "全部"),
                            );
                            for outcome in [
                                TimelineOutcome::Blocked,
                                TimelineOutcome::Confirmed,
                                TimelineOutcome::Allowed,
                                TimelineOutcome::Info,
                            ] {
                                ui.selectable_value(
                                    outcome_filter,
                                    Some(outcome),
                                    outcome_label(ctx, Some(outcome)),
                                );
                            }
                        });
                    if ui.button(crate::i18n::tr(ctx, "Clear", "清空")).clicked() {
                        *action = AuditTimelineUiAction::Clear;
                    }
                });

                ui.add_space(theme.spacing_sm());
                let entries = timeline.filter(host_filter.trim(), *outcome_filter);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(layout_util::dialog_scroll_max_height(ctx, 300.0))
                    .show(ui, |ui| {
                        if entries.is_empty() {
                            ui.label(
                                egui::RichText::new(crate::i18n::tr(
                                    ctx,
                                    "No audit events",
                                    "暂无审计事件",
                                ))
                                .color(theme.text_tertiary()),
                            );
                        }
                        for entry in entries.iter().rev() {
                            theme.frame_region_panel().show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(outcome_label(ctx, Some(entry.outcome)))
                                            .strong()
                                            .color(outcome_color(theme, entry.outcome)),
                                    );
                                    ui.label(
                                        egui::RichText::new(&entry.ts)
                                            .size(theme.font_size_small())
                                            .color(theme.text_tertiary()),
                                    );
                                    if !entry.host.is_empty() {
                                        ui.label(format!("· {}", entry.host));
                                    }
                                });
                                if !entry.command.is_empty() {
                                    ui.label(egui::RichText::new(&entry.command).monospace());
                                }
                                let detail = match (entry.rule.is_empty(), entry.note.is_empty()) {
                                    (true, true) => String::new(),
                                    (false, true) => entry.rule.clone(),
                                    (true, false) => entry.note.clone(),
                                    (false, false) => format!("{} · {}", entry.rule, entry.note),
                                };
                                if !detail.is_empty() {
                                    ui.label(
                                        egui::RichText::new(detail)
                                            .size(theme.font_size_caption())
                                            .color(theme.text_tertiary()),
                                    );
                                }
                            });
                            ui.add_space(theme.spacing_xs());
                        }
                    });
                ui.add_space(theme.spacing_sm());
                ui.horizontal(|ui| {
                    ui.label(format!("{} / {}", entries.len(), timeline.len()));
                    if ui.button(crate::i18n::tr(ctx, "Close", "关闭")).clicked() {
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

fn outcome_label(ctx: &egui::Context, outcome: Option<TimelineOutcome>) -> &'static str {
    match outcome {
        None => crate::i18n::tr(ctx, "All", "全部"),
        Some(TimelineOutcome::Blocked) => crate::i18n::tr(ctx, "Blocked", "已拦截"),
        Some(TimelineOutcome::Confirmed) => crate::i18n::tr(ctx, "Confirmed", "已确认"),
        Some(TimelineOutcome::Allowed) => crate::i18n::tr(ctx, "Allowed", "已放行"),
        Some(TimelineOutcome::Info) => crate::i18n::tr(ctx, "Info", "记录"),
    }
}

fn outcome_color(theme: &Theme, outcome: TimelineOutcome) -> egui::Color32 {
    match outcome {
        TimelineOutcome::Blocked => theme.color_danger_emphasis(),
        TimelineOutcome::Confirmed => theme.amber_color(),
        TimelineOutcome::Allowed => theme.accent_color(),
        TimelineOutcome::Info => theme.text_secondary(),
    }
}
