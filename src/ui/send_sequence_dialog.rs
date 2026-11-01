//! 发送序列管理。入口和文案与命令片段分开：这里是按行键入，不是带占位符的片段。

use eframe::egui;

use crate::core::send_sequence::{SendSequence, SendSequenceStore, SequenceError, MAX_DELAY_MS};
use crate::ui::theme::Theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendSequenceUiAction {
    None,
    Replay(String),
    Stop,
}

pub struct SendSequenceDialog {
    pub open: bool,
    selected_id: Option<String>,
    name: String,
    body: String,
    delay_ms: u32,
    error: String,
}

impl Default for SendSequenceDialog {
    fn default() -> Self {
        Self {
            open: false,
            selected_id: None,
            name: String::new(),
            body: String::new(),
            delay_ms: 0,
            error: String::new(),
        }
    }
}

impl SendSequenceDialog {
    pub fn open_manager(&mut self, store: &SendSequenceStore) {
        self.open = true;
        self.error.clear();
        if self.selected_id.is_none() {
            if let Some(first) = store.list().first() {
                self.load_sequence(first);
            }
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        store: &mut SendSequenceStore,
        session_connected: bool,
        replaying_id: Option<&str>,
    ) -> SendSequenceUiAction {
        if !self.open {
            return SendSequenceUiAction::None;
        }
        if let Some(id) = self.selected_id.clone() {
            if store.get(&id).is_none() {
                self.clear_form();
            }
        }
        let mut action = SendSequenceUiAction::None;
        let mut keep_open = self.open;
        let mut should_close = false;
        let modal_sz = egui::vec2(760.0, 560.0);
        crate::ui::chrome::modal_window("send_sequence_dialog", theme, ctx)
            .open(&mut keep_open)
            .default_size(modal_sz)
            .min_width(560.0)
            .movable(true)
            .resizable(true)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    if crate::ui::chrome::modal_header(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Send sequences", "发送序列"),
                        theme.font_size_modal_title(),
                    ) {
                        should_close = true;
                    }
                    ui.label(
                        egui::RichText::new(crate::i18n::tr(
                            ctx,
                            "Sends each line, in order, into the current connected pane. This is not a command snippet: snippets are searchable commands with placeholders.",
                            "按顺序把每一行发进当前已连接的窗格. 这不是命令片段: 片段是带占位符、可以检索的命令.",
                        ))
                        .size(theme.font_size_caption())
                        .color(theme.text_tertiary()),
                    );
                    ui.add_space(theme.spacing_sm());
                    let content_w = crate::ui::layout_util::finite_content_width(ui);
                    ui.set_min_width(content_w);
                    ui.horizontal(|ui| {
                        self.paint_list(ui, theme, ctx, store, content_w * 0.34);
                        ui.separator();
                        self.paint_editor(
                            ui,
                            theme,
                            ctx,
                            store,
                            content_w * 0.62,
                            session_connected,
                            replaying_id,
                            &mut action,
                        );
                    });
                    if !self.error.is_empty() {
                        ui.add_space(theme.spacing_sm());
                        ui.label(
                            egui::RichText::new(&self.error)
                                .size(theme.font_size_caption())
                                .color(theme.color_danger_emphasis()),
                        );
                    }
                    ui.add_space(theme.spacing_sm());
                    egui::CollapsingHeader::new(crate::i18n::tr(
                        ctx,
                        "Snippets, mist exec, AI batch, or this",
                        "片段, mist exec, AI 批量, 还是发送序列",
                    ))
                    .id_source("send_sequence_compare")
                    .default_open(false)
                    .show(ui, |ui| {
                        paint_difference(ui, theme, ctx);
                    });
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            should_close = true;
        }
        self.open = keep_open && !should_close;
        action
    }

