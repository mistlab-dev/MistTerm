//! Ctrl+R 命令历史：统一走 [`chrome::modal_window`] 弹窗风格。
//! 支持「仅当前标签」筛选，并勾选多条组成命令片段。

use crate::core::command_history::{CommandHistory, HistoryEntry};
use crate::core::CommandCompleter;
use crate::ui::theme::Theme;
use eframe::egui;
use std::collections::HashSet;

const PANEL_W: f32 = 540.0;
const PANEL_H: f32 = 420.0;
const ANCHOR_INSET: f32 = 12.0;

pub struct CommandHistoryOverlay {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub match_index: usize,
    /// 默认开：只看活动标签对应会话的历史。
    pub filter_active_tab: bool,
    /// 勾选的历史条目键：`executed_at|command`
    checked: HashSet<String>,
    /// 组成片段前可改标题。
    compose_title: String,
    /// 用户是否已手改过标题（避免勾选变化时覆盖）。
    compose_title_customized: bool,
    /// 打开后只抢一次搜索焦点，避免挡住标题编辑。
    focus_search_once: bool,
}

impl Default for CommandHistoryOverlay {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            selected: 0,
            match_index: 0,
            filter_active_tab: true,
            checked: HashSet::new(),
            compose_title: String::new(),
            compose_title_customized: false,
            focus_search_once: false,
        }
    }
}

impl CommandHistoryOverlay {
    pub fn open_new(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.match_index = 0;
        self.filter_active_tab = true;
        self.checked.clear();
        self.compose_title.clear();
        self.compose_title_customized = false;
        self.focus_search_once = true;
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
        active_session_id: Option<&str>,
        active_session_name: Option<&str>,
    ) -> CommandHistoryAction {
        if !self.open {
            return CommandHistoryAction::None;
        }
        if !history.is_loaded() {
            return CommandHistoryAction::None;
        }

        let session_filter = if self.filter_active_tab {
            active_session_id
        } else {
            None
        };
        let results = history.search_filtered(&self.query, true, session_filter);
        let fuzzy_suggestions = if results.is_empty()
            && !self.query.trim().is_empty()
            && session_filter.is_none()
        {
            CommandCompleter::new(history)
                .complete(&self.query, 8)
                .into_iter()
                .map(|c| c.command)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        if self.selected >= results.len() && !results.is_empty() {
            self.selected = results.len() - 1;
        }
        let display_len = if results.is_empty() {
            fuzzy_suggestions.len()
        } else {
            results.len()
        };
        if self.selected >= display_len && display_len > 0 {
            self.selected = display_len - 1;
        }
        if self.match_index >= display_len && display_len > 0 {
            self.match_index = 0;
            self.selected = 0;
        }

        let title_id = egui::Id::new("cmd_history_compose_title");
        let title_focused = ctx.memory(|m| m.has_focus(title_id));
        let mut action = CommandHistoryAction::None;

        ctx.input(|i| {
            if i.key_pressed(egui::Key::Escape) {
                action = CommandHistoryAction::Close;
            }
            if !title_focused {
                if i.key_pressed(egui::Key::ArrowDown) {
                    if display_len > 0 {
                        self.selected = (self.selected + 1).min(display_len - 1);
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
                    } else if let Some(cmd) = fuzzy_suggestions.get(self.selected) {
                        action = CommandHistoryAction::Apply(cmd.clone());
                    }
                }
            }
        });

        let content_w = PANEL_W - 32.0;
        let cols = HistoryTableCols::from_content_w(content_w);
        let mut open = self.open;
        let checked_count = results
            .iter()
            .filter(|e| self.checked.contains(&entry_key(e)))
            .count();

        let modal_sz = egui::vec2(PANEL_W, PANEL_H);
        crate::ui::chrome::modal_window("command_history_overlay", theme, ctx)
            .open(&mut open)
            .default_pos(panel_anchor(term_rect, ctx))
            .movable(true)
            .resizable(false)
            .default_size(modal_sz)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    ui.set_width(content_w);
                    ui.set_max_width(content_w);
                    if crate::ui::chrome::modal_header(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Command history", "命令历史"),
                        theme.font_size_prominent(),
                    ) {
                        action = CommandHistoryAction::Close;
                    }

                    ui.label(
                        egui::RichText::new(format!(
                            "{}{}{}",
                            crate::i18n::tr(
                                ctx,
                                "Search (",
                                "搜索 (",
                            ),
                            crate::platform::terminal_history_accel(),
                            crate::i18n::tr(
                                ctx,
                                " next · Esc close · check ≥2 to compose)",
                                " 继续 · Esc 关闭 · 勾选≥2 条组成片段)",
                            ),
                        ))
                        .size(theme.font_size_small())
                        .color(theme.text_tertiary()),
                    );
                    ui.add_space(theme.spacing_sm());

                    let base = crate::i18n::tr(ctx, "Active tab only", "仅当前标签");
                    let filter_label = match active_session_name.filter(|s| !s.is_empty()) {
                        Some(name) => format!("{base}（{name}）"),
                        None => base.to_string(),
                    };
                    let filter_enabled = active_session_id.is_some();
                    ui.add_enabled_ui(filter_enabled, |ui| {
                        if crate::ui::chrome::form_checkbox(
                            ui,
                            theme,
                            &mut self.filter_active_tab,
                            &filter_label,
                        )
                        .changed()
                        {
                            self.selected = 0;
                            self.match_index = 0;
                        }
                    });
                    if !filter_enabled {
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(
                                ctx,
                                "(no active session)",
                                "（无活动会话）",
                            ))
                            .size(theme.font_size_small())
                            .color(theme.text_tertiary()),
                        );
                    }
                    ui.add_space(theme.spacing_sm());

