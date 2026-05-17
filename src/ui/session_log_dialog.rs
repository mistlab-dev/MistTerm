//! 会话日志浏览弹窗

use crate::core::session_logger::{
    list_session_log_files, log_text_for_display, read_log_tail, SessionLogSettings,
};
use crate::ui::chrome;
use crate::ui::layout_util;
use crate::ui::theme::Theme;
use eframe::egui;
use std::path::PathBuf;

pub struct SessionLogDialog {
    pub open: bool,
    pub session_id: String,
    pub session_name: String,
    log_files: Vec<PathBuf>,
    selected_file: usize,
    content: String,
}

impl Default for SessionLogDialog {
    fn default() -> Self {
        Self {
            open: false,
            session_id: String::new(),
            session_name: String::new(),
            log_files: Vec::new(),
            selected_file: 0,
            content: String::new(),
        }
    }
}

impl SessionLogDialog {
    pub fn open_for(&mut self, session_id: &str, session_name: &str, settings: &SessionLogSettings) {
        self.session_id = session_id.to_string();
        self.session_name = session_name.to_string();
        self.log_files = list_session_log_files(&settings.base_dir, session_id);
        self.selected_file = 0;
        self.reload_content(settings);
        self.open = true;
    }

    fn reload_content(&mut self, settings: &SessionLogSettings) {
        self.content.clear();
        if let Some(path) = self.log_files.get(self.selected_file) {
            match read_log_tail(path, 256 * 1024) {
                Ok(text) if text.trim().is_empty() => {
                    self.content =
                        "日志文件存在但尚无内容；请在终端产生输出后再查看。".to_string();
                }
                Ok(text) => {
                    let cleaned = log_text_for_display(&text);
                    self.content = if cleaned.trim().is_empty() {
                        "日志主要为终端控制符，清洗后无可见文本。请直接在终端查看。".to_string()
                    } else {
                        cleaned
                    };
                }
                Err(e) => {
                    self.content = format!("无法读取日志：{}\n路径：{}", e, path.display());
                }
            }
        } else if self.log_files.is_empty() {
            self.content = format!(
                "暂无日志文件。\n目录：{}",
                settings.base_dir.join(&self.session_id).display()
            );
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme, settings: &SessionLogSettings) {
        if !self.open {
            return;
        }
        let mut should_close = false;
        let mut reload = false;
        egui::Window::new("session_log_viewer")
            .open(&mut self.open)
            .title_bar(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .movable(true)
            .resizable(true)
            .collapsible(false)
            .default_size(layout_util::modal_edit_size(ctx))
            .frame(chrome::modal_window_frame(theme))
            .show(ctx, |ui| {
                chrome::modal_content_frame(theme).show(ui, |ui| {
                    let title = format!("会话日志 — {}", self.session_name);
                    if chrome::modal_header(ui, theme, &title, theme.font_size_fragment_dialog_body()) {
                        should_close = true;
                    }
                    ui.label(
                        egui::RichText::new(
                            "本地录制的终端输出（非实时）。已去除颜色控制符；完整原始内容见日志文件。",
                        )
                        .size(theme.font_size_small())
                        .color(theme.fg_low_color()),
                    );
                    if !self.log_files.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label("日期：");
                            let names: Vec<String> = self
                                .log_files
                                .iter()
                                .filter_map(|p| {
                                    p.file_stem().and_then(|s| s.to_str().map(str::to_string))
                                })
                                .collect();
                            let mut label = names
                                .get(self.selected_file)
                                .cloned()
                                .unwrap_or_default();
                            egui::ComboBox::from_id_source("session_log_file")
                                .selected_text(&label)
                                .show_ui(ui, |ui| {
                                    crate::ui::chrome::apply_menu_popup_style(ui, theme);
                                    for (i, name) in names.iter().enumerate() {
                                        if ui.selectable_label(self.selected_file == i, name).clicked() {
                                            self.selected_file = i;
                                            label = name.clone();
                                            reload = true;
                                        }
                                    }
                                });
                            if ui.button("刷新").clicked() {
                                self.log_files =
                                    list_session_log_files(&settings.base_dir, &self.session_id);
                                reload = true;
                            }
                        });
                    }
                    ui.add_space(theme.spacing_sm());
                    let log_h = 280.0_f32;
                    egui::Frame::none()
                        .fill(theme.color_subtle_inset_fill())
                        .rounding(4.0)
                        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(log_h)
                                .stick_to_bottom(false)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(if self.content.is_empty() {
                                                "（无内容）"
                                            } else {
                                                self.content.as_str()
                                            })
                                            .font(egui::FontId::monospace(
                                                theme.font_size_small(),
                                            ))
                                            .color(theme.fg_medium_color()),
                                        )
                                        .wrap(true),
                                    );
                                });
                        });
                    ui.add_space(theme.spacing_md());
                    chrome::modal_footer_actions(ui, theme, |ui, th| {
                        if chrome::modal_secondary_button(ui, th, "关闭").clicked() {
                            should_close = true;
                        }
                    });
                });
            });
        if reload {
            self.reload_content(settings);
        }
        if should_close {
            self.open = false;
        }
    }
}
