//! 本地审计日志查看（JSONL）

use crate::core::{
    format_audit_jsonl_for_display, list_audit_log_files, read_audit_log_tail, AuditSettings,
    AUDIT_LOG_TAIL_READ_BYTES,
};
use crate::ui::chrome;
use crate::ui::layout_util;
use crate::ui::theme::Theme;
use arboard::Clipboard;
use eframe::egui;
use std::path::PathBuf;

pub struct AuditLogDialog {
    pub open: bool,
    log_files: Vec<PathBuf>,
    selected_file: usize,
    content: String,
    search_query: String,
    filter_category: String,
}

impl Default for AuditLogDialog {
    fn default() -> Self {
        Self {
            open: false,
            log_files: Vec::new(),
            selected_file: 0,
            content: String::new(),
            search_query: String::new(),
            filter_category: String::new(),
        }
    }
}

impl AuditLogDialog {
    pub fn open_viewer(&mut self, ctx: &egui::Context, settings: &AuditSettings) {
        self.log_files = list_audit_log_files(&settings.file_dir);
        self.selected_file = 0;
        self.search_query.clear();
        self.filter_category.clear();
        self.reload_content(ctx, settings);
        self.open = true;
    }

    fn reload_content(&mut self, ctx: &egui::Context, settings: &AuditSettings) {
        self.content.clear();
        if !settings.enabled {
            self.content = crate::i18n::tr(
                ctx,
                "Audit logging is disabled in Preferences.",
                "审计日志已在偏好设置中关闭。",
            )
            .to_string();
            return;
        }
        if let Some(path) = self.log_files.get(self.selected_file) {
            match read_audit_log_tail(path, AUDIT_LOG_TAIL_READ_BYTES) {
                Ok(raw) if raw.trim().is_empty() => {
                    self.content = format!(
                        "{}{}",
                        crate::i18n::tr(ctx, "File is empty: ", "文件为空："),
                        path.display()
                    );
                }
                Ok(raw) => {
                    self.content = format_audit_jsonl_for_display(&raw);
                    if self.content.trim().is_empty() {
                        self.content = crate::i18n::tr(
                            ctx,
                            "The file has data but no audit events could be parsed (check JSONL format).",
                            "文件有内容但无法解析为审计事件（请检查 JSONL 格式）。",
                        )
                        .to_string();
                    }
                }
                Err(e) => {
                    self.content = format!(
                        "{}{}\n{}{}",
                        crate::i18n::tr(ctx, "Read failed: ", "无法读取："),
                        e,
                        crate::i18n::tr(ctx, "Path: ", "路径："),
                        path.display()
                    );
                }
            }
        } else if self.log_files.is_empty() {
            self.content = format!(
                "{}\n{}{}\n{}",
                crate::i18n::tr(
                    ctx,
                    "No audit files yet.",
                    "暂无审计文件。",
                ),
                crate::i18n::tr(ctx, "Directory: ", "目录："),
                settings.file_dir.display(),
                crate::i18n::tr(
                    ctx,
                    "Files named audit-YYYY-MM-DD.jsonl appear after connects, credential, or Vault actions.",
                    "执行连接、凭证或 Vault 操作后会生成 audit-YYYY-MM-DD.jsonl",
                ),
            );
        }
    }

