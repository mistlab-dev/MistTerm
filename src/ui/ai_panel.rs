//! 右侧 AI 面板：对话、附带终端上下文、「用到终端」。

use eframe::egui;
use arboard::Clipboard;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread;

use crate::core::{
    extract_shell_commands, redact_for_ai, AppSettings, ChatMessage,
};
use crate::i18n::{self};
use crate::ui::icons::{self, IconId};
use crate::ui::layout_util;
use crate::ui::markdown_view;
use crate::ui::theme::Theme;

#[derive(Clone)]
struct UiMessage {
    role: &'static str,
    content: String,
    commands: Vec<String>,
}

enum BackgroundJob {
    Chat(Receiver<Result<String, String>>),
    Save(Receiver<Result<String, String>>),
    Test(Receiver<Result<(), String>>),
}

pub struct AiPanel {
    messages: Vec<UiMessage>,
    draft_input: String,
    attached_context: String,
    background: Option<BackgroundJob>,
    busy: bool,
    last_error: Option<String>,
    command_for_terminal: Option<String>,
    settings_key_input: String,
    /// 本地已加密保存 Key 时不再在输入框显示明文
    key_configured_stored: bool,
    test_status: Option<String>,
    /// 输入区旁即时提示（空内容、未启用、请求中等）
    input_status: Option<String>,
    last_panel_slot_rect: Option<egui::Rect>,
}

