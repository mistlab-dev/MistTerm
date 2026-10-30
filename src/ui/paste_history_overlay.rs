//! 粘贴历史面板：列出最近成功送入 PTY 的粘贴，回车或双击再次粘贴。

use crate::core::paste_history::{
    format_byte_len, PasteEntry, PasteHistory, PASTE_ENTRY_MAX_BYTES,
};
use crate::ui::theme::Theme;
use eframe::egui;

const PANEL_W: f32 = 560.0;
const PANEL_H: f32 = 420.0;
const ANCHOR_INSET: f32 = 12.0;

pub struct PasteHistoryOverlay {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    focus_search_once: bool,
}

impl Default for PasteHistoryOverlay {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            selected: 0,
            focus_search_once: false,
        }
    }
}

impl PasteHistoryOverlay {
    pub fn open_new(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.focus_search_once = true;
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        history: &PasteHistory,
        term_rect: egui::Rect,
        remembering: bool,
    ) -> PasteHistoryAction {
        if !self.open {
            return PasteHistoryAction::None;
        }

        let results = filtered_entries(history, &self.query);
        if self.selected >= results.len() && !results.is_empty() {
            self.selected = results.len() - 1;
        }

        let mut action = PasteHistoryAction::None;
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Escape) {
                action = PasteHistoryAction::Close;
            }
            if i.key_pressed(egui::Key::ArrowDown) && !results.is_empty() {
                self.selected = (self.selected + 1).min(results.len() - 1);
            }
            if i.key_pressed(egui::Key::ArrowUp) {
                self.selected = self.selected.saturating_sub(1);
            }
            if i.key_pressed(egui::Key::Enter) {
                if let Some(entry) = results.get(self.selected) {
                    action = PasteHistoryAction::Apply(entry.text.clone());
                }
            }
        });

        let content_w = PANEL_W - 32.0;
        let mut open = self.open;
        let accel = crate::platform::paste_history_accel();
        let modal_sz = egui::vec2(PANEL_W, PANEL_H);
        crate::ui::chrome::modal_window("paste_history_overlay", theme, ctx)
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
                        crate::i18n::tr(ctx, "Paste history", "粘贴历史"),
                        theme.font_size_prominent(),
                    ) {
                        action = PasteHistoryAction::Close;
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "{}{}{}",
                            crate::i18n::tr(ctx, "Enter or double-click pastes again (", "回车或双击再次粘贴 ("),
                            accel,
                            crate::i18n::tr(ctx, " · Esc closes)", " · Esc 关闭)"),
                        ))
                        .size(theme.font_size_small())
                        .color(theme.text_tertiary()),
                    );
                    if !remembering {
                        ui.add_space(theme.spacing_sm());
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(
                                ctx,
                                "Remembering is off. Saved entries stay here and can be pasted again, but new pastes are not stored.",
                                "记住粘贴历史已关闭。下面仍是之前保存的记录，可以再次粘贴；新的粘贴不会写入。",
                            ))
                            .size(theme.font_size_small())
                            .color(theme.amber_color()),
                        );
                    }
                    ui.add_space(theme.spacing_sm());
                    let search = crate::ui::chrome::search_field(
                        ui,
                        theme,
                        egui::Id::new("paste_history_search"),
                        &mut self.query,
                        crate::i18n::tr(ctx, "Search paste history…", "搜索粘贴历史…"),
                        content_w,
                    );
                    if self.focus_search_once {
                        search.request_focus();
                        self.focus_search_once = false;
                    }
                    ui.add_space(theme.spacing_sm());

                    if !history.is_loaded() {
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(
                                ctx,
                                "Loading paste history…",
                                "正在加载粘贴历史…",
                            ))
                            .color(theme.text_tertiary()),
                        );
                    } else {
                        egui::ScrollArea::vertical()
                            .max_height(PANEL_H - 210.0)
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                ui.set_width(content_w);
                                if results.is_empty() {
                                    let empty = if self.query.trim().is_empty() {
                                        crate::i18n::tr(
                                            ctx,
                                            "No paste history yet. Paste in the terminal and it will show up here to paste again.",
                                            "还没有粘贴历史。在终端粘贴内容后会出现在这里，可再粘一次。",
                                        )
                                    } else {
                                        crate::i18n::tr(
                                            ctx,
                                            "No matches. Try another search.",
                                            "没有匹配的记录。换个关键词试试。",
                                        )
                                    };
                                    ui.label(
                                        egui::RichText::new(empty)
                                            .size(theme.font_size_caption())
                                            .color(theme.text_tertiary()),
                                    );
                                } else {
                                    for (i, entry) in results.iter().enumerate() {
                                        let (row, delete) = paste_row(
                                            ui,
                                            ctx,
                                            theme,
                                            content_w,
                                            entry,
                                            i == self.selected,
                                        );
                                        if delete {
                                            action = PasteHistoryAction::Delete(entry.id);
                                        } else if row.clicked() {
                                            self.selected = i;
                                            if row.double_clicked() {
                                                action = PasteHistoryAction::Apply(entry.text.clone());
                                            }
                                        }
                                    }
                                }
                            });
                    }

                    ui.add_space(theme.spacing_md());
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!(
                                "{}{}{}",
                                crate::i18n::tr(ctx, "Total ", "共 "),
                                results.len(),
                                crate::i18n::tr(ctx, " entries", " 条"),
                            ))
                            .size(theme.font_size_small())
                            .color(theme.text_tertiary()),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let can = results.get(self.selected).is_some();
                            if ui
                                .add(
                                    crate::ui::chrome::modal_primary_button_widget(
                                        theme,
                                        crate::i18n::tr(ctx, "Paste again", "再次粘贴"),
                                    )
                                    .can_activate(can),
                                )
                                .clicked()
                            {
                                if let Some(entry) = results.get(self.selected) {
                                    action = PasteHistoryAction::Apply(entry.text.clone());
                                }
                            }
                        });
                    });
                });
            });

        if !open {
            action = PasteHistoryAction::Close;
        }
        action
    }
}

