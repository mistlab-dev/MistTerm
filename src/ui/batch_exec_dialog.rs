//! 批量多机 SSH 执行弹窗。

use std::collections::HashSet;

use eframe::egui;

use crate::core::batch_exec::{BatchExecRow, BatchTarget};
use crate::ui::{layout_util, theme::Theme};

pub struct BatchExecDialog {
    pub open: bool,
    pub(crate) selected: HashSet<String>,
    pub(crate) include_team_servers: bool,
    pub(crate) command: String,
    pub(crate) max_parallel: u32,
    pub running: bool,
    pub results: Vec<BatchExecRow>,
    select_filter: String,
}

impl Default for BatchExecDialog {
    fn default() -> Self {
        Self {
            open: false,
            selected: HashSet::new(),
            include_team_servers: true,
            command: String::new(),
            max_parallel: 4,
            running: false,
            results: Vec::new(),
            select_filter: String::new(),
        }
    }
}

impl BatchExecDialog {
    pub fn open(&mut self, preselect_connected: &[String]) {
        self.open = true;
        self.running = false;
        self.results.clear();
        if self.selected.is_empty() {
            self.selected.extend(preselect_connected.iter().cloned());
        }
    }

    pub fn show_modal(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        targets: &[BatchTarget],
        rx: Option<&std::sync::mpsc::Receiver<Vec<BatchExecRow>>>,
    ) -> BatchExecUiAction {
        if !self.open {
            return BatchExecUiAction::None;
        }
        if let Some(ch) = rx {
            if let Ok(rows) = ch.try_recv() {
                self.results = rows;
                self.running = false;
            }
        }

        let mut action = BatchExecUiAction::None;
        let title = crate::i18n::tr(ctx, "Batch run on servers", "批量多机执行");
        let mut keep_open = self.open;
        let mut should_close = false;
        let modal_sz = egui::vec2(720.0, 520.0);
        crate::ui::chrome::modal_window("batch_exec_dialog", theme, ctx)
            .open(&mut keep_open)
            .default_size(modal_sz)
            .min_width(480.0)
            .movable(true)
            .resizable(true)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    if crate::ui::chrome::modal_header(
                        ui,
                        theme,
                        title,
                        theme.font_size_modal_title(),
                    ) {
                        should_close = true;
                    }
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ctx,
                        "Runs one command per host over a separate SSH connection (no terminal tabs).",
                        "每台主机单独建立连接并执行命令（不占用终端标签）。",
                    ))
                    .size(theme.font_size_caption())
                    .color(theme.text_tertiary()),
                );
                ui.add_space(theme.spacing_sm());

                ui.label(crate::i18n::tr(ctx, "Command", "命令"));
                crate::ui::chrome::form_multiline_field_with_hint(
                    ui,
                    theme,
                    egui::Id::new("batch_exec_command"),
                    &mut self.command,
                    "uptime && hostname",
                    layout_util::finite_content_width(ui),
                    3,
                    false,
                );

                crate::ui::chrome::form_control_row(ui, theme, |ui| {
                    crate::ui::chrome::form_inline_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Max parallel", "同时执行台数"),
                    );
                    crate::ui::chrome::form_u32_stepper(
                        ui,
                        theme,
                        egui::Id::new("batch_max_parallel"),
                        &mut self.max_parallel,
                        1..=16,
                        1,
                    );
                    crate::ui::chrome::form_inline_checkbox(
                        ui,
                        theme,
                        "batch_include_team",
                        &mut self.include_team_servers,
                        crate::i18n::tr(ctx, "Include team servers", "包含团队服务器"),
                    );
                });

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm();
                    if crate::ui::chrome::panel_action_button(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Select all visible", "全选当前列表"),
                    )
                    .clicked()
                    {
                        for t in filtered_targets(targets, &self.select_filter) {
                            self.selected.insert(t.id.clone());
                        }
                    }
                    if crate::ui::chrome::panel_action_button(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Clear selection", "清空选择"),
                    )
                    .clicked()
                    {
                        self.selected.clear();
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "{}: {}",
                            crate::i18n::tr(ctx, "Selected", "已选"),
                            self.selected.len()
                        ))
                        .size(theme.font_size_caption())
                        .color(theme.text_tertiary()),
                    );
                });

                ui.add_space(theme.spacing_xs());
                crate::ui::chrome::form_singleline_field(
                    ui,
                    theme,
                    egui::Id::new("batch_host_filter"),
                    &mut self.select_filter,
                    crate::i18n::tr(ctx, "Filter hosts…", "筛选主机…"),
                    ui.available_width(),
                    false,
                );

                ui.add_space(theme.spacing_sm());
                ui.label(crate::i18n::tr(ctx, "Targets", "目标主机"));
                egui::ScrollArea::vertical()
                    .max_height(140.0)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = theme.spacing_sm();
                        for t in filtered_targets(targets, &self.select_filter) {
                            let mut checked = self.selected.contains(&t.id);
                            ui.horizontal(|ui| {
                                if crate::ui::chrome::form_checkbox_with_id(
                                    ui,
                                    theme,
                                    &t.id,
                                    &mut checked,
                                    "",
                                )
                                .changed()
                                {
                                    if checked {
                                        self.selected.insert(t.id.clone());
                                    } else {
                                        self.selected.remove(&t.id);
                                    }
                                }
                                ui.label(format!(
                                    "{} · {}",
                                    t.label,
                                    crate::i18n::session_group_display_name(ctx, &t.group)
                                ));
                            });
                        }
                    });

                ui.add_space(theme.spacing_sm());
                let can_run = !self.running
                    && !self.command.trim().is_empty()
                    && !self.selected.is_empty();
                crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                    if ui
                        .add(
                            crate::ui::chrome::modal_primary_button_widget(
                                th,
                                crate::i18n::tr(ctx, "Run on selected", "在选中主机上执行"),
                            )
                            .can_activate(can_run),
                        )
                        .clicked()
                        && can_run
                    {
                        action = BatchExecUiAction::Run;
                    }
                    if self.running {
                        ui.spinner();
                        ui.label(crate::i18n::tr(ctx, "Running…", "执行中…"));
                    }
                    if crate::ui::chrome::panel_action_button_ex(
                        ui,
                        th,
                        crate::i18n::tr(ctx, "Copy results", "复制结果"),
                        !self.results.is_empty(),
                    )
                    .clicked()
                    {
                        action = BatchExecUiAction::CopyResults;
                    }
                });

                if !self.results.is_empty() {
                    ui.separator();
                    let ok_n = self.results.iter().filter(|r| r.ok).count();
                    ui.label(format!(
                        "{} {}/{}",
                        crate::i18n::tr(ctx, "Finished:", "完成："),
                        ok_n,
                        self.results.len()
                    ));
                    egui::ScrollArea::vertical()
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for row in &self.results {
                                let status = if row.ok {
                                    crate::i18n::tr(ctx, "OK", "成功")
                                } else {
                                    crate::i18n::tr(ctx, "Failed", "失败")
                                };
                                let code = row
                                    .exit_code
                                    .map(|c| c.to_string())
                                    .unwrap_or_else(|| "?".into());
                                let header =
                                    format!("{} — {} (exit {}, {}ms)", row.label, status, code, row.duration_ms);
                                ui.collapsing(header, |ui| {
                                    if let Some(e) = &row.error {
                                        ui.colored_label(theme.red_color(), e);
                                    }
                                    if !row.output.is_empty() {
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&row.output)
                                                    .monospace()
                                                    .size(theme.font_size_caption()),
                                            )
                                            .wrap(true),
                                        );
                                    }
                                });
                            }
                        });
                }
                });
            });
        if should_close {
            keep_open = false;
        }
        self.open = keep_open;
        action
    }
}

fn filtered_targets<'a>(targets: &'a [BatchTarget], filter: &str) -> Vec<&'a BatchTarget> {
    let q = filter.trim().to_lowercase();
    if q.is_empty() {
        return targets.iter().collect();
    }
    targets
        .iter()
        .filter(|t| {
            t.label.to_lowercase().contains(&q)
                || t.group.to_lowercase().contains(&q)
                || t.id.to_lowercase().contains(&q)
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchExecUiAction {
    None,
    Run,
    CopyResults,
}
