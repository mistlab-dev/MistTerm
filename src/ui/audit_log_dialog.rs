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
    pub fn open_viewer(&mut self, settings: &AuditSettings) {
        self.log_files = list_audit_log_files(&settings.file_dir);
        self.selected_file = 0;
        self.search_query.clear();
        self.filter_category.clear();
        self.reload_content(settings);
        self.open = true;
    }

    fn reload_content(&mut self, settings: &AuditSettings) {
        self.content.clear();
        if !settings.enabled {
            self.content = "审计日志已在偏好设置中关闭。".to_string();
            return;
        }
        if let Some(path) = self.log_files.get(self.selected_file) {
            match read_audit_log_tail(path, AUDIT_LOG_TAIL_READ_BYTES) {
                Ok(raw) if raw.trim().is_empty() => {
                    self.content = format!("文件为空：{}", path.display());
                }
                Ok(raw) => {
                    self.content = format_audit_jsonl_for_display(&raw);
                    if self.content.trim().is_empty() {
                        self.content =
                            "文件有内容但无法解析为审计事件（请检查 JSONL 格式）。".to_string();
                    }
                }
                Err(e) => {
                    self.content = format!("无法读取：{e}\n路径：{}", path.display());
                }
            }
        } else if self.log_files.is_empty() {
            self.content = format!(
                "暂无审计文件。\n目录：{}\n执行连接、凭证或 Vault 操作后会生成 audit-YYYY-MM-DD.jsonl",
                settings.file_dir.display()
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
            "无匹配行".to_string()
        } else {
            display
        };
        egui::Window::new("audit_log_viewer")
            .open(&mut self.open)
            .title_bar(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .movable(true)
            .resizable(true)
            .default_size(layout_util::modal_edit_size(ctx))
            .frame(chrome::modal_window_frame(theme))
            .show(ctx, |ui| {
                chrome::modal_content_frame(theme).show(ui, |ui| {
                    if chrome::modal_header(ui, theme, "审计日志", theme.font_size_body()) {
                        should_close = true;
                    }
                    ui.label(
                        egui::RichText::new(format!("目录：{}", settings.file_dir.display()))
                            .size(theme.font_size_caption())
                            .color(theme.color_form_hint()),
                    );
                    ui.horizontal(|ui| {
                        ui.label("日志文件");
                        let prev = self.selected_file;
                        egui::ComboBox::from_id_source("audit_log_file_pick")
                            .selected_text(
                                self.log_files
                                    .get(self.selected_file)
                                    .and_then(|p| p.file_name())
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("（无）"),
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
                        if ui.button("刷新").clicked() {
                            self.log_files = list_audit_log_files(&settings.file_dir);
                            reload = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("搜索");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.search_query)
                                .hint_text("action / host / resource…")
                                .desired_width(180.0),
                        );
                        ui.label("类别");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.filter_category)
                                .hint_text("session / vault / …")
                                .desired_width(100.0),
                        );
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
                            if chrome::modal_secondary_button(ui, th, "关闭").clicked() {
                                should_close = true;
                            }
                            if chrome::modal_secondary_button(ui, th, "复制全部").clicked() {
                                copy_all = true;
                            }
                        });
                    });
                });
            });
        if reload {
            self.reload_content(settings);
        }
        if copy_all {
            let _ = Clipboard::new().and_then(|mut c| c.set_text(display_for_view));
        }
        if should_close {
            self.open = false;
        }
    }
}
