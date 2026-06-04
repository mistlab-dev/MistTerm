//! 会话日志浏览弹窗

use crate::core::session_logger::{
    list_session_log_files, log_text_for_display, read_log_tail, SessionLogSettings,
    LOG_TAIL_READ_BYTES,
};
use crate::ui::chrome;
use crate::ui::layout_util;
use crate::ui::theme::Theme;
use arboard::Clipboard;
use eframe::egui;
use std::path::PathBuf;

pub struct SessionLogDialog {
    pub open: bool,
    pub session_id: String,
    pub session_name: String,
    log_files: Vec<PathBuf>,
    selected_file: usize,
    content: String,
    search_query: String,
    pending_send_to_ai: bool,
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
            search_query: String::new(),
            pending_send_to_ai: false,
        }
    }
}

impl SessionLogDialog {
    pub fn take_pending_send_to_ai(&mut self) -> bool {
        let v = self.pending_send_to_ai;
        self.pending_send_to_ai = false;
        v
    }

    pub fn content_for_ai(&self) -> Option<String> {
        let body = self.filtered_content();
        if body.trim().is_empty() {
            return None;
        }
        let header = if self.session_name.trim().is_empty() {
            format!("Session ID: {}", self.session_id)
        } else {
            format!("Session: {}", self.session_name)
        };
        Some(format!("{header}\n\n{body}"))
    }
    pub fn open_for(
        &mut self,
        ctx: &egui::Context,
        session_id: &str,
        session_name: &str,
        settings: &SessionLogSettings,
    ) {
        self.session_id = session_id.to_string();
        self.session_name = session_name.to_string();
        self.log_files = list_session_log_files(&settings.base_dir, session_id);
        self.selected_file = 0;
        self.search_query.clear();
        self.reload_content(ctx, settings);
        self.open = true;
    }

    fn reload_content(&mut self, ctx: &egui::Context, settings: &SessionLogSettings) {
        self.content.clear();
        if let Some(path) = self.log_files.get(self.selected_file) {
            match read_log_tail(path, LOG_TAIL_READ_BYTES) {
                Ok(text) if text.trim().is_empty() => {
                    self.content = crate::i18n::tr(
                        ctx,
                        "The log file exists but is empty; produce terminal output before viewing.",
                        "日志文件存在但尚无内容；请在终端产生输出后再查看。",
                    )
                    .to_string();
                }
                Ok(text) => {
                    let cleaned = log_text_for_display(&text);
                    self.content = if cleaned.trim().is_empty() {
                        crate::i18n::tr(
                            ctx,
                            "Logs are mostly terminal control sequences; stripping left nothing visible—check the terminal or raw log file.",
                            "日志主要为终端控制符，清洗后无可见文本。请直接在终端查看。",
                        )
                        .to_string()
                    } else {
                        cleaned
                    };
                }
                Err(e) => {
                    self.content = format!(
                        "{}{}\n{}{}",
                        crate::i18n::tr(ctx, "Failed to read log: ", "无法读取日志："),
                        e,
                        crate::i18n::tr(ctx, "Path: ", "路径："),
                        path.display()
                    );
                }
            }
        } else if self.log_files.is_empty() {
            self.content = format!(
                "{}\n{}{}",
                crate::i18n::tr(ctx, "No session log files yet.", "暂无日志文件。"),
                crate::i18n::tr(ctx, "Directory: ", "目录："),
                settings
                    .base_dir
                    .join(sanitize_dir_hint(&self.session_id))
                    .display()
            );
        }
    }

