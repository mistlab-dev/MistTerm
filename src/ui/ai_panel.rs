//! 右侧 AI 面板：对话、附带终端上下文、「用到终端」。

use eframe::egui;
use arboard::Clipboard;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;

use crate::core::{
    delete_chat, extract_shell_commands, is_runnable_shell_command, load_chat,
    prepare_terminal_context, save_chat,
    AppSettings, ChatEvent, ChatMessage, PreparedTerminalContext, StoredAiMessage,
    StoredContextRef, TerminalSessionMeta, run_chat_with_key,
};
use crate::i18n::{self};
use crate::ui::icons::IconId;
use crate::ui::layout_util;
use crate::ui::theme::Theme;

#[derive(Clone)]
struct TerminalContextRef {
    text: String,
    line_count: usize,
    char_count: usize,
    truncated: bool,
    original_line_count: usize,
    original_char_count: usize,
    /// 非终端选区时的芯片标题键（如 `monitor`、`session_log`）。
    source_key: Option<String>,
}

impl TerminalContextRef {
    fn from_prepared(prep: PreparedTerminalContext) -> Self {
        Self {
            text: prep.text,
            line_count: prep.line_count,
            char_count: prep.char_count,
            truncated: prep.truncated,
            original_line_count: prep.original_line_count,
            original_char_count: prep.original_char_count,
            source_key: None,
        }
    }

    fn context_source_title(ctx: &egui::Context, key: &str) -> String {
        match key {
            "monitor" => i18n::tr(ctx, "Monitor snapshot", "监控快照").to_string(),
            "session_log" => i18n::tr(ctx, "Session log", "会话日志").to_string(),
            "terminal_tail" => i18n::tr(ctx, "Terminal output", "终端输出").to_string(),
            other => other.to_string(),
        }
    }

    fn chip_label(&self, ctx: &egui::Context, index: usize) -> String {
        let title = if let Some(key) = &self.source_key {
            Self::context_source_title(ctx, key)
        } else if self.line_count <= 3 && self.char_count <= 120 && index == 0 {
            i18n::tr(ctx, "Terminal selection", "终端选区").to_string()
        } else {
            format!(
                "{} {}",
                i18n::tr(ctx, "Terminal selection", "终端选区"),
                index + 1
            )
        };
        let unit = if self.line_count == 1 {
            i18n::tr(ctx, "line", "行")
        } else {
            i18n::tr(ctx, "lines", "行")
        };
        let mut label = format!("{title} · {} {unit}", self.line_count);
        if self.truncated {
            label.push_str(&format!(
                " ({})",
                i18n::tr(ctx, "truncated", "已截断")
            ));
        }
        label
    }
}

#[derive(Clone)]
struct UiMessage {
    role: &'static str,
    /// 气泡内展示的用户问题或助手回复（不含附带终端全文）。
    content: String,
    /// 发往 API 的完整 user 正文（含终端上下文）；助手消息为 None。
    api_content: Option<String>,
    /// 本条 user 消息附带的终端选区引用（可多条）。
    context_refs: Vec<TerminalContextRef>,
    commands: Vec<String>,
}

enum BackgroundJob {
    Chat {
        rx: Receiver<ChatEvent>,
    },
    Save(Receiver<Result<String, String>>),
    Test(Receiver<Result<(), String>>),
}