                    let search_resp = crate::ui::chrome::search_field(
                        ui,
                        theme,
                        egui::Id::new("cmd_history_search"),
                        &mut self.query,
                        crate::i18n::tr(ctx, "Search history…", "搜索历史命令…"),
                        content_w,
                    );
                    if self.focus_search_once {
                        search_resp.request_focus();
                        self.focus_search_once = false;
                    }
                    ui.add_space(theme.spacing_sm());
                    column_header(ui, ctx, theme, cols);
                    ui.add_space(2.0);
                    egui::ScrollArea::vertical()
                        .max_height(PANEL_H - 220.0)
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            ui.set_width(content_w);
                            if results.is_empty() {
                                if !fuzzy_suggestions.is_empty() {
                                    ui.label(
                                        egui::RichText::new(crate::i18n::tr(
                                            ctx,
                                            "Suggested commands",
                                            "建议命令",
                                        ))
                                        .size(theme.font_size_small())
                                        .color(theme.text_tertiary()),
                                    );
                                    ui.add_space(4.0);
                                    for (i, cmd) in fuzzy_suggestions.iter().enumerate() {
                                        let (row, _) = fuzzy_suggestion_row(
                                            ui,
                                            ctx,
                                            theme,
                                            cmd,
                                            i == self.selected,
                                        );
                                        if row.clicked() {
                                            action = CommandHistoryAction::Apply(cmd.clone());
                                        }
                                    }
                                } else {
                                    let empty_msg = if !self.query.trim().is_empty() {
                                        crate::i18n::tr(
                                            ctx,
                                            "No matches. Try another search.",
                                            "没有匹配的记录。换个关键词试试。",
                                        )
                                    } else if self.filter_active_tab
                                        && active_session_id.is_some()
                                    {
                                        crate::i18n::tr(
                                            ctx,
                                            "No history for this tab yet. Run commands here and reopen, or turn off “Active tab only” to see all.",
                                            "当前标签还没有命令历史。在此会话里执行命令后会出现在这里；也可取消「仅当前标签」查看全部。",
                                        )
                                    } else {
                                        crate::i18n::tr(
                                            ctx,
                                            "No command history yet. After you run commands in a session, search, rerun, or compose snippets here.",
                                            "还没有命令历史。连接会话并执行命令后，可在这里搜索、重跑或勾选组成片段。",
                                        )
                                    };
                                    ui.label(
                                        egui::RichText::new(empty_msg)
                                            .size(theme.font_size_caption())
                                            .color(theme.text_tertiary()),
                                    );
                                }
                            } else {
                                for (i, entry) in results.iter().enumerate() {
                                    let key = entry_key(entry);
                                    let mut checked = self.checked.contains(&key);
                                    let (row, delete, check_clicked) = history_row(
                                        ui,
                                        ctx,
                                        theme,
                                        cols,
                                        entry,
                                        i == self.selected,
                                        &mut checked,
                                    );
                                    if check_clicked {
                                        if checked {
                                            self.checked.insert(key);
                                        } else {
                                            self.checked.remove(&entry_key(entry));
                                        }
                                        if !self.compose_title_customized {
                                            let n = self.checked.len();
                                            if n >= 2 {
                                                let first_cmd = results
                                                    .iter()
                                                    .filter(|e| self.checked.contains(&entry_key(e)))
                                                    .map(|e| e.command.as_str())
                                                    .next();
                                                self.compose_title = default_compose_title(
                                                    ctx,
                                                    first_cmd,
                                                    n,
                                                );
                                            } else {
                                                self.compose_title.clear();
                                            }
                                        }
                                    } else if delete {
                                        action =
                                            CommandHistoryAction::Delete(entry.command.clone());
                                        self.checked.remove(&entry_key(entry));
                                    } else if row.clicked() {
                                        self.selected = i;
                                        self.match_index = i;
                                        if row.double_clicked() {
                                            action = CommandHistoryAction::Apply(
                                                entry.command.clone(),
                                            );
                                        }
                                    }
                                }
                            }
                        });

                    ui.add_space(theme.spacing_md());
                    if checked_count >= 1 {
                        crate::ui::chrome::form_field_label(
                            ui,
                            theme,
                            crate::i18n::tr(ctx, "Snippet title", "片段标题"),
                        );
                        ui.add_space(2.0);
                        let before = self.compose_title.clone();
                        crate::ui::chrome::form_singleline_field(
                            ui,
                            theme,
                            title_id,
                            &mut self.compose_title,
                            crate::i18n::tr(
                                ctx,
                                "Title for composed snippet",
                                "组合片段标题",
                            ),
                            content_w,
                            false,
                        );
                        if self.compose_title != before {
                            self.compose_title_customized = true;
                        }
                        ui.add_space(theme.spacing_sm());
                    }

                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{}{}{} · {}{}",
                                crate::i18n::tr(ctx, "Total ", "共 "),
                                if results.is_empty() {
                                    fuzzy_suggestions.len()
                                } else {
                                    results.len()
                                },
                                crate::i18n::tr(ctx, " results", " 条结果"),
                                crate::i18n::tr(ctx, "selected ", "已选 "),
                                checked_count,
                            ))
                            .size(theme.font_size_small())
                            .color(theme.text_tertiary()),
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let can_compose = checked_count >= 2;
                                let compose_resp = ui.add(
                                    crate::ui::chrome::modal_primary_button_widget(
                                        theme,
                                        crate::i18n::tr(ctx, "Compose snippet", "组成片段"),
                                    )
                                    .can_activate(can_compose),
                                );
                                if !can_compose {
                                    compose_resp.on_hover_text(crate::i18n::tr(
                                        ctx,
                                        "Select at least 2 commands to compose a snippet",
                                        "至少勾选 2 条命令才能组成片段",
                                    ));
                                } else if compose_resp.clicked() {
                                    let mut picked: Vec<HistoryEntry> = results
                                        .iter()
                                        .filter(|e| self.checked.contains(&entry_key(e)))
                                        .cloned()
                                        .collect();
                                    picked.sort_by_key(|e| e.executed_at);
                                    let commands: Vec<String> =
                                        picked.into_iter().map(|e| e.command).collect();
                                    let title = if self.compose_title.trim().is_empty() {
                                        default_compose_title(
                                            ctx,
                                            commands.first().map(|s| s.as_str()),
                                            commands.len(),
                                        )
                                    } else {
                                        self.compose_title.trim().to_string()
                                    };
                                    action = CommandHistoryAction::ComposeFragment {
                                        title,
                                        commands,
                                    };
                                }
                            },
                        );
                    });
                });
            });

        if !open {
            action = CommandHistoryAction::Close;
        }
        if matches!(action, CommandHistoryAction::ComposeFragment { .. }) {
            self.checked.clear();
            self.compose_title.clear();
            self.compose_title_customized = false;
        }

        action
    }
}