    fn filtered_content(&self) -> String {
        let q = self.search_query.trim();
        if q.is_empty() {
            return self.content.clone();
        }
        let ql = q.to_lowercase();
        self.content
            .lines()
            .filter(|line| line.to_lowercase().contains(&ql))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme, settings: &SessionLogSettings) {
        if !self.open {
            return;
        }
        let mut should_close = false;
        let mut reload = false;
        let mut copy_all = false;
        let display = self.filtered_content();
        let display_for_view = if display.is_empty() {
            crate::i18n::tr(ctx, "(No matches)", "（无匹配内容）").to_string()
        } else {
            display
        };
        let r = ctx.screen_rect();
        let sw = r.width().max(360.0);
        let sh = r.height().max(280.0);
        let modal_size = egui::vec2(
            (sw * 0.52).clamp(520.0, 860.0),
            (sh * 0.60).clamp(420.0, 700.0),
        );
        let default_pos = egui::pos2(
            r.min.x + (sw - modal_size.x) * 0.5,
            r.min.y + (sh - modal_size.y) * 0.5,
        );
        let title = format!(
            "{}{}",
            crate::i18n::tr(ctx, "Session log — ", "会话日志 — "),
            self.session_name
        );
        chrome::modal_window("session_log_viewer", theme, ctx)
            .id(egui::Id::new("session_log_viewer_window"))
            .open(&mut self.open)
            .default_pos(default_pos)
            .movable(true)
            .resizable(false)
            .fixed_size(modal_size)
            .show(ctx, |ui| {
                chrome::modal_content_frame(theme).show(ui, |ui| {
                    let content_w = layout_util::textedit_width_in_parent(ui, 24.0);
                    ui.set_width(content_w);
                    if chrome::modal_header(ui, theme, &title, chrome::modal_title_font_size(theme)) {
                        should_close = true;
                    }
                    let search_w = content_w;
                    crate::ui::chrome::search_field(
                        ui,
                        theme,
                        egui::Id::new("session_log_search"),
                        &mut self.search_query,
                        crate::i18n::tr(ctx, "Filter log…", "过滤日志内容…"),
                        search_w,
                    );
                    if !self.log_files.is_empty() {
                        ui.horizontal(|ui| {
                            crate::ui::chrome::form_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Date", "日期"),
                            );
                            let names: Vec<String> = self
                                .log_files
                                .iter()
                                .filter_map(|p| {
                                    p.file_name().and_then(|s| s.to_str().map(str::to_string))
                                })
                                .collect();
                            egui::ComboBox::from_id_source("session_log_file")
                                .selected_text(
                                    names
                                        .get(self.selected_file)
                                        .cloned()
                                        .unwrap_or_default(),
                                )
                                .show_ui(ui, |ui| {
                                    crate::ui::chrome::apply_menu_popup_style(ui, theme);
                                    for (i, name) in names.iter().enumerate() {
                                        if ui.selectable_label(self.selected_file == i, name).clicked() {
                                            self.selected_file = i;
                                            reload = true;
                                        }
                                    }
                                });
                            if chrome::panel_action_icon_button(
                                ui,
                                theme,
                                crate::ui::icons::IconId::Refresh,
                                crate::i18n::tr(ctx, "Refresh", "刷新"),
                            )
                                .clicked() {
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
                            ui.set_width(content_w);
                            egui::ScrollArea::vertical()
                                .id_source("session_log_body_scroll")
                                .max_height(log_h)
                                .stick_to_bottom(false)
                                .auto_shrink([true, false])
                                .show(ui, |ui| {
                                    let w = layout_util::textedit_width_in_parent(ui, 12.0);
                                    ui.set_width(w);
                                    chrome::selectable_readonly_monospace(
                                        ui,
                                        theme,
                                        display_for_view.as_str(),
                                        theme.font_size_small(),
                                        w,
                                    );
                                });
                        });
                    ui.add_space(theme.spacing_md());
                    ui.horizontal(|ui| {
                        ui.set_width(content_w);
                        let btn_reserve = theme.size_modal_footer_btn_min_w_secondary() * 2.0
                            + ui.spacing().item_spacing.x * 2.0;
                        let caption_w = (content_w - btn_reserve).max(80.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(caption_w, ui.spacing().interact_size.y),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_max_width(caption_w);
                                ui.add(
                                    egui::Label::new(chrome::rich_caption(
                                        theme,
                                        crate::i18n::tr(
                                            ctx,
                                            "Locally recorded terminal output (not live). Color codes are stripped; see the log file for raw content.",
                                            "本地录制的终端输出（非实时）。已去除颜色控制符；完整原始内容见日志文件。",
                                        ),
                                    ))
                                    .wrap(true),
                                );
                            },
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if chrome::modal_secondary_icon_button(
                                    ui,
                                    theme,
                                    crate::ui::icons::IconId::Copy,
                                    crate::i18n::tr(ctx, "Copy all", "复制全部"),
                                )
                                    .clicked() {
                                    copy_all = true;
                                }
                                if chrome::modal_secondary_icon_button(
                                    ui,
                                    theme,
                                    crate::ui::icons::IconId::Api,
                                    crate::i18n::tr(ctx, "Send to AI", "发送到 AI"),
                                )
                                .on_hover_text(crate::i18n::tr(
                                    ctx,
                                    "Attach log content to the AI panel",
                                    "将会话日志附带至 AI 面板",
                                ))
                                .clicked()
                                {
                                    self.pending_send_to_ai = true;
                                }
                            },
                        );
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

fn sanitize_dir_hint(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}