pub struct AiPanel {
    messages: Vec<UiMessage>,
    draft_input: String,
    attached_contexts: Vec<TerminalContextRef>,
    session_meta: Option<TerminalSessionMeta>,
    chat_session_key: String,
    chat_dirty: bool,
    background: Option<BackgroundJob>,
    busy: bool,
    streaming: bool,
    chat_cancel: Option<Arc<AtomicBool>>,
    last_error: Option<String>,
    command_for_terminal: Option<String>,
    settings_key_input: String,
    /// 本地已加密保存 Key 时不再在输入框显示明文
    key_configured_stored: bool,
    test_status: Option<String>,
    /// 输入区旁即时提示（空内容、未启用、请求中等）
    input_status: Option<String>,
    /// AI 多行输入框是否持有键盘焦点；用于避免右 dock 打开时误拦 PTY 输入。
    draft_input_focused: bool,
    /// 输入栏「附带终端」按钮：由 App 读取并注入最近终端输出。
    attach_terminal_tail_requested: bool,
    last_panel_slot_rect: Option<egui::Rect>,
    /// 清空对话二次确认（防误触）。
    confirm_clear_chat: bool,
    /// 从 API 拉取到的模型 id 列表；拉取失败时为空并改用手动输入。
    available_models: Vec<String>,
    models_manual_fallback: bool,
    models_loading: bool,
    models_fetch: Option<Receiver<Result<Vec<String>, String>>>,
    /// 上次成功/失败拉取对应的 `base_url|key` 签名，避免每帧重复请求。
    models_fetch_signature: Option<String>,
    models_status: Option<String>,
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
            attached_contexts: Vec::new(),
            session_meta: None,
            chat_session_key: "global".to_string(),
            chat_dirty: false,
            background: None,
            busy: false,
            streaming: false,
            chat_cancel: None,
            last_error: None,
            command_for_terminal: None,
            settings_key_input: String::new(),
            key_configured_stored: false,
            test_status: None,
            input_status: None,
            draft_input_focused: false,
            attach_terminal_tail_requested: false,
            last_panel_slot_rect: None,
            confirm_clear_chat: false,
            available_models: Vec::new(),
            models_manual_fallback: false,
            models_loading: false,
            models_fetch: None,
            models_fetch_signature: None,
            models_status: None,
        }
    }

    pub fn attach_context(&mut self, text: String) {
        self.attach_context_labeled(None, text);
    }

    pub fn attach_context_labeled(&mut self, source_key: Option<&str>, text: String) {
        let prep = prepare_terminal_context(&text);
        if prep.line_count == 0 {
            return;
        }
        let mut item = TerminalContextRef::from_prepared(prep);
        item.source_key = source_key.map(str::to_string);
        if self
            .attached_contexts
            .iter()
            .any(|c| c.text == item.text && c.source_key == item.source_key)
        {
            return;
        }
        self.attached_contexts.push(item);
        self.confirm_clear_chat = false;
    }

    /// 附带选区后聚焦输入框（便于直接输入问题）。
    pub fn focus_draft_input(&self, ctx: &egui::Context) {
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("mistterm_ai_draft")));
    }

    /// 只有 AI 输入框真正聚焦时才阻止终端接收键盘；普通查看 AI 回复不应抢 PTY。
    pub fn is_draft_input_focused(&self) -> bool {
        self.draft_input_focused
    }

    pub fn attach_session_meta(&mut self, meta: TerminalSessionMeta) {
        if meta.host.is_some() || meta.username.is_some() || meta.session_name.is_some() {
            self.session_meta = Some(meta);
        }
    }

    pub fn set_chat_session_key(&mut self, key: String, persist: bool) {
        if self.chat_session_key == key {
            return;
        }
        if persist && self.chat_dirty {
            self.flush_persisted_chat(false);
        }
        self.chat_session_key = key;
        self.messages.clear();
        self.last_error = None;
        if persist {
            self.load_persisted_chat();
        }
    }

    pub fn cancel_generation(&mut self, ctx: &egui::Context) {
        if let Some(cancel) = &self.chat_cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        if self
            .messages
            .last()
            .is_some_and(|m| m.role == "assistant" && m.content.is_empty())
        {
            self.messages.pop();
        }
        self.background = None;
        self.busy = false;
        self.streaming = false;
        self.chat_cancel = None;
        self.input_status = Some(i18n::tr(ctx, "Generation stopped", "已停止生成").to_string());
    }

    pub fn take_command_for_terminal(&mut self) -> Option<String> {
        self.command_for_terminal.take()
    }

    /// 输入栏请求附带当前终端最近输出（由 App 每帧消费）。
    pub fn take_attach_terminal_tail_request(&mut self) -> bool {
        std::mem::replace(&mut self.attach_terminal_tail_requested, false)
    }

    fn load_persisted_chat(&mut self) {
        self.messages = load_chat(&self.chat_session_key)
            .into_iter()
            .map(stored_to_ui_message)
            .collect();
        self.chat_dirty = false;
    }

    fn flush_persisted_chat(&mut self, clear: bool) {
        if clear {
            delete_chat(&self.chat_session_key);
            self.chat_dirty = false;
            return;
        }
        let stored: Vec<StoredAiMessage> = self.messages.iter().map(ui_message_to_stored).collect();
        let _ = save_chat(&self.chat_session_key, &stored);
        self.chat_dirty = false;
    }

    pub fn clear_context(&mut self) {
        self.attached_contexts.clear();
    }

    #[inline]
    pub(crate) fn last_panel_slot_rect(&self) -> Option<egui::Rect> {
        self.last_panel_slot_rect
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
                let h = ui.available_height().max(1.0);
                let w = ui.available_width().max(1.0);
                ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
            });
        let dock_inset = theme.spacing_right_dock_screen_inset();
        let slot = layout_util::side_panel_place_slot(ctx, &panel.response, dock_col_w, dock_inset);
        crate::ui::chrome::paint_right_dock_slot_gap(ctx, theme, slot);
        self.last_panel_slot_rect = Some(slot);
        if let Some(slot) = self.last_panel_slot_rect {
            layout_util::record_right_dock_panel_rect(&slot, right_dock_outer_left);
        } else {
            layout_util::record_right_dock_panel(&panel.response, right_dock_outer_left);
        }
    }

    /// 轮询后台保存 / 测试 / 对话请求（面板或设置窗打开时由 workspace 调用）。
    pub fn poll_background(&mut self, ctx: &egui::Context, app_settings: &mut AppSettings) {
        self.poll_pending(ctx, app_settings);
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
            theme,
            &geom,
            layout_util::SidePanelProfile::Standard,
            |ui, _body_w| {
                let prev_gap_y = ui.spacing().item_spacing.y;
                ui.spacing_mut().item_spacing.y = 0.0;
                theme.frame_right_dock_header_band().show(ui, |ui| {
                    layout_util::set_width_to_available(ui);
                    crate::ui::chrome::dock_header_horizontal(ui, theme, |ui| {
                        crate::ui::chrome::panel_header_title_leading(
                            ui,
                            theme,
                            crate::ui::icons::IconId::Api,
                            i18n::tr(ctx, "AI Assistant", "AI 助手"),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::ui::chrome::dock_close_icon_button(
                                ui,
                                theme,
                                i18n::tr(ctx, "Close AI panel", "关闭 AI 面板"),
                            )
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
        if self.streaming {
            return Some(i18n::tr(ctx, "Generating AI reply…", "AI 回复生成中…").to_string());
        }
        if self.busy {
            return Some(i18n::tr(ctx, "Waiting for AI…", "等待 AI 响应…").to_string());
        }
        if self.is_background_busy() {
            return self.test_status.clone();
        }
        if let Some(err) = &self.last_error {
            return Some(err.clone());
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
        if !ready {
            ui.colored_label(
                theme.amber_color(),
                i18n::tr(
                    ctx,
                    "Configure OpenAI-compatible API URL, API Key, and model in Tools → AI Settings.",
                    "请在「工具 → AI 设置」中配置 OpenAI 兼容 API 地址、API Key 与模型。",
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
        } else if !self.busy && !self.streaming {
            if let Some(status) = self.status_line(ctx) {
                ui.horizontal(|ui| {
                    if self.is_background_busy() {
                        ui.spinner();
                    }
                    ui.label(
                        egui::RichText::new(status).size(theme.font_size_small()),
                    );
                });
                ui.add_space(theme.spacing_xs());
            }
        }
        bind_row_width(ui);
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            bind_row_width(ui);
            self.show_input_bar(ui, ctx, theme, app_settings, ready);
            ui.add_space(theme.spacing_xs());
            let scroll_h = ui.available_height().max(64.0);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), scroll_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    bind_row_width(ui);
                    ui.set_min_height(scroll_h);
                    ui.set_max_height(scroll_h);
                    self.show_conversation(ui, ctx, theme, app_settings);
                },
            );
        });
    }

    fn is_awaiting_assistant_reply(&self) -> bool {
        (self.busy || self.streaming)
            && self
                .messages
                .last()
                .is_some_and(|m| m.role == "assistant" && m.content.is_empty())
    }

    fn generation_status_text(&self, ctx: &egui::Context) -> String {
        if self.streaming {
            i18n::tr(ctx, "Generating AI reply…", "AI 回复生成中…").to_string()
        } else {
            i18n::tr(ctx, "Waiting for AI…", "等待 AI 响应…").to_string()
        }
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
            && (!self.draft_input.trim().is_empty() || !self.attached_contexts.is_empty())
    }

    fn effective_api_key<'a>(&'a self, app_settings: &'a AppSettings) -> Option<String> {
        if !self.settings_key_input.trim().is_empty() {
            return Some(self.settings_key_input.trim().to_string());
        }
        app_settings.ai.load_api_key()
    }

    fn models_fetch_signature(app_settings: &AppSettings, key: &str) -> String {
        format!("{}|{}", app_settings.ai.base_url.trim(), key.trim())
    }

    fn invalidate_models_fetch(&mut self) {
        self.models_fetch_signature = None;
        self.models_status = None;
    }

    fn maybe_start_models_fetch(&mut self, ctx: &egui::Context, app_settings: &AppSettings) {
        if self.models_loading || self.models_fetch.is_some() {
            return;
        }
        let Some(key) = self.effective_api_key(app_settings) else {
            return;
        };
        if app_settings.ai.base_url.trim().is_empty() {
            return;
        }
        let sig = Self::models_fetch_signature(app_settings, &key);
        if self.models_fetch_signature.as_deref() == Some(sig.as_str()) {
            return;
        }
        self.start_models_fetch(ctx, app_settings, &key, sig);
    }

    fn start_models_fetch(
        &mut self,
        ctx: &egui::Context,
        app_settings: &AppSettings,
        key: &str,
        signature: String,
    ) {
        self.models_loading = true;
        self.models_manual_fallback = false;
        self.available_models.clear();
        self.models_status = Some(
            i18n::tr(ctx, "Loading models…", "正在拉取模型列表…")
                .to_string(),
        );
        let ai = app_settings.ai.clone();
        let key = key.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        self.models_fetch = Some(rx);
        thread::spawn(move || {
            let result = crate::core::fetch_models_with_key(&ai, &key);
            let _ = tx.send(result);
        });
        self.models_fetch_signature = Some(signature);
    }

    fn poll_models_fetch(&mut self, ctx: &egui::Context, app_settings: &mut AppSettings) {
        let Some(rx) = self.models_fetch.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(models)) => {
                self.available_models = models;
                self.models_manual_fallback = false;
                self.models_loading = false;
                self.models_status = None;
                if app_settings.ai.model.trim().is_empty() {
                    if let Some(first) = self.available_models.first() {
                        app_settings.ai.model = first.clone();
                    }
                }
                ctx.request_repaint();
            }
            Ok(Err(e)) => {
                self.available_models.clear();
                self.models_manual_fallback = true;
                self.models_loading = false;
                self.models_status = Some(i18n::localize_backend_error(
                    i18n::language(ctx),
                    &e,
                ));
                ctx.request_repaint();
            }
            Err(TryRecvError::Empty) => {
                self.models_fetch = Some(rx);
                ctx.request_repaint_after(std::time::Duration::from_millis(120));
            }
            Err(TryRecvError::Disconnected) => {
                self.available_models.clear();
                self.models_manual_fallback = true;
                self.models_loading = false;
                self.models_status = Some(
                    i18n::tr(ctx, "Model fetch interrupted", "拉取模型列表已中断").to_string(),
                );
            }
        }
    }

    fn show_setup_fields(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        app_settings: &mut AppSettings,
    ) {
        let mut do_refresh_models = false;
        let mut do_save = false;
        let mut do_test = false;
        {
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
                    .clicked()
                {
                    self.key_configured_stored = false;
                    self.invalidate_models_fetch();
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
        ui.add_space(theme.spacing_sm());
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(i18n::tr(ctx, "Model", "模型"))
                    .size(theme.font_size_small())
                    .color(label),
            );
            if self.models_loading {
                ui.spinner();
            }
            if !self.models_loading
                && crate::ui::chrome::panel_action_icon_button(
                    ui,
                    theme,
                    IconId::Refresh,
                    i18n::tr(ctx, "Refresh model list", "刷新模型列表"),
                )
                .on_hover_text(i18n::tr(
                    ctx,
                    "Fetch models from API",
                    "从 API 拉取模型列表",
                ))
                .clicked()
            {
                do_refresh_models = true;
            }
        });
        if !self.models_manual_fallback && !self.available_models.is_empty() {
            let current = if settings.model.trim().is_empty() {
                i18n::tr(ctx, "Select a model", "选择模型").to_string()
            } else {
                settings.model.clone()
            };
            egui::ComboBox::from_id_source("ai_model_select")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    crate::ui::chrome::apply_menu_popup_style(ui, theme);
                    for id in &self.available_models {
                        if ui.selectable_label(settings.model == *id, id).clicked() {
                            settings.model = id.clone();
                        }
                    }
                });
        } else {
            ui.add(
                egui::TextEdit::singleline(&mut settings.model)
                    .hint_text(i18n::tr(ctx, "Enter model ID", "输入模型 ID"))
                    .desired_width(f32::INFINITY),
            );
            if self.models_manual_fallback {
                ui.label(
                    egui::RichText::new(i18n::tr(
                        ctx,
                        "Could not load models from API — enter the model ID manually.",
                        "无法从 API 拉取模型列表，请手动输入模型 ID。",
                    ))
                    .size(theme.font_size_small())
                    .color(theme.amber_color()),
                );
            }
        }
        if let Some(ref s) = self.models_status {
            ui.label(
                egui::RichText::new(s)
                    .size(theme.font_size_small())
                    .color(theme.color_form_hint()),
            );
        }
        ui.add_space(theme.spacing_sm());
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(i18n::tr(ctx, "Max tokens", "最大 tokens"))
                    .size(theme.font_size_small())
                    .color(label),
            );
            ui.add(egui::DragValue::new(&mut settings.max_tokens).speed(32));
            ui.label(
                egui::RichText::new(i18n::tr(ctx, "Timeout (s)", "超时 (秒)"))
                    .size(theme.font_size_small())
                    .color(label),
            );
            ui.add(egui::DragValue::new(&mut settings.timeout_secs).speed(1));
            ui.label(
                egui::RichText::new(i18n::tr(ctx, "Retries", "重试次数"))
                    .size(theme.font_size_small())
                    .color(label),
            );
            ui.add(egui::DragValue::new(&mut settings.request_retries).speed(1));
        });
        crate::ui::chrome::form_checkbox(
            ui,
            theme,
            &mut settings.stream_responses,
            i18n::tr(ctx, "Stream responses", "流式输出"),
        );
        crate::ui::chrome::form_checkbox(
            ui,
            theme,
            &mut settings.attach_session_meta,
            i18n::tr(ctx, "Attach session info", "附带会话信息"),
        );
        crate::ui::chrome::form_checkbox(
            ui,
            theme,
            &mut settings.persist_chats,
            i18n::tr(ctx, "Persist chat history", "保存对话历史"),
        );
        ui.label(
            egui::RichText::new(i18n::tr(ctx, "System prompt (optional)", "System prompt（可选）"))
                .size(theme.font_size_small())
                .color(label),
        );
        ui.add(
            egui::TextEdit::multiline(&mut settings.system_prompt)
                .hint_text(i18n::tr(
                    ctx,
                    "Leave empty for default ops assistant prompt",
                    "留空使用默认运维助手提示词",
                ))
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        let setup_busy = self.is_background_busy();
        ui.horizontal(|ui| {
            if !setup_busy
                && crate::ui::chrome::panel_action_primary_button_with_icon_ex(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Check,
                    i18n::tr(ctx, "Save", "保存"),
                    true,
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
        }
        if do_refresh_models {
            self.invalidate_models_fetch();
        }
        self.maybe_start_models_fetch(ctx, app_settings);
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
        app_settings: &AppSettings,
    ) {
        let scroll_h = ui.available_height().max(64.0);
        egui::ScrollArea::vertical()
            .id_source("mistterm_ai_chat_scroll")
            .max_height(scroll_h)
            .auto_shrink([false; 2])
            .stick_to_bottom(true)
            .drag_to_scroll(false)
            .show(ui, |ui| {
                bind_row_width(ui);
                ui.add_space(theme.spacing_xs());
                if self.messages.is_empty() && !self.busy {
                    ui.label(
                        egui::RichText::new(i18n::tr(
                            ctx,
                            "Type below; attach terminal selections in the input area.",
                            "在下方输入；终端选区会作为引用附在输入框中。",
                        ))
                            .size(theme.font_size_small())
                            .color(theme.color_form_hint()),
                    );
                    ui.add_space(theme.spacing_xs());
                    for sample in example_questions(ctx) {
                        if ui
                            .small_button(sample)
                            .on_hover_text(i18n::tr(ctx, "Fill input", "填入输入框"))
                            .clicked()
                        {
                            self.draft_input = sample.to_string();
                        }
                    }
                }
                let mut msg_action = None;
                for (i, msg) in self.messages.iter().enumerate() {
                    if self.is_awaiting_assistant_reply()
                        && i + 1 == self.messages.len()
                    {
                        continue;
                    }
                    let mut picked = None;
                    self.render_message(ui, ctx, theme, msg, i, &mut picked, &mut msg_action);
                    if let Some(cmd) = picked {
                        self.command_for_terminal = Some(cmd);
                    }
                    if i + 1 < self.messages.len() {
                        ui.add_space(theme.spacing_xs());
                    }
                }
                if self.is_awaiting_assistant_reply() {
                    self.render_generation_placeholder(ui, theme);
                }
                if let Some(action) = msg_action {
                    self.handle_message_action(ctx, app_settings, action);
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
        msg_index: usize,
        command_pick: &mut Option<String>,
        msg_action: &mut Option<AiMessageAction>,
    ) {
        let is_user = msg.role == "user";
        let bubble_fill = if is_user {
            theme.color_ai_user_bubble_fill()
        } else {
            theme.color_subtle_inset_fill()
        };
        let bubble_stroke = if is_user {
            egui::Stroke::new(
                theme.hairline_width(ui.ctx()),
                theme.color_ai_user_bubble_stroke(),
            )
        } else {
            theme.divider_stroke()
        };
        let inner_pad = if is_user {
            egui::vec2(16.0, 12.0)
        } else {
            egui::vec2(12.0, 10.0)
        };
        let rounding = egui::Rounding::same(theme.radius_list_item());

        let mut render_bubble_body = |ui: &mut egui::Ui, body_w: f32| {
            let prev_gap_y = ui.spacing().item_spacing.y;
            ui.spacing_mut().item_spacing.y = theme.spacing_xs();
            if !msg.context_refs.is_empty() {
                for (ci, context) in msg.context_refs.iter().enumerate() {
                    let mut remove = false;
                    show_terminal_context_chip(
                        ui,
                        ctx,
                        theme,
                        context,
                        ui.id().with(("msg_ctx", msg_index, ci)),
                        ci,
                        false,
                        &mut remove,
                    );
                    ui.add_space(theme.spacing_xs());
                }
            }
            if is_user {
                if !msg.content.trim().is_empty() {
                    show_wrapped_user_text(ui, theme, &msg.content, body_w);
                }
            } else {
                show_assistant_text(ui, theme, &msg.content, body_w);
            }
            let runnable_commands: Vec<&String> = msg
                .commands
                .iter()
                .filter(|cmd| is_runnable_shell_command(cmd))
                .collect();
            if !is_user && !runnable_commands.is_empty() {
                ui.add_space(theme.spacing_xs());
                ui.label(
                    egui::RichText::new(i18n::tr(ctx, "Runnable commands", "可执行命令"))
                        .size(theme.font_size_small())
                        .color(theme.color_form_hint()),
                );
                for cmd in runnable_commands {
                    if show_command_card(ui, ctx, theme, cmd) {
                        *command_pick = Some(cmd.to_string());
                    }
                    ui.add_space(theme.spacing_xs());
                }
            }
            if !is_user && !msg.content.trim().is_empty() {
                let hover_id = ui.id().with(("ai_msg_hover", msg_index));
                let rect = ui.min_rect().union(ui.max_rect());
                let hovered = ui
                    .interact(rect, hover_id, egui::Sense::hover())
                    .hovered();
                let emphasis = if hovered { 1.0 } else { 0.46 };
                ui.add_space(theme.spacing_xs());
                ui.horizontal(|ui| {
                    if crate::ui::chrome::panel_ghost_action_button(
                        ui,
                        theme,
                        IconId::File,
                        i18n::tr(ctx, "Copy", "复制"),
                        emphasis,
                    )
                    .clicked()
                    {
                        if let Ok(mut clip) = Clipboard::new() {
                            let _ = clip.set_text(msg.content.clone());
                        }
                    }
                    if crate::ui::chrome::panel_ghost_action_button(
                        ui,
                        theme,
                        IconId::Refresh,
                        i18n::tr(ctx, "Regenerate", "重新生成"),
                        emphasis,
                    )
                    .clicked()
                    {
                        *msg_action = Some(AiMessageAction::Regenerate(msg_index));
                    }
                });
            }
            ui.spacing_mut().item_spacing.y = prev_gap_y;
        };

        let context_menu = |ui: &mut egui::Ui| {
            crate::ui::chrome::apply_context_menu_style(ui, theme);
            if crate::ui::chrome::popup_menu_button(
                ui,
                theme,
                i18n::tr(ctx, "Copy full message", "复制全文"),
            )
            .clicked()
            {
                let copy_text = message_copy_text(msg);
                if let Ok(mut clip) = Clipboard::new() {
                    let _ = clip.set_text(copy_text);
                }
                ui.close_menu();
            }
        };

        if is_user {
            let row_w = ai_content_width(ui);
            let safe_pad = ai_message_side_padding(theme);
            let max_row_w = (row_w - safe_pad * 2.0).max(96.0);
            let bubble_w = max_row_w.max(160.0).min(max_row_w);
            let left_gap = (row_w - bubble_w - safe_pad).max(safe_pad);

            ui.horizontal(|ui| {
                if left_gap > 0.0 {
                    ui.add_space(left_gap);
                }
                egui::Frame::none()
                    .fill(bubble_fill)
                    .stroke(bubble_stroke)
                    .rounding(rounding)
                    .inner_margin(inner_pad)
                    .show(ui, |ui| {
                        let inner_w = (bubble_w - inner_pad.x * 2.0).max(24.0);
                        ui.set_min_width(inner_w);
                        ui.set_width(inner_w);
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_min_width(inner_w);
                            ui.set_width(inner_w);
                            render_bubble_body(ui, inner_w);
                        });
                    })
                    .response
                    .context_menu(context_menu);
            });
        } else {
            let row_w = ai_content_width(ui);
            let safe_pad = ai_message_side_padding(theme);
            let bubble_w = (row_w - safe_pad * 2.0).max(96.0);
            ui.horizontal(|ui| {
                ui.add_space(safe_pad);
                egui::Frame::none()
                    .fill(bubble_fill)
                    .stroke(bubble_stroke)
                    .rounding(rounding)
                    .inner_margin(inner_pad)
                    .show(ui, |ui| {
                        let inner_w = (bubble_w - inner_pad.x * 2.0).max(24.0);
                        ui.set_min_width(inner_w);
                        ui.set_width(inner_w);
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_min_width(inner_w);
                            ui.set_width(inner_w);
                            render_bubble_body(ui, inner_w);
                        });
                    })
                    .response
                    .context_menu(context_menu);
                ui.add_space(safe_pad);
            });
        }
    }

    fn render_generation_placeholder(&self, ui: &mut egui::Ui, theme: &Theme) {
        let row_w = ui.available_width().max(120.0);
        let safe_pad = ai_message_side_padding(theme);
        let bubble_w = (row_w - safe_pad * 2.0).max(96.0);
        ui.horizontal(|ui| {
            ui.add_space(safe_pad);
            egui::Frame::none()
                .fill(theme.color_subtle_inset_fill())
                .stroke(theme.divider_stroke())
                .rounding(theme.radius_list_item())
                .inner_margin(egui::vec2(12.0, 10.0))
                .show(ui, |ui| {
                    ui.set_width((bubble_w - 24.0).max(48.0));
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            egui::RichText::new(self.generation_status_text(ui.ctx()))
                                .size(theme.font_size_small())
                                .color(theme.text_secondary()),
                        );
                    });
                });
        });
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
        let generating = self.busy || self.streaming;
        let ctrl_enter = ui.input(|i| {
            i.key_pressed(egui::Key::Enter) && (i.modifiers.ctrl || i.modifiers.command)
        });
        let _row_w = bind_row_width(ui);
        let draft_id = egui::Id::new("mistterm_ai_draft");
        let focused = ui.memory(|m| m.has_focus(draft_id));
        self.draft_input_focused = focused;
        let mut send_clicked = false;
        let mut clear_draft_clicked = false;
        let mut clear_chat_clicked = false;
        let mut attach_terminal_clicked = false;
        theme.frame_form_text_input(focused).show(ui, |ui| {
            let inner_w =
                (ui.available_width() - theme.spacing_search_input_x() * 2.0 - 4.0).max(48.0);
            if !self.attached_contexts.is_empty() {
                self.show_attached_context_chip_row(ui, ctx, theme);
                if self.attached_contexts.iter().any(|c| c.truncated) {
                    ui.colored_label(
                        theme.amber_color(),
                        i18n::tr(
                            ctx,
                            "Some selections were truncated to fit model limits.",
                            "部分选区已截断以适配模型上限。",
                        ),
                    );
                }
                ui.add_space(theme.spacing_xs());
            }
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
                        i18n::tr(
                            ctx,
                            "Ask a question or send attached selection (Ctrl + Enter)",
                            "输入问题，或附带终端输出（Ctrl + Enter）",
                        ),
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
            ui.horizontal(|ui| {
                if ui
                    .add_enabled_ui(can_type, |ui| {
                        crate::ui::chrome::panel_action_button_with_icon_ex(
                            ui,
                            theme,
                            IconId::TerminalPrompt,
                            i18n::tr(ctx, "Attach terminal", "附带终端"),
                            true,
                        )
                        .on_hover_text(i18n::tr(
                            ctx,
                            "Attach the last 50 lines from the active terminal (no copy needed)",
                            "附带当前活动终端最近 50 行（无需手动复制）",
                        ))
                        .clicked()
                    })
                    .inner
                {
                    attach_terminal_clicked = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let send_label = i18n::tr(ctx, "Send", "发送");
                let stop_label = i18n::tr(ctx, "Stop", "停止");
                let clear_draft_label = i18n::tr(ctx, "Clear input", "清空输入");
                let clear_chat_label = i18n::tr(ctx, "Clear chat", "清空对话");
                if generating {
                    if crate::ui::chrome::panel_action_button_with_icon_ex(
                        ui,
                        theme,
                        IconId::Cross,
                        stop_label,
                        true,
                    )
                    .on_hover_text(i18n::tr(ctx, "Stop generation", "停止生成"))
                    .clicked()
                    {
                        self.cancel_generation(ctx);
                    }
                } else {
                    send_clicked = ui
                        .add_enabled_ui(can_send, |ui| {
                            crate::ui::chrome::panel_action_primary_button_with_icon_ex(
                                ui,
                                theme,
                                IconId::Upload,
                                send_label,
                                true,
                            )
                            .on_hover_text(i18n::tr(
                                ctx,
                                "Send (Ctrl + Enter)",
                                "发送 (Ctrl + Enter)",
                            ))
                            .clicked()
                        })
                        .inner;
                }
                ui.add_space(theme.spacing_xs());
                clear_chat_clicked = ui
                    .add_enabled_ui(can_type, |ui| {
                        crate::ui::chrome::panel_action_button_with_icon_ex(
                            ui,
                            theme,
                            IconId::Trash,
                            clear_chat_label,
                            true,
                        )
                        .on_hover_text(if self.confirm_clear_chat {
                            i18n::tr(ctx, "Click again to confirm", "再次点击确认")
                        } else {
                            i18n::tr(
                                ctx,
                                "Clear all messages (click twice to confirm)",
                                "清空全部对话（需点击两次确认）",
                            )
                        })
                        .clicked()
                    })
                    .inner;
                ui.add_space(theme.spacing_xs());
                clear_draft_clicked = ui
                    .add_enabled_ui(can_type && !self.draft_input.is_empty(), |ui| {
                        crate::ui::chrome::panel_action_button_with_icon_ex(
                            ui,
                            theme,
                            IconId::Cross,
                            clear_draft_label,
                            true,
                        )
                        .on_hover_text(i18n::tr(ctx, "Clear draft only", "仅清空输入框"))
                        .clicked()
                    })
                    .inner;
                });
            });
        });
        if attach_terminal_clicked {
            self.attach_terminal_tail_requested = true;
        }
        if clear_draft_clicked {
            self.draft_input.clear();
            self.attached_contexts.clear();
            self.input_status = None;
        }
        if clear_chat_clicked {
            if self.confirm_clear_chat {
                self.messages.clear();
                self.last_error = None;
                self.input_status = None;
                self.confirm_clear_chat = false;
                self.chat_dirty = true;
                self.flush_persisted_chat(true);
            } else if !self.messages.is_empty() {
                self.confirm_clear_chat = true;
                self.input_status = Some(
                    i18n::tr(
                        ctx,
                        "Click “Clear chat” again to delete all messages",
                        "再次点击「清空对话」以删除全部消息",
                    )
                    .to_string(),
                );
            }
        } else if !clear_draft_clicked && !send_clicked {
            self.confirm_clear_chat = false;
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
                            "Enter a question or attach terminal selection first",
                            "请输入问题，或先从终端附带选区",
                        )
                        .to_string(),
                    );
                }
                SendOutcome::NotReady(msg) => {
                    self.input_status = Some(msg);
                }
            }
        }
        let _ = app_settings;
    }

    /// 输入框内引用芯片（Cursor 式：附在 composer 上，不在对话区单独占行）。
    fn show_attached_context_chip_row(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
    ) {
        let mut remove_idx = None;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(i18n::tr(ctx, "Attached:", "已附带："))
                    .size(theme.font_size_small())
                    .color(theme.text_secondary()),
            );
            for (i, context) in self.attached_contexts.iter().enumerate() {
                let mut remove = false;
                show_terminal_context_chip(
                    ui,
                    ctx,
                    theme,
                    context,
                    egui::Id::new(("mistterm_ai_input_ctx", i)),
                    i,
                    true,
                    &mut remove,
                );
                if remove {
                    remove_idx = Some(i);
                }
            }
        });
        if let Some(i) = remove_idx {
            self.attached_contexts.remove(i);
        }
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
        let has_context = !self.attached_contexts.is_empty();
        if question.is_empty() && !has_context {
            return SendOutcome::Empty;
        }
        let display_question = if question.is_empty() {
            i18n::tr(
                ctx,
                "Explain the attached terminal output",
                "请解读附带的终端输出",
            )
            .to_string()
        } else {
            question
        };
        self.draft_input.clear();
        let context_refs = std::mem::take(&mut self.attached_contexts);
        let user_body = build_user_api_body(
            ctx,
            &display_question,
            &context_refs,
            self.session_meta.as_ref(),
            app_settings.ai.attach_session_meta,
        );
        self.messages.push(UiMessage {
            role: "user",
            content: display_question,
            api_content: Some(user_body),
            context_refs,
            commands: vec![],
        });
        self.chat_dirty = true;
        self.start_chat_request(ctx, app_settings);
        SendOutcome::Sent
    }

    fn start_chat_request(&mut self, ctx: &egui::Context, app_settings: &AppSettings) {
        let api_messages: Vec<ChatMessage> = self
            .messages
            .iter()
            .map(|m| ChatMessage {
                role: if m.role == "user" {
                    "user".to_string()
                } else {
                    "assistant".to_string()
                },
                content: if m.role == "user" {
                    m.api_content.as_ref().unwrap_or(&m.content).clone()
                } else {
                    m.content.clone()
                },
            })
            .collect();
        let settings = app_settings.ai.clone();
        let api_key = match self.effective_api_key(app_settings) {
            Some(k) => k,
            None => {
                self.last_error = Some(
                    i18n::tr(ctx, "Fill in and save API Key first", "请先填写并保存 API Key")
                        .to_string(),
                );
                return;
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        self.chat_cancel = Some(cancel.clone());
        self.background = Some(BackgroundJob::Chat { rx });
        self.busy = true;
        self.streaming = settings.stream_responses;
        self.last_error = None;
        self.messages.push(UiMessage {
            role: "assistant",
            content: String::new(),
            api_content: None,
            context_refs: vec![],
            commands: vec![],
        });
        thread::spawn(move || {
            run_chat_with_key(&settings, &api_key, &api_messages, &cancel, &tx, false);
        });
    }

    fn handle_message_action(
        &mut self,
        ctx: &egui::Context,
        app_settings: &AppSettings,
        action: AiMessageAction,
    ) {
        match action {
            AiMessageAction::Regenerate(idx) => {
                if self.busy || self.is_background_busy() {
                    return;
                }
                if idx >= self.messages.len() || self.messages[idx].role != "assistant" {
                    return;
                }
                self.messages.truncate(idx);
                if self.messages.last().is_some_and(|m| m.role == "user") {
                    self.chat_dirty = true;
                    self.start_chat_request(ctx, app_settings);
                }
            }
        }
    }

    fn poll_pending(&mut self, ctx: &egui::Context, app_settings: &mut AppSettings) {
        self.poll_models_fetch(ctx, app_settings);
        let Some(job) = &self.background else {
            return;
        };
        match job {
            BackgroundJob::Chat { rx, .. } => match rx.try_recv() {
                Ok(ChatEvent::Delta(chunk)) => {
                    if let Some(last) = self.messages.last_mut() {
                        if last.role == "assistant" {
                            last.content.push_str(&chunk);
                        }
                    }
                    self.streaming = true;
                    ctx.request_repaint();
                }
                Ok(ChatEvent::Finished) => {
                    if let Some(last) = self.messages.last_mut() {
                        if last.role == "assistant" {
                            last.commands = extract_shell_commands(&last.content);
                        }
                    }
                    self.background = None;
                    self.busy = false;
                    self.streaming = false;
                    self.chat_cancel = None;
                    self.input_status = None;
                    self.attached_contexts.clear();
                    self.chat_dirty = true;
                    if app_settings.ai.persist_chats {
                        self.flush_persisted_chat(false);
                    }
                    ctx.request_repaint();
                }
                Ok(ChatEvent::Failed(e)) => {
                    if self
                        .messages
                        .last()
                        .is_some_and(|m| m.role == "assistant" && m.content.is_empty())
                    {
                        self.messages.pop();
                    }
                    self.last_error = Some(i18n::localize_backend_error(i18n::language(ctx), &e));
                    self.input_status = None;
                    self.background = None;
                    self.busy = false;
                    self.streaming = false;
                    self.chat_cancel = None;
                    ctx.request_repaint();
                }
                Ok(ChatEvent::Cancelled) => {
                    if self
                        .messages
                        .last()
                        .is_some_and(|m| m.role == "assistant" && m.content.is_empty())
                    {
                        self.messages.pop();
                    }
                    self.input_status = Some(
                        i18n::tr(ctx, "Generation stopped", "已停止生成").to_string(),
                    );
                    self.background = None;
                    self.busy = false;
                    self.streaming = false;
                    self.chat_cancel = None;
                    ctx.request_repaint();
                }
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(80));
                }
                Err(TryRecvError::Disconnected) => {
                    self.last_error = Some(
                        i18n::tr(ctx, "Request interrupted", "请求已中断").to_string(),
                    );
                    self.background = None;
                    self.busy = false;
                    self.streaming = false;
                    self.chat_cancel = None;
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

enum AiMessageAction {
    Regenerate(usize),
}

enum SendOutcome {
    Sent,
    Empty,
    NotReady(String),
}

fn ai_message_side_padding(theme: &Theme) -> f32 {
    theme.spacing_md().max(12.0)
}

fn ai_content_width(ui: &egui::Ui) -> f32 {
    let w = ui.available_width().max(120.0);
    let bar = ui.spacing().scroll_bar_width;
    (w - bar - 4.0).max(96.0)
}

fn show_wrapped_user_text(ui: &mut egui::Ui, theme: &Theme, text: &str, width: f32) {
    let width = width.max(24.0);
    let font_size = theme.font_size_body();
    let text = user_text_with_soft_breaks(text, width, font_size);
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = width;
    job.append(
        &text,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(font_size),
            color: theme.text_primary(),
            ..Default::default()
        },
    );
    let galley = ui.ctx().fonts(|f| f.layout_job(job));
    let size = egui::vec2(width, galley.size().y.max(font_size * 1.45));
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().galley(rect.min, galley);
}

fn show_assistant_text(ui: &mut egui::Ui, theme: &Theme, text: &str, width: f32) {
    let width = width.max(24.0);
    let font_size = theme.font_size_body();
    let gap = theme.spacing_xs();
    let paragraph_gap = theme.spacing_sm().max(6.0);
    ui.set_max_width(width);

    let mut in_code = false;
    for raw in text.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if trimmed.is_empty() {
            ui.add_space(paragraph_gap);
            continue;
        }
        if in_code {
            continue;
        }

        let (line, strong, mono) = assistant_display_line(trimmed, in_code);
        if is_assistant_section_heading(&line) {
            ui.add_space(paragraph_gap);
            continue;
        }
        if is_command_like_line(&line) {
            // 命令已在下方“可执行命令”区域统一展示，正文里跳过，避免重复卡片。
            ui.add_space(paragraph_gap);
            continue;
        }
        let font = if mono {
            egui::FontId::monospace(theme.font_size_small())
        } else {
            egui::FontId::proportional(font_size)
        };
        let color = theme.text_primary();
        let wrap_font_size = if mono { theme.font_size_small() } else { font_size };
        for wrapped in wrap_text_for_units(&line, width, wrap_font_size) {
            let mut line_font = font.clone();
            if strong {
                line_font.size += 0.5;
            }
            paint_ai_text_line(ui, &wrapped, line_font, color, width);
            ui.add_space(gap);
        }
    }
}

fn break_shell_command_for_wrap(text: &str, width: f32, font_size: f32) -> String {
    let cols = (width / (font_size * 0.62)).floor() as usize;
    let cols = cols.clamp(6, 64);
    let mut out = String::with_capacity(text.len() + text.len() / cols.max(1));
    for (li, line) in text.lines().enumerate() {
        if li > 0 {
            out.push('\n');
        }
        let mut run = 0usize;
        for ch in line.chars() {
            out.push(ch);
            if ch.is_whitespace() || ch == '|' || ch == ';' || ch == '&' {
                run = 0;
                continue;
            }
            run += 1;
            if run >= cols {
                out.push('\u{200b}');
                run = 0;
            }
        }
    }
    out
}

fn paint_ai_text_line(
    ui: &mut egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    width: f32,
) {
    let width = width.max(24.0);
    let display = if font.family == egui::FontFamily::Monospace {
        break_shell_command_for_wrap(text, width, font.size)
    } else {
        text.to_string()
    };
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = width;
    job.append(
        &display,
        0.0,
        egui::TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    let galley = ui.ctx().fonts(|f| f.layout_job(job));
    let height = galley
        .size()
        .y
        .max(ui.text_style_height(&egui::TextStyle::Body));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter()
        .with_clip_rect(rect)
        .galley(rect.min, galley);
}

fn assistant_display_line(line: &str, in_code: bool) -> (String, bool, bool) {
    if in_code {
        return (line.to_string(), false, true);
    }
    let mut s = line.trim().to_string();
    let mut strong = false;
    if let Some(rest) = s.strip_prefix("* ") {
        s = format!("• {}", rest.trim());
    } else if let Some(rest) = s.strip_prefix("- ") {
        s = format!("• {}", rest.trim());
    }
    for prefix in ["### ", "## ", "# "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.trim().to_string();
            strong = true;
            break;
        }
    }
    s = s
        .replace("**", "")
        .replace("__", "")
        .replace('`', "");
    (s, strong, false)
}

fn wrap_text_for_units(line: &str, width: f32, font_size: f32) -> Vec<String> {
    let max_units = (width / (font_size * 0.58)).floor().max(8.0);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut units = 0.0f32;

    for token in split_wrap_tokens(line) {
        let token_units = text_units(&token);
        if units + token_units > max_units && !cur.trim().is_empty() {
            out.push(cur.trim_end().to_string());
            cur.clear();
            units = 0.0;
        }
        if token_units > max_units && !is_protected_token(token.trim()) {
            for ch in token.chars() {
                let u = char_units(ch);
                if units + u > max_units && !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    units = 0.0;
                }
                cur.push(ch);
                units += u;
            }
        } else {
            cur.push_str(&token);
            units += token_units;
        }
    }
    let cur = cur.trim_end();
    if !cur.is_empty() {
        out.push(cur.to_string());
    }
    out
}

fn split_wrap_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    for ch in line.chars() {
        cur.push(ch);
        if ch.is_whitespace()
            || matches!(ch, ',' | '，' | '。' | ':' | '：' | ';' | '；' | '/' | '|' | ')')
        {
            tokens.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

fn text_units(text: &str) -> f32 {
    text.chars().map(char_units).sum()
}

fn char_units(ch: char) -> f32 {
    if ch.is_ascii() { 0.56 } else { 1.0 }
}

fn is_protected_token(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let has_digit = token.chars().any(|c| c.is_ascii_digit());
    let dot_count = token.chars().filter(|&c| c == '.').count();
    if has_digit && dot_count >= 2 {
        return true;
    }
    token.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn is_assistant_section_heading(line: &str) -> bool {
    let s = line
        .trim()
        .trim_end_matches([':', '：'])
        .trim();
    matches!(
        s,
        "结论"
            | "关键点"
            | "风险"
            | "下一步"
            | "建议命令"
            | "可执行命令"
            | "Conclusion"
            | "Key points"
            | "Risks"
            | "Next steps"
            | "Suggested commands"
            | "Runnable commands"
    )
}

fn is_command_like_line(line: &str) -> bool {
    let s = line.trim();
    if s.starts_with('•') {
        return false;
    }
    let has_pipe = s.contains('|');
    let has_shell_tool = [" awk ", " sort ", " head ", " cut ", " ls ", " find ", " du "]
        .iter()
        .any(|needle| format!(" {s} ").contains(needle));
    has_pipe || has_shell_tool
}

fn user_text_with_soft_breaks(text: &str, width: f32, font_size: f32) -> String {
    let cols = (width / (font_size * 0.95)).floor() as usize;
    let cols = cols.clamp(8, 48);
    let mut out = String::with_capacity(text.len() + text.len() / cols.max(1));
    let mut run = 0usize;
    for ch in text.chars() {
        out.push(ch);
        if ch.is_whitespace() || ch == '-' || ch == '_' || ch == '/' || ch == '.' {
            run = 0;
            continue;
        }
        run += 1;
        if run >= cols {
            out.push('\u{200b}');
            run = 0;
        }
    }
    out
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
    if !is_runnable_shell_command(cmd) {
        return false;
    }
    let display = cmd
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if display.is_empty() {
        return false;
    }
    egui::Frame::none()
        .fill(theme.color_markdown_code_block_fill())
        .stroke(egui::Stroke::new(
            theme.hairline_width(ui.ctx()),
            theme.accent_alpha(90),
        ))
        .rounding(theme.radius_list_item())
        .inner_margin(egui::vec2(10.0, 8.0))
        .show(ui, |ui| {
            let row_w = ui.available_width().max(48.0);
            ui.set_max_width(row_w);
            ui.set_width(row_w);
            let wrapped = break_shell_command_for_wrap(
                &display,
                row_w,
                theme.font_size_small(),
            );
            paint_ai_text_line(
                ui,
                &wrapped,
                egui::FontId::monospace(theme.font_size_small()),
                theme.text_primary(),
                row_w,
            );
            ui.add_space(theme.spacing_sm());
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                clicked = crate::ui::chrome::panel_action_primary_button_with_icon_ex(
                    ui,
                    theme,
                    IconId::TerminalPrompt,
                    i18n::tr(ctx, "Send", "发送"),
                    true,
                )
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

fn message_copy_text(msg: &UiMessage) -> String {
    if msg.context_refs.is_empty() {
        return msg.content.clone();
    }
    let mut out = msg.content.clone();
    for (i, context) in msg.context_refs.iter().enumerate() {
        out.push_str(&format!(
            "\n\n--- Terminal context {} ---\n{}",
            i + 1,
            context.text
        ));
    }
    out
}

fn build_user_api_body(
    ctx: &egui::Context,
    question: &str,
    contexts: &[TerminalContextRef],
    meta: Option<&TerminalSessionMeta>,
    attach_meta: bool,
) -> String {
    let mut body = question.to_string();
    if attach_meta {
        if let Some(m) = meta.and_then(|m| m.format_block()) {
            body.push_str("\n\n");
            body.push_str(&m);
        }
    }
    if !contexts.is_empty() {
        body.push_str(i18n::tr(
            ctx,
            "\n\n--- Terminal context ---\n",
            "\n\n--- 终端上下文 ---\n",
        ));
        for (i, c) in contexts.iter().enumerate() {
            if contexts.len() > 1 {
                body.push_str(&format!("### {} {}\n", i + 1, i18n::tr(ctx, "Selection", "选区")));
            }
            body.push_str(&c.text);
            if i + 1 < contexts.len() {
                body.push('\n');
            }
        }
    }
    body
}

fn example_questions(ctx: &egui::Context) -> [&'static str; 3] {
    [
        i18n::tr(ctx, "Explain this error", "解释这条报错"),
        i18n::tr(ctx, "What should I run next?", "接下来该运行什么？"),
        i18n::tr(ctx, "Summarize this output", "总结这段输出"),
    ]
}

fn context_ref_to_stored(c: &TerminalContextRef) -> StoredContextRef {
    StoredContextRef {
        text: c.text.clone(),
        line_count: c.line_count,
        char_count: c.char_count,
        truncated: c.truncated,
        original_line_count: c.original_line_count,
        original_char_count: c.original_char_count,
        source_key: c.source_key.clone(),
    }
}

fn stored_to_context_ref(c: StoredContextRef) -> TerminalContextRef {
    TerminalContextRef {
        text: c.text,
        line_count: c.line_count,
        char_count: c.char_count,
        truncated: c.truncated,
        original_line_count: c.original_line_count,
        original_char_count: c.original_char_count,
        source_key: c.source_key,
    }
}

fn ui_message_to_stored(m: &UiMessage) -> StoredAiMessage {
    StoredAiMessage {
        role: m.role.to_string(),
        content: m.content.clone(),
        api_content: m.api_content.clone(),
        context_refs: m.context_refs.iter().map(context_ref_to_stored).collect(),
        commands: m.commands.clone(),
    }
}

fn stored_to_ui_message(m: StoredAiMessage) -> UiMessage {
    UiMessage {
        role: if m.role == "assistant" {
            "assistant"
        } else {
            "user"
        },
        content: m.content,
        api_content: m.api_content,
        context_refs: m
            .context_refs
            .into_iter()
            .map(stored_to_context_ref)
            .collect(),
        commands: m.commands,
    }
}

/// 终端选区引用芯片：对话区只显示链接式摘要，全文在弹出层查看（类似 Cursor @ 引用）。
fn show_terminal_context_chip(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    theme: &Theme,
    context: &TerminalContextRef,
    popup_id: egui::Id,
    index: usize,
    removable: bool,
    remove_clicked: &mut bool,
) {
    let label = context.chip_label(ctx, index);
    let chip_h = theme.size_panel_filter_chip_h();
    let font = egui::FontId::proportional(theme.font_size_small());
    let text_color = theme.accent_color();
    let icon_px = theme.font_size_small() + 1.0;
    let icon_gap = 4.0;
    let pad_x = 8.0;
    let text_w = ui
        .painter()
        .layout_no_wrap(label.clone(), font.clone(), text_color)
        .size()
        .x;
    let max_chip_w = ui.available_width().max(96.0);
    let chip_w = (pad_x * 2.0 + icon_px + icon_gap + text_w).min(max_chip_w);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(chip_w, chip_h), egui::Sense::click());
    let fill = if response.hovered() {
        theme.accent_alpha(24)
    } else {
        theme.accent_alpha(12)
    };
    ui.painter().rect(
        rect,
        theme.radius_category(),
        fill,
        egui::Stroke::new(1.0, theme.accent_alpha(24)),
    );
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + pad_x, rect.center().y - icon_px * 0.5),
        egui::vec2(icon_px, icon_px),
    );
    crate::ui::icons::paint_icon(ui, icon_rect, IconId::Attachment, text_color, icon_px);
    let text_x = icon_rect.max.x + icon_gap;
    ui.painter().with_clip_rect(rect.shrink(2.0)).galley(
        egui::pos2(text_x, rect.center().y - theme.font_size_small() * 0.5),
        ui.painter()
            .layout_no_wrap(label, font, text_color),
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        ui.memory_mut(|mem| mem.toggle_popup(popup_id));
    }
    egui::popup::popup_below_widget(ui, popup_id, &response, |ui| {
        show_context_popup_body(ui, ctx, theme, context, index);
    });
    if removable {
        ui.add_space(theme.spacing_xs());
        if crate::ui::chrome::panel_toolbar_icon_button(
            ui,
            theme,
            IconId::Trash,
            i18n::tr(ctx, "Clear context", "清除上下文"),
        )
        .clicked()
        {
            *remove_clicked = true;
        }
    }
}

fn show_context_popup_body(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    theme: &Theme,
    context: &TerminalContextRef,
    index: usize,
) {
    crate::ui::chrome::apply_menu_popup_style(ui, theme);
    ui.set_min_width(280.0);
    ui.set_max_width(520.0);
    ui.label(
        egui::RichText::new(context.chip_label(ctx, index))
            .size(theme.font_size_small())
            .strong(),
    );
    ui.add_space(theme.spacing_xs());
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(&context.text)
                    .monospace()
                    .size(theme.font_size_small()),
            );
        });
    ui.add_space(theme.spacing_xs());
    ui.horizontal(|ui| {
        if crate::ui::chrome::panel_action_button_with_icon_ex(
            ui,
            theme,
            IconId::File,
            i18n::tr(ctx, "Copy", "复制"),
            true,
        )
        .clicked()
        {
            if let Ok(mut clip) = Clipboard::new() {
                let _ = clip.set_text(context.text.clone());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_context_ref_counts_metadata() {
        let prep = prepare_terminal_context("line one\nline two\n");
        let r = TerminalContextRef::from_prepared(prep);
        assert_eq!(r.line_count, 2);
        assert!(r.char_count >= 16);
    }

    #[test]
    fn message_copy_text_includes_context_block() {
        let prep = prepare_terminal_context("err: fail");
        let ctx_ref = TerminalContextRef::from_prepared(prep);
        let msg = UiMessage {
            role: "user",
            content: "explain".to_string(),
            api_content: None,
            context_refs: vec![ctx_ref],
            commands: vec![],
        };
        let copied = message_copy_text(&msg);
        assert!(copied.contains("explain"));
        assert!(copied.contains("err: fail"));
    }
}