impl Default for AiPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl AiPanel {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            draft_input: String::new(),
            attached_context: String::new(),
            background: None,
            busy: false,
            last_error: None,
            command_for_terminal: None,
            settings_key_input: String::new(),
            key_configured_stored: false,
            test_status: None,
            input_status: None,
            last_panel_slot_rect: None,
        }
    }

    pub fn attach_context(&mut self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        self.attached_context = redact_for_ai(&text);
    }

    pub fn clear_context(&mut self) {
        self.attached_context.clear();
    }

    pub fn take_command_for_terminal(&mut self) -> Option<String> {
        self.command_for_terminal.take()
    }

    pub fn show_side_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        open: &mut bool,
        right_dock_outer_left: &mut Option<f32>,
        dock_col_w: f32,
    ) {
        if !*open {
            self.last_panel_slot_rect = None;
            return;
        }
        let (def_w, min_w, max_w) = layout_util::right_dock_resize_bounds(dock_col_w);
        let panel = egui::SidePanel::right(layout_util::AI_PANEL_ID)
            .default_width(def_w)
            .min_width(min_w)
            .max_width(max_w)
            .resizable(true)
            .frame(crate::ui::chrome::right_dock_placeholder_frame(theme))
            .show(ctx, |ui| {
                crate::ui::chrome::paint_right_dock_left_gap(ui, theme);
                self.last_panel_slot_rect = Some(ui.max_rect());
                let h = ui.available_height().max(1.0);
                let w = ui.available_width().max(1.0);
                ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
            });
        if let Some(slot) = self.last_panel_slot_rect {
            layout_util::record_right_dock_panel_rect(&slot, right_dock_outer_left);
        } else {
            layout_util::record_right_dock_panel(&panel.response, right_dock_outer_left);
        }
    }

    /// 轮询后台保存 / 测试 / 对话请求（面板或设置窗打开时由 workspace 调用）。
    pub fn poll_background(&mut self, ctx: &egui::Context) {
        self.poll_pending(ctx);
    }

    pub fn show_settings_dialog(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        open: &mut bool,
        app_settings: &mut AppSettings,
    ) {
        if !*open {
            return;
        }
        self.key_configured_stored = app_settings.ai.has_api_key();
        let mut should_close = false;
        let text_low = theme.color_form_hint();
        let modal_sz = layout_util::modal_edit_size(ctx);
        crate::ui::chrome::modal_window("ai_settings_modal", theme, ctx)
            .open(open)
            .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
            .movable(true)
            .resizable(false)
            .default_size(modal_sz)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    if crate::ui::chrome::modal_header(
                        ui,
                        theme,
                        i18n::tr(ctx, "AI settings", "AI 设置"),
                        crate::ui::chrome::modal_title_font_size(theme),
                    ) {
                        should_close = true;
                    }
                    ui.label(
                        egui::RichText::new(i18n::tr(
                            ctx,
                            "OpenAI-compatible APIs; API Key is encrypted locally in settings.json and not routed through team servers.",
                            "OpenAI 兼容接口；API Key 加密保存在本机 settings.json，不经团队服务器。",
                        ))
                        .size(theme.font_size_small())
                        .color(text_low),
                    );
                    ui.add_space(theme.spacing_sm());
                    self.show_setup_fields(ui, ctx, theme, app_settings);
                });
            });
        if should_close {
            *open = false;
        }
    }

    pub fn show_foreground_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        open: &mut bool,
        app_settings: &mut AppSettings,
    ) {
        if !*open {
            return;
        }
        let screen = ctx.screen_rect();
        let dock_inset = theme.spacing_right_dock_screen_inset();
        let Some(slot) = layout_util::right_dock_foreground_slot(
            self.last_panel_slot_rect,
            ctx,
            layout_util::AI_PANEL_ID,
            layout_util::SidePanelProfile::Standard,
            None,
            dock_inset,
        ) else {
            return;
        };
        let geom = crate::ui::chrome::prepare_right_dock_foreground_geom(slot, screen, theme);
        let layer_id = crate::ui::chrome::right_dock_foreground_layer_id("mistterm_ai_fg");
        crate::ui::chrome::paint_right_dock_foreground_shell(ctx, layer_id, geom.paint, theme);
        crate::ui::chrome::show_right_dock_foreground_body(
            "mistterm_ai_fg",
            ctx,
            &geom,
            layout_util::SidePanelProfile::Standard,
            |ui, _body_w| {
                let prev_gap_y = ui.spacing().item_spacing.y;
                ui.spacing_mut().item_spacing.y = 0.0;
                theme.frame_right_dock_header_band().show(ui, |ui| {
                    layout_util::set_width_to_available(ui);
                    ui.horizontal(|ui| {
                        crate::ui::chrome::panel_header_title_leading(
                            ui,
                            theme,
                            crate::ui::icons::IconId::Api,
                            i18n::tr(ctx, "AI Assistant", "AI 助手"),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::ui::chrome::dock_close_icon_button(ui, theme)
                                .on_hover_text(i18n::tr(ctx, "Close AI panel", "关闭 AI 面板"))
                                .clicked()
                            {
                                *open = false;
                            }
                        });
                    });
                });
                crate::ui::chrome::right_dock_header_divider(ui, theme);
                ui.spacing_mut().item_spacing.y = prev_gap_y;
                ui.add_space(theme.spacing_xs());
                self.key_configured_stored = app_settings.ai.has_api_key();
                self.show_panel_body(ui, ctx, theme, app_settings);
            },
        );
    }

    fn status_line(&self, ctx: &egui::Context) -> Option<String> {
        if self.busy {
            return Some(i18n::tr(ctx, "Generating AI reply…", "AI 回复生成中…").to_string());
        }
        if self.is_background_busy() {
            return self.test_status.clone();
        }
        self.input_status.clone()
    }

    fn show_panel_body(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        app_settings: &mut AppSettings,
    ) {
        let ready = self.can_chat(app_settings);
        if let Some(status) = self.status_line(ctx) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    egui::RichText::new(status).size(theme.font_size_small()),
                );
            });
            ui.add_space(theme.spacing_xs());
        } else if !ready {
            ui.colored_label(
                theme.amber_color(),
                i18n::tr(
                    ctx,
                    "Configure API Key & model via Tools → AI Settings, then enter your question below.",
                    "请先在菜单「工具 → AI 设置」中配置 API Key 与模型，再在下方输入问题。",
                ),
            );
            ui.add_space(theme.spacing_sm());
        } else if !app_settings.ai.enabled {
            ui.colored_label(
                theme.amber_color(),
                i18n::tr(
                    ctx,
                    "Turn on “Enable AI” in Tools → AI Settings.",
                    "请在「工具 → AI 设置」中勾选「启用 AI」。",
                ),
            );
            ui.add_space(theme.spacing_sm());
        }
        let row_w = bind_row_width(ui);
        // 以当前光标起点作为正文起始，避免把提示文案错误地覆盖到标题栏区域。
        let flex_top = ui.cursor().min.y;
        let flex_h = ui.available_height().max(1.0);
        let flex_left = ui.min_rect().min.x;
        let flex_rect = egui::Rect::from_min_max(
            egui::pos2(flex_left, flex_top),
            egui::pos2(flex_left + row_w, flex_top + flex_h),
        );
        let gap = theme.spacing_xs();
        let bottom_pad = 0.0;
        let input_h = ai_input_block_height(theme);
        let input_rect = egui::Rect::from_min_max(
            egui::pos2(flex_rect.min.x, flex_rect.max.y - input_h - bottom_pad),
            egui::pos2(flex_rect.max.x, flex_rect.max.y - bottom_pad),
        );
        let chat_rect = egui::Rect::from_min_max(
            flex_rect.min,
            egui::pos2(flex_rect.max.x, (input_rect.min.y - gap).max(flex_rect.min.y)),
        );
        let chat_h = chat_rect.height().max(64.0);
        if chat_rect.height() > 1.0 {
            ui.allocate_ui_at_rect(chat_rect, |ui| {
                bind_row_width(ui);
                self.show_conversation(ui, ctx, theme, chat_h);
            });
        }
        ui.allocate_ui_at_rect(input_rect, |ui| {
            bind_row_width(ui);
            self.show_input_bar(ui, ctx, theme, app_settings, ready);
        });
    }

    fn is_background_busy(&self) -> bool {
        self.background.is_some()
    }

    /// 已具备 Key 与模型（可编辑输入；发送另需勾选「启用 AI」）。
    fn can_chat(&self, app_settings: &AppSettings) -> bool {
        !app_settings.ai.model.trim().is_empty()
            && (app_settings.ai.has_api_key() || !self.settings_key_input.trim().is_empty())
    }

    fn can_send_now(&self, app_settings: &AppSettings) -> bool {
        self.can_chat(app_settings)
            && app_settings.ai.enabled
            && !self.busy
            && !self.is_background_busy()
    }

    fn effective_api_key<'a>(&'a self, app_settings: &'a AppSettings) -> Option<String> {
        if !self.settings_key_input.trim().is_empty() {
            return Some(self.settings_key_input.trim().to_string());
        }
        app_settings.ai.load_api_key()
    }

    fn show_setup_fields(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        app_settings: &mut AppSettings,
    ) {
        let settings = &mut app_settings.ai;
        let label = theme.color_form_hint();
        crate::ui::chrome::form_checkbox(
            ui,
            theme,
            &mut settings.enabled,
            i18n::tr(ctx, "Enable AI", "启用 AI"),
        );
        ui.add_space(theme.spacing_sm());
        ui.label(
            egui::RichText::new(i18n::tr(ctx, "API base URL", "API 地址"))
                .size(theme.font_size_small())
                .color(label),
        );
        ui.add(
            egui::TextEdit::singleline(&mut settings.base_url)
                .hint_text("https://api.openai.com/v1")
                .desired_width(f32::INFINITY),
        );
        ui.label(
            egui::RichText::new(i18n::tr(ctx, "Model", "模型"))
                .size(theme.font_size_small())
                .color(label),
        );
        ui.add(
            egui::TextEdit::singleline(&mut settings.model)
                .hint_text("gpt-4o-mini")
                .desired_width(f32::INFINITY),
        );
        ui.label(
            egui::RichText::new("API Key")
                .size(theme.font_size_small())
                .color(label),
        );
        if self.key_configured_stored && self.settings_key_input.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(i18n::tr(ctx, "Saved encrypted locally", "已加密保存在本机配置"))
                        .size(theme.font_size_small())
                        .color(theme.green_color()),
                );
                if ui
                    .small_button(i18n::tr(ctx, "Change Key", "更换 Key"))
                    .clicked() {
                    self.key_configured_stored = false;
                }
            });
        }
        ui.add(
            egui::TextEdit::singleline(&mut self.settings_key_input)
                .password(true)
                .hint_text(if self.key_configured_stored {
                    i18n::tr(ctx, "Enter new key, then Save", "输入新 Key 后点保存")
                } else {
                    "sk-..."
                })
                .desired_width(f32::INFINITY),
        );
        let setup_busy = self.is_background_busy();
        let mut do_save = false;
        let mut do_test = false;
        ui.horizontal(|ui| {
            if !setup_busy
                && crate::ui::chrome::panel_action_primary_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Check,
                    i18n::tr(ctx, "Save", "保存"),
                )
                .clicked()
            {
                do_save = true;
            }
            if !setup_busy
                && crate::ui::chrome::panel_action_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Plug,
                    i18n::tr(ctx, "Test connection", "测试连接"),
                )
                    .clicked()
            {
                do_test = true;
            }
        });
        if do_save {
            self.start_save_background(ctx, app_settings);
        }
        if do_test {
            self.start_test_background(ctx, app_settings);
        }
        if let Some(ref s) = self.test_status {
            ui.horizontal(|ui| {
                if self.is_background_busy() {
                    ui.spinner();
                }
                ui.label(
                    egui::RichText::new(s)
                        .size(theme.font_size_small())
                        .color(theme.color_form_hint()),
                );
            });
        }
    }

    fn start_save_background(&mut self, ctx: &egui::Context, app_settings: &mut AppSettings) {
        if self.is_background_busy() {
            return;
        }
        self.test_status = Some(i18n::tr(ctx, "Saving…", "保存中…").into());
        self.last_error = None;
        let key = self.settings_key_input.clone();
        if !key.trim().is_empty() {
            if let Err(e) = app_settings.ai.set_api_key(&key) {
                self.test_status = Some(format!(
                    "{}{e}",
                    i18n::tr(ctx, "Save failed: ", "保存失败："),
                ));
                return;
            }
            self.settings_key_input.clear();
            self.key_configured_stored = true;
        }
        let settings = app_settings.clone();
        let saved_key = !key.trim().is_empty();
        let lang = i18n::language(ctx);
        let (tx, rx) = std::sync::mpsc::channel();
        self.background = Some(BackgroundJob::Save(rx));
        thread::spawn(move || {
            let loc = i18n::Locale::from(lang);
            let result = settings.save().map(|_| {
                if saved_key {
                    loc.tr(
                        "API Key encrypted and saved — you can ask below",
                        "已加密保存 API Key，可在下方输入问题",
                    )
                    .to_string()
                } else {
                    loc.tr(
                        "Saved endpoint and model",
                        "已保存地址与模型",
                    )
                    .to_string()
                }
            }).map_err(|e| e.to_string());
            let _ = tx.send(result);
        });
    }

    fn start_test_background(&mut self, ctx: &egui::Context, app_settings: &AppSettings) {
        if self.is_background_busy() {
            return;
        }
        let Some(key) = self.effective_api_key(app_settings) else {
            self.test_status = Some(i18n::tr(ctx, "Fill in API Key first", "请先填写 API Key").into());
            return;
        };
        self.test_status = Some(i18n::tr(ctx, "Testing connection…", "测试连接中…").into());
        self.last_error = None;
        let ai = app_settings.ai.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.background = Some(BackgroundJob::Test(rx));
        thread::spawn(move || {
            let r = crate::core::test_connection_with_key(&ai, &key);
            let _ = tx.send(r);
        });
    }

    fn show_conversation(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        scroll_h: f32,
    ) {
        if !self.attached_context.is_empty() {
            ui.group(|ui| {
                let _ = bind_row_width(ui);
                ui.label(
                    egui::RichText::new(i18n::tr(
                        ctx,
                        "Attached terminal context",
                        "附带的终端上下文",
                    ))
                        .size(theme.font_size_small())
                        .strong(),
                );
                let preview: String = self
                    .attached_context
                    .lines()
                    .take(8)
                    .collect::<Vec<_>>()
                    .join("\n");
                let more = self.attached_context.lines().count() > 8;
                ui.label(
                    egui::RichText::new(if more {
                        format!("{preview}\n…")
                    } else {
                        preview
                    })
                    .monospace()
                    .size(theme.font_size_small()),
                );
                if crate::ui::chrome::panel_action_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Trash,
                    i18n::tr(ctx, "Clear context", "清除上下文"),
                )
                    .clicked() {
                    self.clear_context();
                }
            });
            ui.add_space(theme.spacing_sm());
        }
        egui::ScrollArea::vertical()
            .id_source("mistterm_ai_chat_scroll")
            .max_height(scroll_h)
            .auto_shrink([false; 2])
            .stick_to_bottom(true)
            .drag_to_scroll(false)
            .show(ui, |ui| {
                bind_row_width(ui);
                if self.messages.is_empty() && !self.busy {
                    ui.label(
                        egui::RichText::new(i18n::tr(
                            ctx,
                            "Type below; attach terminal selections to interpret output.",
                            "在下方输入问题；附带终端选区可请求解读输出。",
                        ))
                            .size(theme.font_size_small())
                            .color(theme.color_form_hint()),
                    );
                }
                for (i, msg) in self.messages.iter().enumerate() {
                    let mut picked = None;
                    self.render_message(ui, ctx, theme, msg, &mut picked);
                    if let Some(cmd) = picked {
                        self.command_for_terminal = Some(cmd);
                    }
                    if i + 1 < self.messages.len() {
                        ui.add_space(theme.spacing_xs());
                    }
                }
            });
    }

    /// 渲染单条消息；`command_pick` 收集本帧内「执行」或命令卡片的点击。
    fn render_message(
        &self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        msg: &UiMessage,
        command_pick: &mut Option<String>,
    ) {
        let bubble_fill = if msg.role == "user" {
            theme.accent_alpha(28)
        } else {
            theme.color_subtle_inset_fill()
        };
        let bubble_stroke = if msg.role == "user" {
            egui::Stroke::NONE
        } else {
            theme.divider_stroke()
        };
        let rounding = egui::Rounding::same(theme.radius_list_item());
        let mut render_bubble = |ui: &mut egui::Ui| {
            egui::Frame::none()
                .fill(bubble_fill)
                .stroke(bubble_stroke)
                .rounding(rounding)
                .inner_margin(egui::vec2(10.0, 9.0))
                .show(ui, |ui| {
                    if msg.role != "user" {
                        let _ = bind_row_width(ui);
                    } else {
                        ui.set_max_width(ui.available_width());
                    }
                    let prev_gap_y = ui.spacing().item_spacing.y;
                    ui.spacing_mut().item_spacing.y = theme.spacing_xs();
                    markdown_view::show_markdown(
                        ui,
                        theme,
                        &msg.content,
                        command_pick,
                        msg.role != "user",
                    );
                    if msg.role == "assistant" && !msg.commands.is_empty() {
                        ui.add_space(theme.spacing_xs());
                        ui.label(
                            egui::RichText::new(i18n::tr(ctx, "Runnable commands", "可执行命令"))
                                .size(theme.font_size_small())
                                .color(theme.color_form_hint()),
                        );
                        for cmd in &msg.commands {
                            if show_command_card(ui, ctx, theme, cmd) {
                                *command_pick = Some(cmd.clone());
                            }
                            ui.add_space(theme.spacing_xs());
                        }
                    }
                    ui.spacing_mut().item_spacing.y = prev_gap_y;
                })
                .response
                .context_menu(|ui| {
                    crate::ui::chrome::apply_context_menu_style(ui, theme);
                    if crate::ui::chrome::popup_menu_button(
                        ui,
                        theme,
                        i18n::tr(ctx, "Copy full message", "复制全文"),
                    )
                        .clicked() {
                        if let Ok(mut clip) = Clipboard::new() {
                            let _ = clip.set_text(msg.content.clone());
                        }
                        ui.close_menu();
                    }
                });
        };
        if msg.role == "user" {
            ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                let max_w = (ui.available_width() * 0.88).max(120.0);
                ui.set_max_width(max_w);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                        ui.set_max_width(max_w);
                        render_bubble(ui);
                    });
                });
            });
        } else {
            render_bubble(ui);
        }
    }

    fn show_input_bar(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        app_settings: &mut AppSettings,
        ready: bool,
    ) {
        let can_type = ready && !self.is_background_busy();
        let can_send = self.can_send_now(app_settings);
        let ctrl_enter = ui.input(|i| {
            i.key_pressed(egui::Key::Enter) && (i.modifiers.ctrl || i.modifiers.command)
        });
        let _row_w = bind_row_width(ui);
        let draft_id = egui::Id::new("mistterm_ai_draft");
        let focused = ui.memory(|m| m.has_focus(draft_id));
        let mut send_clicked = false;
        let mut clear_clicked = false;
        theme.frame_form_text_input(focused).show(ui, |ui| {
            let inner_w =
                (ui.available_width() - theme.spacing_search_input_x() * 2.0 - 4.0).max(48.0);
            // 与全局输入框一致：占位符使用 hint 色，正文仍用输入正文色。
            let prev_override = ui.style_mut().visuals.override_text_color;
            ui.style_mut().visuals.override_text_color = Some(theme.color_form_hint());
            ui.add(
                egui::TextEdit::multiline(&mut self.draft_input)
                    .id(draft_id)
                    .frame(false)
                    .interactive(can_type)
                    .hint_text(crate::ui::chrome::hint_rich(
                        theme,
                        i18n::tr(ctx, "Ask a question, Ctrl+Enter to send", "输入问题，Ctrl+Enter 发送"),
                        theme.font_size_control_input(),
                    ))
                    .desired_rows(2)
                    .desired_width(inner_w)
                    .text_color(theme.color_text_input_text())
                    .font(egui::FontId::proportional(theme.font_size_control_input())),
            );
            ui.style_mut().visuals.override_text_color = prev_override;
            // 把按钮行往下推一截：原 spacing_xs(2px) 让发送/清空贴在多行输入下沿，按起来逼仄。
            ui.add_space(theme.spacing_sm() + 4.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                send_clicked = ui
                    .add_enabled_ui(can_send, |ui| {
                        ai_panel_icon_button(ui, theme, IconId::Upload, true)
                            .on_hover_text(i18n::tr(ctx, "Send (Ctrl+Enter)", "发送 (Ctrl+Enter)"))
                            .clicked()
                    })
                    .inner;
                ui.add_space(theme.spacing_xs());
                clear_clicked = ui
                    .add_enabled_ui(can_type, |ui| {
                        ai_panel_icon_button(ui, theme, IconId::Trash, false)
                            .on_hover_text(i18n::tr(ctx, "Clear conversation", "清空对话"))
                            .clicked()
                    })
                    .inner;
            });
        });
        if clear_clicked {
            self.messages.clear();
            self.last_error = None;
            self.input_status = None;
        }
        if (send_clicked || ctrl_enter) && can_send {
            match self.send_message(ctx, app_settings) {
                SendOutcome::Sent => {
                    self.input_status = None;
                    ctx.request_repaint();
                }
                SendOutcome::Empty => {
                    self.input_status = Some(
                        i18n::tr(
                            ctx,
                            "Enter a question (gray text is hint, not your input)",
                            "请输入问题（灰字为示例，非已输入内容）",
                        )
                        .to_string(),
                    );
                }
                SendOutcome::NotReady(msg) => {
                    self.input_status = Some(msg);
                }
            }
        }
        if let Some(ref e) = self.last_error {
            ui.colored_label(theme.red_color(), e);
        }
        let _ = app_settings;
    }

    fn send_message(&mut self, ctx: &egui::Context, app_settings: &AppSettings) -> SendOutcome {
        if !self.can_chat(app_settings) {
            return SendOutcome::NotReady(
                i18n::tr(ctx, "Configure API Key & model and save first", "请先配置 API Key 与模型并保存")
                    .to_string(),
            );
        }
        if !app_settings.ai.enabled {
            return SendOutcome::NotReady(
                i18n::tr(ctx, "Enable AI first", "请先勾选「启用 AI」").to_string(),
            );
        }
        if self.busy || self.is_background_busy() {
            return SendOutcome::NotReady(
                i18n::tr(ctx, "Wait for the current operation to finish", "请等待当前操作完成")
                    .to_string(),
            );
        }
        let question = self.draft_input.trim().to_string();
        if question.is_empty() {
            return SendOutcome::Empty;
        }
        self.draft_input.clear();
        let mut user_body = question.clone();
        if !self.attached_context.is_empty() {
            user_body.push_str(i18n::tr(
                ctx,
                "\n\n--- Terminal context ---\n",
                "\n\n--- 终端上下文 ---\n",
            ));
            user_body.push_str(&self.attached_context);
        }
        self.messages.push(UiMessage {
            role: "user",
            content: question,
            commands: vec![],
        });
        let last_idx = self.messages.len().saturating_sub(1);
        let api_messages: Vec<ChatMessage> = self
            .messages
            .iter()
            .enumerate()
            .map(|(i, m)| ChatMessage {
                role: if m.role == "user" {
                    "user".to_string()
                } else {
                    "assistant".to_string()
                },
                content: if i == last_idx && m.role == "user" {
                    user_body.clone()
                } else {
                    m.content.clone()
                },
            })
            .collect();
        let settings = app_settings.ai.clone();
        let api_key = match self.effective_api_key(app_settings) {
            Some(k) => k,
            None => {
                let msg = i18n::tr(ctx, "Fill in and save API Key first", "请先填写并保存 API Key")
                    .to_string();
                self.last_error = Some(msg.clone());
                return SendOutcome::NotReady(msg);
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.background = Some(BackgroundJob::Chat(rx));
        self.busy = true;
        self.last_error = None;
        thread::spawn(move || {
            let r = crate::core::chat_completions_with_key(&settings, &api_key, &api_messages);
            let _ = tx.send(r);
        });
        SendOutcome::Sent
    }

    fn poll_pending(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.background else {
            return;
        };
        match job {
            BackgroundJob::Chat(rx) => match rx.try_recv() {
                Ok(Ok(reply)) => {
                    let commands = extract_shell_commands(&reply);
                    self.messages.push(UiMessage {
                        role: "assistant",
                        content: reply,
                        commands,
                    });
                    self.background = None;
                    self.busy = false;
                    self.input_status = None;
                    self.attached_context.clear();
                    ctx.request_repaint();
                }
                Ok(Err(e)) => {
                    self.last_error = Some(i18n::localize_backend_error(i18n::language(ctx), &e));
                    self.input_status = None;
                    self.background = None;
                    self.busy = false;
                    ctx.request_repaint();
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(120));
                }
                Err(TryRecvError::Disconnected) => {
                    self.last_error = Some(
                        i18n::tr(ctx, "Request interrupted", "请求已中断").to_string(),
                    );
                    self.background = None;
                    self.busy = false;
                }
            },
            BackgroundJob::Save(rx) => match rx.try_recv() {
                Ok(Ok(msg)) => {
                    self.test_status = Some(msg);
                    self.background = None;
                    ctx.request_repaint();
                }
                Ok(Err(e)) => {
                    self.test_status = Some(format!(
                        "{}{e}",
                        i18n::tr(ctx, "Save failed: ", "保存失败："),
                    ));
                    self.background = None;
                    ctx.request_repaint();
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(120));
                }
                Err(TryRecvError::Disconnected) => {
                    self.test_status = Some(
                        i18n::tr(ctx, "Save interrupted", "保存已中断").to_string(),
                    );
                    self.background = None;
                }
            },
            BackgroundJob::Test(rx) => match rx.try_recv() {
                Ok(Ok(())) => {
                    self.test_status = Some(
                        i18n::tr(ctx, "Connection OK", "连接成功").to_string(),
                    );
                    self.background = None;
                    ctx.request_repaint();
                }
                Ok(Err(e)) => {
                    self.test_status = Some(e);
                    self.background = None;
                    ctx.request_repaint();
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(120));
                }
                Err(TryRecvError::Disconnected) => {
                    self.test_status = Some(
                        i18n::tr(ctx, "Test interrupted", "测试已中断").to_string(),
                    );
                    self.background = None;
                }
            },
        }
    }
}

