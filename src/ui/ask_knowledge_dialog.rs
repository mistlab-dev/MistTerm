//! 「问：我们怎么……」知识检索对话框（先团队知识，再可选模型兜底）。

use eframe::egui;

use crate::core::{KnowledgeHit, KnowledgeSource};
use crate::ui::{chrome, layout_util};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AskKnowledgeUiAction {
    #[default]
    None,
    Search,
    UseHit(usize),
    AskModel,
}

pub fn show_ask_knowledge_modal(
    ctx: &egui::Context,
    theme: &Theme,
    open: &mut bool,
    query: &mut String,
    hits: &[KnowledgeHit],
    searched: bool,
    action: &mut AskKnowledgeUiAction,
) {
    if !*open {
        return;
    }
    *action = AskKnowledgeUiAction::None;
    let mut dialog_open = *open;
    let mut should_close = false;
    let modal_sz = egui::vec2(640.0, 520.0);
    let title = crate::i18n::tr(ctx, "Ask: how do we…", "问：我们怎么……");

    chrome::modal_window("ask_knowledge", theme, ctx)
        .open(&mut dialog_open)
        .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
        .movable(true)
        .resizable(true)
        .default_size(modal_sz)
        .show(ctx, |ui| {
            chrome::modal_content_frame(theme).show(ui, |ui| {
                if chrome::modal_header(ui, theme, title, theme.font_size_modal_title()) {
                    should_close = true;
                }
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ctx,
                        "Search team knowledge first. Model answers are labeled as not team knowledge.",
                        "先检索团队知识；模型回答会标明「非团队知识」。",
                    ))
                    .size(theme.font_size_caption())
                    .color(theme.text_tertiary()),
                );
                ui.add_space(theme.spacing_sm());

                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(query)
                            .desired_width(420.0)
                            .hint_text(crate::i18n::tr(
                                ctx,
                                "e.g. how do we clean logs safely",
                                "例如：我们怎么安全清理日志",
                            )),
                    );
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui
                        .button(crate::i18n::tr(ctx, "Search", "检索"))
                        .clicked()
                        || (enter && !query.trim().is_empty())
                    {
                        *action = AskKnowledgeUiAction::Search;
                    }
                });

                ui.add_space(theme.spacing_sm());
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(layout_util::dialog_scroll_max_height(ctx, 280.0))
                    .show(ui, |ui| {
                        if !searched {
                            ui.label(
                                egui::RichText::new(crate::i18n::tr(
                                    ctx,
                                    "Enter a question and search.",
                                    "输入问题后点击检索。",
                                ))
                                .color(theme.text_tertiary()),
                            );
                        } else if hits.is_empty() {
                            ui.label(
                                egui::RichText::new(crate::i18n::tr(
                                    ctx,
                                    "No team alternative yet.",
                                    "暂无团队替代",
                                ))
                                .color(theme.text_tertiary()),
                            );
                        } else {
                            for (i, hit) in hits.iter().enumerate() {
                                theme.frame_region_panel().show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(source_label(ctx, hit.source))
                                                .strong()
                                                .color(theme.accent_color()),
                                        );
                                        ui.label(
                                            egui::RichText::new(&hit.anchor)
                                                .size(theme.font_size_small())
                                                .color(theme.text_tertiary())
                                                .monospace(),
                                        );
                                    });
                                    ui.label(egui::RichText::new(&hit.title).strong());
                                    let preview: String = hit.body.chars().take(160).collect();
                                    ui.label(
                                        egui::RichText::new(preview)
                                            .size(theme.font_size_caption())
                                            .monospace(),
                                    );
                                    if hit.fragment.is_some()
                                        && ui
                                            .button(crate::i18n::tr(
                                                ctx,
                                                "Use in terminal",
                                                "用到终端",
                                            ))
                                            .clicked()
                                    {
                                        *action = AskKnowledgeUiAction::UseHit(i);
                                    }
                                });
                                ui.add_space(theme.spacing_xs());
                            }
                        }
                    });

                if searched && hits.is_empty() {
                    ui.add_space(theme.spacing_sm());
                    let ask = ui
                        .button(crate::i18n::tr(
                            ctx,
                            "Ask AI instead…",
                            "改用 AI 回答…",
                        ))
                        .on_hover_text(crate::i18n::tr(
                            ctx,
                            "Close this dialog, open the AI panel, and ask the model. The answer will be labeled as not team knowledge.",
                            "关闭本对话框，打开右侧 AI 面板并向模型提问；回答会标明「非团队知识」。",
                        ));
                    if ask.clicked() {
                        *action = AskKnowledgeUiAction::AskModel;
                    }
                }
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

fn source_label(ctx: &egui::Context, source: KnowledgeSource) -> String {
    crate::i18n::tr(ctx, source.label_en(), source.label_zh()).to_string()
}