    fn paint_list(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        store: &SendSequenceStore,
        width: f32,
    ) {
        ui.vertical(|ui| {
            ui.set_width(width.max(160.0));
            if crate::ui::chrome::panel_header_new_button_with_label(
                ui,
                theme,
                crate::i18n::tr(ctx, "New", "新建"),
            )
            .clicked()
            {
                self.clear_form();
            }
            ui.add_space(theme.spacing_xs());
            egui::ScrollArea::vertical()
                .id_source("send_sequence_list")
                .max_height(360.0)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    if store.list().is_empty() {
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(
                                ctx,
                                "No send sequences yet. Create one to send multiple lines, in order, to the current connected session.",
                                "还没有发送序列。新建一条，把多行命令按顺序发到当前已连接的会话。",
                            ))
                            .size(theme.font_size_caption())
                            .color(theme.text_tertiary()),
                        );
                        return;
                    }
                    let mut pick: Option<String> = None;
                    for seq in store.list() {
                        let selected = self.selected_id.as_deref() == Some(seq.id.as_str());
                        let resp = ui.selectable_label(selected, &seq.name);
                        if resp.clicked() {
                            pick = Some(seq.id.clone());
                        }
                    }
                    if let Some(id) = pick {
                        if let Some(seq) = store.get(&id) {
                            self.load_sequence(seq);
                        }
                    }
                });
        });
    }

    fn paint_editor(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        store: &mut SendSequenceStore,
        width: f32,
        session_connected: bool,
        replaying_id: Option<&str>,
        action: &mut SendSequenceUiAction,
    ) {
        ui.vertical(|ui| {
            let width = width.max(280.0);
            ui.set_width(width);
            crate::ui::chrome::form_field_label(ui, theme, crate::i18n::tr(ctx, "Name", "名称"));
            crate::ui::chrome::form_singleline_field(
                ui,
                theme,
                egui::Id::new("send_sequence_name"),
                &mut self.name,
                crate::i18n::tr(ctx, "After login", "登录后"),
                width,
                false,
            );
            ui.add_space(theme.spacing_sm());
            crate::ui::chrome::form_control_row(ui, theme, |ui| {
                crate::ui::chrome::form_inline_label(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Delay between lines (ms)", "行间间隔 (毫秒)"),
                );
                crate::ui::chrome::form_u32_stepper(
                    ui,
                    theme,
                    egui::Id::new("send_sequence_delay"),
                    &mut self.delay_ms,
                    0..=MAX_DELAY_MS,
                    50,
                );
            });
            ui.label(
                egui::RichText::new(crate::i18n::tr(
                    ctx,
                    "Fixed wait only. Does not wait for a remote prompt. 0 sends the next line immediately.",
                    "只做固定等待, 不会等远端提示符. 0 表示上一行发出后马上发下一行.",
                ))
                .size(theme.font_size_caption())
                .color(theme.text_tertiary()),
            );
            ui.add_space(theme.spacing_sm());
            crate::ui::chrome::form_field_label(
                ui,
                theme,
                crate::i18n::tr(ctx, "Lines to send", "要发送的行"),
            );
            crate::ui::chrome::form_multiline_field_with_hint(
                ui,
                theme,
                egui::Id::new("send_sequence_body"),
                &mut self.body,
                crate::i18n::tr(
                    ctx,
                    "One line per send. A blank line presses Enter.",
                    "一行发一次. 空行就是按一次回车.",
                ),
                width,
                8,
                false,
            );
            ui.add_space(theme.spacing_sm());
            ui.label(
                egui::RichText::new(crate::i18n::tr(
                    ctx,
                    "Sends only to the current pane, even if broadcast input is on.",
                    "只发到当前这一个窗格. 即使开了广播输入, 也不会发到其它窗格.",
                ))
                .size(theme.font_size_caption())
                .color(theme.text_tertiary()),
            );
            ui.add_space(theme.spacing_sm());
            let sending_this = replaying_id.is_some()
                && self.selected_id.as_deref() == replaying_id;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm();
                if crate::ui::chrome::panel_action_primary_button(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Save", "保存"),
                )
                .clicked()
                {
                    let _ = self.save_current(ctx, store);
                }
                if crate::ui::chrome::panel_action_button_ex(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Delete", "删除"),
                    self.selected_id.is_some(),
                )
                .clicked()
                {
                    if let Some(id) = self.selected_id.clone() {
                        store.delete(&id);
                        if let Err(e) = store.save() {
                            self.error = format!(
                                "{} {e}",
                                crate::i18n::tr(ctx, "Could not save:", "保存失败:")
                            );
                        } else {
                            self.clear_form();
                        }
                    }
                }
                let send_label = if sending_this {
                    crate::i18n::tr(ctx, "Sending…", "正在发送…")
                } else {
                    crate::i18n::tr(ctx, "Send to this session", "发到当前会话")
                };
                let send = crate::ui::chrome::panel_action_primary_button_ex(
                    ui,
                    theme,
                    send_label,
                    session_connected && !sending_this,
                );
                let send_clicked = send.clicked();
                if !session_connected {
                    send.on_hover_text(crate::i18n::tr(
                        ctx,
                        "Connect a session first",
                        "请先连接一个会话",
                    ));
                }
                if send_clicked {
                    match self.save_current(ctx, store) {
                        Ok(id) => *action = SendSequenceUiAction::Replay(id),
                        Err(_) => {}
                    }
                }
                if replaying_id.is_some()
                    && crate::ui::chrome::panel_action_button(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Stop", "停止"),
                    )
                    .clicked()
                {
                    *action = SendSequenceUiAction::Stop;
                }
            });
        });
    }

    fn save_current(
        &mut self,
        ctx: &egui::Context,
        store: &mut SendSequenceStore,
    ) -> Result<String, ()> {
        let saved = if let Some(id) = self.selected_id.clone() {
            store.update(&id, &self.name, &self.body, self.delay_ms)
        } else {
            store.create(&self.name, &self.body, self.delay_ms)
        };
        match saved {
            Ok(seq) => {
                if let Err(e) = store.save() {
                    self.error = format!(
                        "{} {e}",
                        crate::i18n::tr(ctx, "Could not save:", "保存失败:")
                    );
                    Err(())
                } else {
                    self.error.clear();
                    self.load_sequence(&seq);
                    Ok(seq.id)
                }
            }
            Err(err) => {
                self.error = sequence_error_text(ctx, err).to_string();
                Err(())
            }
        }
    }

    fn load_sequence(&mut self, seq: &SendSequence) {
        self.selected_id = Some(seq.id.clone());
        self.name = seq.name.clone();
        self.body = seq.body.clone();
        self.delay_ms = seq.delay_ms;
        self.error.clear();
    }

    fn clear_form(&mut self) {
        self.selected_id = None;
        self.name.clear();
        self.body.clear();
        self.delay_ms = 0;
        self.error.clear();
    }
}