fn filtered_entries<'a>(history: &'a PasteHistory, query: &str) -> Vec<&'a PasteEntry> {
    let q = query.trim().to_lowercase();
    history
        .entries_newest_first()
        .filter(|entry| q.is_empty() || entry.text.to_lowercase().contains(&q))
        .collect()
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

fn paste_row(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    theme: &Theme,
    width: f32,
    entry: &PasteEntry,
    selected: bool,
) -> (egui::Response, bool) {
    let row_h = 32.0;
    let (row_rect, response) =
        ui.allocate_exact_size(egui::vec2(width, row_h), egui::Sense::click());
    let rounding = theme.radius_list_item();
    if selected {
        ui.painter()
            .rect_filled(row_rect, rounding, theme.color_list_row_selected_fill());
    } else if response.hovered() {
        ui.painter()
            .rect_filled(row_rect, rounding, theme.list_row_hover_bg());
    }

    let time = entry
        .pasted_at_local()
        .map(|t| t.format("%H:%M").to_string())
        .unwrap_or_else(|| "—".to_string());
    let preview = entry.preview_oneline(72);
    let mut delete_clicked = false;
    ui.scope_builder(egui::UiBuilder::new().max_rect(row_rect.shrink2(egui::vec2(8.0), 4.0), |ui| {
        ui.horizontal(|ui| {
            ui.set_width(width - 16.0);
            ui.label(
                egui::RichText::new(time)
                    .size(theme.font_size_small())
                    .color(theme.text_tertiary()),
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(preview)
                        .font(egui::FontId::monospace(theme.font_size_normal()))
                        .color(theme.text_primary()),
                )
                .truncate(),
            );
            if entry.truncated {
                let cap_kb = PASTE_ENTRY_MAX_BYTES / 1024;
                let badge = crate::i18n::tr(ctx, "truncated", "已截断");
                ui.label(
                    egui::RichText::new(badge)
                        .size(theme.font_size_small())
                        .color(theme.amber_color()),
                )
                .on_hover_text(format!(
                    "{} {} · {} {} KB. {}",
                    crate::i18n::tr(ctx, "Original", "原文"),
                    format_byte_len(entry.original_bytes),
                    crate::i18n::tr(ctx, "history kept the first", "历史只保留前"),
                    cap_kb,
                    crate::i18n::tr(
                        ctx,
                        "Pasting again sends that prefix only, not this note.",
                        "再次粘贴只发送这段前缀，不会带上这条说明。",
                    ),
                ));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if response.hovered()
                    && crate::ui::chrome::icon_button(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Trash,
                        theme.color_body_text_muted(),
                    )
                    .on_hover_text(crate::i18n::tr(ctx, "Remove from history", "从历史删除"))
                    .clicked()
                {
                    delete_clicked = true;
                }
            });
        });
    });
    (response, delete_clicked)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteHistoryAction {
    None,
    Close,
    Apply(String),
    Delete(u64),
}