enum SendOutcome {
    Sent,
    Empty,
    NotReady(String),
}

fn ai_panel_icon_button(ui: &mut egui::Ui, theme: &Theme, id: IconId, primary: bool) -> egui::Response {
    // 比通用 dock 图标按钮放大一档：原 24px 命中 / 18px 字形太小，鼠标不好点。
    let hit = (theme.size_tab_bar_icon_btn() + 8.0).max(30.0);
    let icon_px = (theme.size_icon_glyph() + 4.0).max(22.0);
    let (idle, hover) = if primary {
        (theme.accent_color(), theme.accent_color())
    } else {
        (
            theme.color_tab_bar_icon(),
            theme.color_tab_bar_icon_hover(),
        )
    };
    icons::icon_hit_button(
        ui,
        id,
        hit,
        icon_px,
        idle,
        hover,
        theme.color_tab_bar_icon_btn_hover_fill(),
        if primary {
            theme.accent_alpha(80)
        } else {
            theme.accent_alpha(45)
        },
        theme.radius_list_item(),
    )
}

/// 底部输入区占用高度（多行框 + 按钮行 + 间距，供 `allocate_ui_at_rect` 切分）。
/// `toolbar` 的 hit/spacing 必须与 [`ai_panel_icon_button`] 与 `show_input_bar` 里
/// 多行框→按钮行之间的 `add_space` 保持一致，否则会与上方滚动区错位。
fn ai_input_block_height(theme: &Theme) -> f32 {
    let line = theme.font_size_control_input() * 1.45;
    let field = line * 2.0 + theme.spacing_search_input_y() * 2.0 + 12.0;
    let btn_hit = (theme.size_tab_bar_icon_btn() + 8.0).max(30.0);
    let toolbar = btn_hit + (theme.spacing_sm() + 4.0) + 2.0;
    field + toolbar + theme.spacing_xs() + 6.0
}