fn sequence_error_text(ctx: &egui::Context, err: SequenceError) -> &'static str {
    match err {
        SequenceError::EmptyName => crate::i18n::tr(ctx, "Enter a name", "请填写名称"),
        SequenceError::NameTooLong => crate::i18n::tr(ctx, "Name is too long", "名称过长"),
        SequenceError::DuplicateName => crate::i18n::tr(
            ctx,
            "A sequence with this name already exists",
            "已有同名发送序列",
        ),
        SequenceError::EmptyBody => crate::i18n::tr(ctx, "Add at least one line", "至少写一行"),
        SequenceError::BodyTooLarge => crate::i18n::tr(ctx, "Text is too long", "内容过长"),
        SequenceError::TooManyLines => {
            crate::i18n::tr(ctx, "Too many lines (200 max)", "行数过多 (最多 200 行)")
        }
        SequenceError::DelayTooLong => crate::i18n::tr(
            ctx,
            "Delay must be 10000 ms or less",
            "行间间隔不能超过 10000 毫秒",
        ),
        SequenceError::TooMany => crate::i18n::tr(
            ctx,
            "Too many sequences (64 max)",
            "发送序列过多 (最多 64 条)",
        ),
        SequenceError::NotFound => {
            crate::i18n::tr(ctx, "Sequence was deleted", "这条发送序列已删除")
        }
    }
}

fn paint_difference(ui: &mut egui::Ui, theme: &Theme, ctx: &egui::Context) {
    let rows: [(&str, &str, &str, &str); 4] = [
        (
            "Command snippets",
            "命令片段",
            "One searchable command, often with <placeholders>. Fill them in, then send to the current terminal.",
            "一条可检索的命令, 常带 <占位符>. 填完再发到当前终端.",
        ),
        (
            "mist exec",
            "mist exec",
            "CLI: run on one or many hosts over a separate SSH connection and collect output. Does not type into an open terminal.",
            "命令行对一台或多台主机另开连接执行, 并收回输出. 不往已打开的终端里敲.",
        ),
        (
            "AI batch",
            "AI 批量",
            "Describe a task in words. After you confirm, it runs on the hosts in the plan. Not a fixed key sequence.",
            "用自然语言描述任务, 确认后按计划在多台主机执行. 不是写死的按键顺序.",
        ),
        (
            "Send sequences",
            "发送序列",
            "After you are connected, send these lines in order to this one pane. Optional fixed delay. Does not wait for a prompt and does not broadcast.",
            "连上之后, 按顺序把这几行发进当前这一个窗格. 可选固定间隔. 不等提示符, 也不广播.",
        ),
    ];
    for (en_title, zh_title, en_body, zh_body) in rows {
        ui.add_space(theme.spacing_xs());
        ui.label(
            egui::RichText::new(crate::i18n::tr(ctx, en_title, zh_title))
                .size(theme.font_size_caption())
                .strong()
                .color(theme.text_primary()),
        );
        ui.label(
            egui::RichText::new(crate::i18n::tr(ctx, en_body, zh_body))
                .size(theme.font_size_caption())
                .color(theme.text_secondary()),
        );
    }
}
