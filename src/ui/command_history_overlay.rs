//! Ctrl+R 命令历史搜索：窄浮层，可拖动，列标题对齐。

use crate::core::command_history::{CommandHistory, HistoryEntry};
use crate::ui::theme::Theme;
use eframe::egui;

const PANEL_W: f32 = 480.0;
const PANEL_H: f32 = 340.0;
const ANCHOR_INSET: f32 = 12.0;

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

        let content_w = PANEL_W - 24.0;
        let cols = HistoryTableCols::from_content_w(content_w);
        let mut open = self.open;

        egui::Window::new(crate::i18n::tr(ctx, "Command history", "命令历史"))
            .id(egui::Id::new("command_history_overlay"))
            .open(&mut open)
            .default_pos(panel_anchor(term_rect, ctx))
            .movable(true)
            .resizable(false)
            .collapsible(false)
            .default_size(egui::vec2(PANEL_W, PANEL_H))
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(theme.bg_window_color())
                    .stroke(egui::Stroke::new(1.0, theme.accent_alpha(80)))
                    .rounding(8.0)
                    .inner_margin(egui::vec2(12.0, 10.0)),
            )
            .show(ctx, |ui| {
                ui.set_width(content_w);
                ui.set_max_width(content_w);
                ui.label(
                    egui::RichText::new(format!(
                        "{}{}{}",
                        crate::i18n::tr(
                            ctx,
                            "Search command history (",
                            "搜索命令历史 (",
                        ),
                        crate::platform::terminal_history_accel(),
                        crate::i18n::tr(
                            ctx,
                            " next · Esc to close · drag title bar to move)",
                            " 继续 · Esc 关闭 · 拖动标题栏移动)",
                        ),
                    ))
                    .size(theme.font_size_small())
                    .color(theme.text_tertiary()),
                );
                ui.add_space(6.0);
                let search_resp = crate::ui::chrome::search_field(
                    ui,
                    theme,
                    egui::Id::new("cmd_history_search"),
                    &mut self.query,
                    crate::i18n::tr(ctx, "Search history…", "搜索历史命令…"),
                    content_w,
                );
                if self.open {
                    search_resp.request_focus();
                }
                ui.add_space(8.0);
                column_header(ui, ctx, theme, cols);
                ui.add_space(2.0);
                egui::ScrollArea::vertical()
                    .max_height(PANEL_H - 128.0)
                    .show(ui, |ui| {
                        ui.set_width(content_w);
                        if results.is_empty() {
                            ui.label(
                                egui::RichText::new(crate::i18n::tr(
                                    ctx,
                                    "No matches",
                                    "无匹配记录",
                                ))
                                .color(theme.text_tertiary()),
                            );
                        } else {
                            for (i, entry) in results.iter().enumerate() {
                                let (row, delete) = history_row(
                                    ui,
                                    ctx,
                                    theme,
                                    cols,
                                    entry,
                                    i == self.selected,
                                );
                                if delete {
                                    action =
                                        CommandHistoryAction::Delete(entry.command.clone());
                                } else if row.clicked() {
                                    action =
                                        CommandHistoryAction::Apply(entry.command.clone());
                                }
                            }
                        }
                    });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{}{}{}",
                            crate::i18n::tr(ctx, "Total ", "共 "),
                            results.len(),
                            crate::i18n::tr(ctx, " results", " 条结果"),
                        ))
                        .size(theme.font_size_small())
                        .color(theme.text_tertiary()),
                    );
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if crate::ui::chrome::chrome_small_icon_button(
                                ui,
                                theme,
                                crate::ui::icons::IconId::Close,
                            )
                            .on_hover_text(crate::i18n::tr(ctx, "Esc to close", "Esc 关闭"))
                            .clicked()
                            {
                                action = CommandHistoryAction::Close;
                            }
                        },
                    );
                });
            });

        if !open {
            action = CommandHistoryAction::Close;
        }

        action
    }
}

#[derive(Clone, Copy)]
struct HistoryTableCols {
    total: f32,
    marker: f32,
    command: f32,
    session: f32,
    status: f32,
    actions: f32,
}

impl HistoryTableCols {
    const MARKER_W: f32 = 16.0;
    const SESSION_W: f32 = 72.0;
    const STATUS_W: f32 = 48.0;
    const ACTIONS_W: f32 = 28.0;
    const ROW_H: f32 = 26.0;
    const COLS: usize = 5;

    fn from_content_w(w: f32) -> Self {
        let w = w.max(1.0);
        let marker = Self::MARKER_W;
        let session = Self::SESSION_W;
        let status = Self::STATUS_W;
        let actions = Self::ACTIONS_W;
        let command = (w - marker - session - status - actions).max(96.0);
        Self {
            total: w,
            marker,
            command,
            session,
            status,
            actions,
        }
    }

    fn col_width(self, col: usize) -> f32 {
        match col {
            0 => self.marker,
            1 => self.command,
            2 => self.session,
            3 => self.status,
            _ => self.actions,
        }
    }

    fn col_layout(col: usize) -> egui::Layout {
        if col == 3 {
            egui::Layout::right_to_left(egui::Align::Center)
        } else {
            egui::Layout::left_to_right(egui::Align::Center)
        }
    }