    fn filtered_content(&self) -> String {
        let q = self.search_query.trim().to_lowercase();
        let cat = self.filter_category.trim().to_lowercase();
        if q.is_empty() && cat.is_empty() {
            return self.content.clone();
        }
        self.content
            .lines()
            .filter(|line| {
                let ll = line.to_lowercase();
                (q.is_empty() || ll.contains(&q)) && (cat.is_empty() || ll.contains(&cat))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme, settings: &AuditSettings) {
        if !self.open {
            return;
        }
        let mut should_close = false;
        let mut reload = false;
        let mut copy_all = false;
        let display = self.filtered_content();
        let display_for_view = if display.is_empty() {
            crate::i18n::tr(ctx, "No matching lines", "无匹配行").to_string()
        } else {
            display
        };
        let modal_sz = layout_util::modal_edit_size(ctx);
        chrome::modal_window("audit_log_viewer", theme, ctx)
            .open(&mut self.open)
            .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
            .movable(true)
            .resizable(true)
            .default_size(modal_sz)
            .show(ctx, |ui| {
                chrome::modal_content_frame(theme).show(ui, |ui| {
                    if chrome::modal_header(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Audit log", "审计日志"),
                        chrome::modal_title_font_size(theme),
                    ) {
                        should_close = true;
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "{}{}",
                            crate::i18n::tr(ctx, "Directory: ", "目录："),
                            settings.file_dir.display()
                        ))
                            .size(theme.font_size_caption())
                            .color(theme.color_form_hint()),
                    );
                    ui.horizontal(|ui| {
                        chrome::form_field_label(
                            ui,
                            theme,
                            crate::i18n::tr(ctx, "Log file", "日志文件"),
                        );
                        let prev = self.selected_file;
                        egui::ComboBox::from_id_source("audit_log_file_pick")
                            .selected_text(
                                self.log_files
                                    .get(self.selected_file)
                                    .and_then(|p| p.file_name())
                                    .and_then(|n| n.to_str())
                                    .unwrap_or(crate::i18n::tr(ctx, "(none)", "（无）")),
                            )
                            .show_ui(ui, |ui| {
                                chrome::apply_menu_popup_style(ui, theme);
                                for (i, path) in self.log_files.iter().enumerate() {
                                    let label = path
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or("?");
                                    if ui.selectable_label(self.selected_file == i, label).clicked()
                                    {
                                        self.selected_file = i;
                                    }
                                }
                            });
                        if self.selected_file != prev {
                            reload = true;
                        }
                        if chrome::panel_action_icon_button(
                            ui,
                            theme,
                            crate::ui::icons::IconId::Refresh,
                            crate::i18n::tr(ctx, "Refresh", "刷新"),
                        )
                            .clicked() {
                            self.log_files = list_audit_log_files(&settings.file_dir);
                            reload = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        let row_w = ui.available_width().max(320.0);
                        let half = ((row_w - theme.spacing_panel_gap()) / 2.0).max(120.0);
                        ui.vertical(|ui| {
                            ui.set_width(half);
                            chrome::form_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Search", "搜索"),
                            );
                            chrome::form_singleline_field(
                                ui,
                                theme,
                                egui::Id::new("audit_log_search"),
                                &mut self.search_query,
                                crate::i18n::tr(
                                    ctx,
                                    "action / host / resource…",
                                    "操作 / 主机 / 资源…",
                                ),
                                half,
                                false,
                            );
                        });
                        ui.vertical(|ui| {
                            ui.set_width(half);
                            chrome::form_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Category", "类别"),
                            );
                            chrome::form_singleline_field(
                                ui,
                                theme,
                                egui::Id::new("audit_log_category"),
                                &mut self.filter_category,
                                crate::i18n::tr(
                                    ctx,
                                    "session / vault / …",
                                    "会话 / vault / …",
                                ),
                                half,
                                false,
                            );
                        });
                    });
                    ui.add_space(4.0);
                    let h = ui.available_height().max(200.0);
                    egui::ScrollArea::vertical()
                        .id_source("audit_log_scroll")
                        .max_height(h)
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            let w = ui.max_rect().width();
                            ui.set_width(w);
                            chrome::selectable_readonly_monospace(
                                ui,
                                theme,
                                display_for_view.as_str(),
                                theme.font_size_caption(),
                                w,
                            );
                        });
                    ui.add_space(theme.spacing_sm());
                    ui.horizontal(|ui| {
                        chrome::modal_footer_actions(ui, theme, |ui, th| {
                            if chrome::modal_secondary_icon_button(
                                ui,
                                th,
                                crate::ui::icons::IconId::Close,
                                crate::i18n::tr(ctx, "Close", "关闭"),
                            )
                                .clicked() {
                                should_close = true;
                            }
                            if chrome::modal_secondary_icon_button(
                                ui,
                                th,
                                crate::ui::icons::IconId::Copy,
                                crate::i18n::tr(ctx, "Copy all", "复制全部"),
                            )
                                .clicked() {
                                copy_all = true;
                            }
                        });
                    });
                });
            });
        if reload {
            self.reload_content(ctx, settings);
        }
        if copy_all {
            let _ = Clipboard::new().and_then(|mut c| c.set_text(display_for_view));
        }
        if should_close {
            self.open = false;
        }
    }
}