fn entry_key(e: &HistoryEntry) -> String {
    format!("{}|{}", e.executed_at, e.command)
}

fn default_compose_title(
    ctx: &egui::Context,
    first_command: Option<&str>,
    n: usize,
) -> String {
    let preview = first_command
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|c| {
            let joined = c.split_whitespace().collect::<Vec<_>>().join(" ");
            let mut it = joined.chars();
            let head: String = it.by_ref().take(40).collect();
            if it.next().is_some() {
                format!("{head}…")
            } else {
                head
            }
        });
    match (crate::i18n::language(ctx), preview) {
        (crate::i18n::UiLanguage::Zh, Some(cmd)) => format!("历史命令：{cmd}"),
        (crate::i18n::UiLanguage::Zh, None) => format!("历史命令：{n} 条"),
        (_, Some(cmd)) => format!("History: {cmd}"),
        (_, None) => format!("History: {n} commands"),
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
    const MARKER_W: f32 = 28.0;
    const SESSION_W: f32 = 72.0;
    const STATUS_W: f32 = 48.0;
    const ACTIONS_W: f32 = 28.0;
    const ROW_H: f32 = 28.0;
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
        ctx.content_rect().shrink(ANCHOR_INSET).left_top()
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

fn fuzzy_suggestion_row(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    theme: &Theme,
    command: &str,
    selected: bool,
) -> (egui::Response, bool) {
    let row_h = HistoryTableCols::ROW_H;
    let (row_rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), egui::Sense::click());
    let rounding = theme.radius_list_item();
    if selected {
        ui.painter()
            .rect_filled(row_rect, rounding, theme.color_list_row_selected_fill());
    } else if response.hovered() {
        ui.painter()
            .rect_filled(row_rect, rounding, theme.list_row_hover_bg());
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(row_rect.shrink2(egui::vec2(8.0, 4.0))), |ui| {
        ui.label(
            egui::RichText::new(format!(
                "{} {}",
                crate::i18n::tr(ctx, "Suggest", "建议"),
                command
            ))
            .color(theme.text_primary()),
        );
    });
    (response, false)
}