    fn table_cell(
        ui: &mut egui::Ui,
        cols: Self,
        col: usize,
        row_h: f32,
        add: impl FnOnce(&mut egui::Ui),
    ) {
        let w = cols.col_width(col);
        ui.allocate_ui_with_layout(egui::vec2(w, row_h), Self::col_layout(col), |ui| {
            ui.set_width(w);
            ui.set_min_width(w);
            ui.set_max_width(w);
            add(ui);
        });
    }

    fn paint_row_strip(
        ui: &mut egui::Ui,
        cols: Self,
        row_h: f32,
        mut paint_col: impl FnMut(&mut egui::Ui, usize),
    ) {
        ui.set_width(cols.total);
        ui.set_min_width(cols.total);
        ui.set_max_width(cols.total);
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_width(cols.total);
            ui.set_min_width(cols.total);
            for col in 0..Self::COLS {
                Self::table_cell(ui, cols, col, row_h, |cell| paint_col(cell, col));
            }
        });
    }
}

fn panel_anchor(term_rect: egui::Rect, ctx: &egui::Context) -> egui::Pos2 {
    if term_rect.width() > 1.0 && term_rect.height() > 1.0 {
        egui::pos2(
            term_rect.left() + ANCHOR_INSET,
            term_rect.top() + ANCHOR_INSET,
        )
    } else {
        ctx.screen_rect().shrink(ANCHOR_INSET).left_top()
    }
}

fn column_header(ui: &mut egui::Ui, ctx: &egui::Context, theme: &Theme, cols: HistoryTableCols) {
    let cap_font = egui::FontId::proportional(theme.font_size_small());
    let cap_color = theme.text_tertiary();
    ui.allocate_ui_with_layout(
        egui::vec2(cols.total, HistoryTableCols::ROW_H),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            HistoryTableCols::paint_row_strip(ui, cols, HistoryTableCols::ROW_H, |cell, col| {
                let text = match col {
                    0 => return,
                    1 => crate::i18n::tr(ctx, "Command", "命令"),
                    2 => crate::i18n::tr(ctx, "Session", "会话"),
                    3 => crate::i18n::tr(ctx, "Result", "结果"),
                    _ => return,
                };
                cell.label(
                    egui::RichText::new(text)
                        .font(cap_font.clone())
                        .color(cap_color),
                );
            });
        },
    );
    ui.painter().hline(
        ui.min_rect().x_range(),
        ui.min_rect().bottom(),
        egui::Stroke::new(1.0, theme.border_divider_color()),
    );
}

fn history_row(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    theme: &Theme,
    cols: HistoryTableCols,
    entry: &HistoryEntry,
    selected: bool,
) -> (egui::Response, bool) {
    let row_h = HistoryTableCols::ROW_H;
    let (row_rect, response) =
        ui.allocate_exact_size(egui::vec2(cols.total, row_h), egui::Sense::click());
    let rounding = theme.radius_list_item();
    if selected {
        ui.painter()
            .rect_filled(row_rect, rounding, theme.list_row_selected_bg());
    } else if response.hovered() {
        ui.painter()
            .rect_filled(row_rect, rounding, theme.list_row_hover_bg());
    }

    let session_label = entry.session_name.as_deref().unwrap_or("—");
    let (status_text, status_color) = if entry.success {
        (
            crate::i18n::tr(ctx, "OK", "成功"),
            theme.green_color(),
        )
    } else {
        (
            crate::i18n::tr(ctx, "Failed", "失败"),
            theme.amber_color(),
        )
    };
    let cmd_color = if entry.success {
        theme.text_primary()
    } else {
        theme.amber_color()
    };

    let mut delete_clicked = false;
    ui.allocate_ui_at_rect(row_rect, |ui| {
        HistoryTableCols::paint_row_strip(ui, cols, row_h, |cell, col| match col {
            0 => {
                if selected {
                    cell.label(
                        egui::RichText::new("→")
                            .color(theme.accent_color())
                            .size(theme.font_size_small()),
                    );
                }
            }
            1 => {
                cell.add(
                    egui::Label::new(
                        egui::RichText::new(entry.display_command())
                            .font(egui::FontId::monospace(theme.font_size_normal()))
                            .color(cmd_color),
                    )
                    .truncate(true),
                );
            }
            2 => {
                cell.add(
                    egui::Label::new(
                        egui::RichText::new(session_label)
                            .size(theme.font_size_small())
                            .color(theme.text_secondary()),
                    )
                    .truncate(true),
                );
            }
            3 => {
                cell.label(
                    egui::RichText::new(status_text)
                        .size(theme.font_size_small())
                        .color(status_color),
                );
            }
            _ => {
                if response.hovered()
                    && crate::ui::chrome::icon_button(
                        cell,
                        theme,
                        crate::ui::icons::IconId::Trash,
                        theme.color_body_text_muted(),
                    )
                    .on_hover_text(crate::i18n::tr(
                        ctx,
                        "Remove from history",
                        "从历史删除",
                    ))
                    .clicked()
                {
                    delete_clicked = true;
                }
            }
        });
    });
    (response, delete_clicked)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandHistoryAction {
    None,
    Close,
    Apply(String),
    Delete(String),
}