/// 子 Ui 占满**当前**可用行宽（勿把外层宽度传入 Frame/ScrollArea 内层，否则会左裁切）。
fn bind_row_width(ui: &mut egui::Ui) -> f32 {
    let w = layout_util::set_width_to_available(ui);
    ui.set_width(w);
    w
}

fn show_command_card(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    theme: &Theme,
    cmd: &str,
) -> bool {
    let mut clicked = false;
    let preview = compact_command_preview(cmd);
    egui::Frame::none()
        .fill(theme.color_text_input_fill())
        .stroke(theme.stroke_input())
        .rounding(theme.radius_list_item())
        .inner_margin(egui::vec2(8.0, 5.0))
        .show(ui, |ui| {
            let row_w = layout_util::set_width_to_available(ui);
            ui.horizontal(|ui| {
                ui.set_max_width(row_w);
                ui.label(
                    egui::RichText::new(preview)
                        .monospace()
                        .size(theme.font_size_small()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    clicked = ai_panel_icon_button(ui, theme, IconId::TerminalPrompt, true)
                        .on_hover_text(i18n::tr(
                            ctx,
                            "Send this command to the terminal",
                            "发送该命令到终端",
                        ))
                        .clicked();
                });
            });
        });
    clicked
}

fn compact_command_preview(cmd: &str) -> String {
    let first = cmd
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    if first.is_empty() {
        return String::new();
    }
    let mut chars = first.chars();
    let head: String = chars.by_ref().take(72).collect();
    if chars.next().is_some() || cmd.lines().skip_while(|l| l.trim().is_empty()).nth(1).is_some() {
        format!("{head} ...")
    } else {
        head
    }
}