fn history_row(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    theme: &Theme,
    cols: HistoryTableCols,
    entry: &HistoryEntry,
    selected: bool,
    checked: &mut bool,
) -> (egui::Response, bool, bool) {
    let row_h = HistoryTableCols::ROW_H;
    let (row_rect, response) =
        ui.allocate_exact_size(egui::vec2(cols.total, row_h), egui::Sense::click());
    let rounding = theme.radius_list_item();
    if selected || *checked {
        ui.painter()
            .rect_filled(row_rect, rounding, theme.color_list_row_selected_fill());
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
    let mut check_clicked = false;
    ui.scope_builder(egui::UiBuilder::new().max_rect(row_rect), |ui| {
        HistoryTableCols::paint_row_strip(ui, cols, row_h, |cell, col| match col {
            0 => {
                let before = *checked;
                let id = egui::Id::new(("cmd_hist_check", entry.executed_at, &entry.command));
                if crate::ui::chrome::form_checkbox_with_id(cell, theme, id, checked, "")
                    .changed()
                {
                    check_clicked = true;
                }
                if *checked != before {
                    check_clicked = true;
                }
            }
            1 => {
                cell.add(
                    egui::Label::new(
                        egui::RichText::new(entry.display_command())
                            .font(egui::FontId::monospace(theme.font_size_normal()))
                            .color(cmd_color),
                    )
                    .truncate(),
                );
            }
            2 => {
                cell.add(
                    egui::Label::new(
                        egui::RichText::new(session_label)
                            .size(theme.font_size_small())
                            .color(theme.text_secondary()),
                    )
                    .truncate(),
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
    (response, delete_clicked, check_clicked)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandHistoryAction {
    None,
    Close,
    Apply(String),
    Delete(String),
    /// 多条历史按时间正序拼成一条多行片段。
    ComposeFragment {
        title: String,
        commands: Vec<String>,
    },
}
