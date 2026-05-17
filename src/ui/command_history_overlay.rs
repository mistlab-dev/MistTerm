//! Ctrl+R 命令历史搜索覆盖层

use crate::core::command_history::{CommandHistory, HistoryEntry};
use crate::ui::theme::Theme;
use eframe::egui;

pub struct CommandHistoryOverlay {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub match_index: usize,
}

impl Default for CommandHistoryOverlay {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            selected: 0,
            match_index: 0,
        }
    }
}

impl CommandHistoryOverlay {
    pub fn open_new(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.match_index = 0;
    }

    pub fn cycle_match(&mut self, result_len: usize) {
        if result_len == 0 {
            self.match_index = 0;
            self.selected = 0;
            return;
        }
        self.match_index = (self.match_index + 1) % result_len;
        self.selected = self.match_index;
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        history: &CommandHistory,
        term_rect: egui::Rect,
    ) -> CommandHistoryAction {
        if !self.open {
            return CommandHistoryAction::None;
        }
        if !history.is_loaded() {
            return CommandHistoryAction::None;
        }
        let results = history.search(&self.query, true);
        if self.selected >= results.len() && !results.is_empty() {
            self.selected = results.len() - 1;
        }
        if self.match_index >= results.len() && !results.is_empty() {
            self.match_index = 0;
            self.selected = 0;
        }
        let mut action = CommandHistoryAction::None;

        ctx.input(|i| {
            if i.key_pressed(egui::Key::Escape) {
                action = CommandHistoryAction::Close;
            }
            if i.key_pressed(egui::Key::ArrowDown) {
                if !results.is_empty() {
                    self.selected = (self.selected + 1).min(results.len() - 1);
                    self.match_index = self.selected;
                }
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                self.selected = self.selected.saturating_sub(1);
                self.match_index = self.selected;
            }
            if i.key_pressed(egui::Key::Enter) {
                if let Some(entry) = results.get(self.selected) {
                    action = CommandHistoryAction::Apply(entry.command.clone());
                }
            }
        });

        let panel_h = (term_rect.height() * 0.45).clamp(180.0, 420.0);
        let inner = egui::Rect::from_min_max(
            egui::pos2(term_rect.left() + 12.0, term_rect.top() + 12.0),
            egui::pos2(term_rect.right() - 12.0, term_rect.top() + panel_h),
        );

        egui::Area::new(egui::Id::new("command_history_overlay"))
            .fixed_pos(inner.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_clip_rect(inner);
                egui::Frame::popup(&ctx.style())
                    .fill(theme.bg_window_color())
                    .stroke(egui::Stroke::new(1.0, theme.accent_alpha(80)))
                    .rounding(8.0)
                    .inner_margin(egui::vec2(12.0, 10.0))
                    .show(ui, |ui| {
                        ui.set_width(inner.width() - 24.0);
                        ui.label(
                            egui::RichText::new("搜索命令历史 (Ctrl+R 继续 · Esc 关闭)")
                                .size(theme.font_size_small())
                                .color(theme.fg_low_color()),
                        );
                        ui.add_space(6.0);
                        let te = egui::TextEdit::singleline(&mut self.query)
                            .hint_text("❯")
                            .desired_width(f32::INFINITY)
                            .font(egui::FontId::monospace(theme.font_size_normal()));
                        let resp = ui.add(te);
                        if self.open {
                            resp.request_focus();
                        }
                        ui.add_space(8.0);
                        egui::ScrollArea::vertical()
                            .max_height(inner.height() - 90.0)
                            .show(ui, |ui| {
                                if results.is_empty() {
                                    ui.label(
                                        egui::RichText::new("无匹配记录")
                                            .color(theme.fg_low_color()),
                                    );
                                } else {
                                    for (i, entry) in results.iter().enumerate() {
                                        let row = row_button(ui, theme, entry, i == self.selected);
                                        if row.clicked() {
                                            action = CommandHistoryAction::Apply(entry.command.clone());
                                        }
                                        if row.hovered() {
                                            ui.horizontal(|ui| {
                                                ui.add_space(ui.available_width() - 28.0);
                                                if ui.small_button("🗑").on_hover_text("从历史删除").clicked() {
                                                    action = CommandHistoryAction::Delete(entry.command.clone());
                                                }
                                            });
                                        }
                                    }
                                }
                            });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("共 {} 条结果", results.len()))
                                    .size(theme.font_size_small())
                                    .color(theme.fg_low_color()),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("Esc 关闭").clicked() {
                                    action = CommandHistoryAction::Close;
                                }
                            });
                        });
                    });
            });

        action
    }
}

fn row_button(ui: &mut egui::Ui, theme: &Theme, entry: &HistoryEntry, selected: bool) -> egui::Response {
    let suffix = entry
        .session_name
        .as_deref()
        .map(|n| format!(" # {}", n))
        .unwrap_or_default();
    let cmd = entry.display_command();
    let status = if entry.success { "" } else { " · 失败" };
    let label = format!("{}{}{}", cmd, suffix, status);
    let fill = if selected {
        theme.accent_alpha(51)
    } else {
        egui::Color32::TRANSPARENT
    };
    let text_color = if entry.success {
        theme.fg_high_color()
    } else {
        theme.amber_color()
    };
    ui.add(
        egui::Button::new(
            egui::RichText::new(if selected {
                format!("→ {}", label)
            } else {
                format!("  {}", label)
            })
            .font(egui::FontId::monospace(theme.font_size_normal()))
            .color(text_color),
        )
        .fill(fill)
        .stroke(egui::Stroke::NONE)
        .frame(false),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandHistoryAction {
    None,
    Close,
    Apply(String),
    Delete(String),
}
