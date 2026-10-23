//! 排障复盘草稿预览（#67）：AI 生成的 Markdown 草稿可编辑预览，
//! 人确认后「保存到 MistDocs」（用户 JWT + create_doc）。无自动保存路径。

use crate::core::postmortem::{
    create_document_blocking, postmortem_title, CreateDocumentRequest, PostmortemDraft,
};
use crate::core::TeamService;
use crate::ui::chrome;
use crate::ui::layout_util;
use crate::ui::theme::Theme;
use arboard::Clipboard;
use eframe::egui;
use std::sync::mpsc::Receiver;

/// 保存请求（后台线程 → UI）。
type SaveResult = Result<String, String>; // Ok(doc_id)

#[derive(PartialEq, Clone, Copy, Debug)]
enum SavePhase {
    Idle,
    Saving,
}

pub struct PostmortemDialog {
    pub open: bool,
    /// 草稿本体（标题 + 可编辑正文）。
    pub draft: PostmortemDraft,
    /// 本次排障的会话记录快照（生成用；保存后清空）。
    pub session_context: String,
    /// 保存结果通道（后台线程回传）。
    save_rx: Option<Receiver<SaveResult>>,
    phase: SavePhase,
    /// UI 点击保存后置位，由宿主 take 后用自己的 TeamService 执行。
    pending_save: bool,
    /// 保存成功后的文档 id（用于提示/打开链接），非空时禁止再次保存。
    saved_doc_id: Option<String>,
    /// 用户可见的错误信息。
    error: Option<String>,
    /// 生成中的状态占位（生成走 AI 面板链路，这里只显示状态）。
    generating: bool,
}

impl Default for PostmortemDialog {
    fn default() -> Self {
        Self {
            open: false,
            draft: PostmortemDraft {
                title: String::new(),
                markdown: String::new(),
                references: Vec::new(),
            },
            session_context: String::new(),
            save_rx: None,
            phase: SavePhase::Idle,
            pending_save: false,
            saved_doc_id: None,
            error: None,
            generating: false,
        }
    }
}

impl PostmortemDialog {
    /// 打开预览：由 AI 面板生成完成后调用（草稿已在本地，未经确认不落库）。
    pub fn open_with_draft(&mut self, host: &str, date: &str, markdown: String, references: Vec<String>) {
        let markdown = crate::core::postmortem::with_references(&markdown, &references);
        self.draft = PostmortemDraft {
            title: postmortem_title(host, date),
            markdown,
            references,
        };
        self.saved_doc_id = None;
        self.error = None;
        self.phase = SavePhase::Idle;
        self.generating = false;
        self.open = true;
    }

    pub fn set_generating(&mut self, on: bool) {
        self.generating = on;
        if on {
            self.open = true;
        }
    }

    pub fn is_generating(&self) -> bool {
        self.generating
    }

    pub fn is_saving(&self) -> bool {
        self.phase == SavePhase::Saving
    }

    /// 发起后台保存。由宿主在 take_pending_save 后调用（登录态由宿主提供）。
    pub fn begin_save_with_creds(
        &mut self,
        api_base: &str,
        access_token: &str,
        team_id: &str,
    ) {
        if self.phase == SavePhase::Saving || self.saved_doc_id.is_some() {
            return;
        }
        self.error = None;
        if let Some(err) = self.draft.validation_error() {
            self.error = Some(err.to_string());
            return;
        }
        let req = CreateDocumentRequest {
            title: self.draft.title.trim().to_string(),
            r#type: "doc".to_string(),
            folder_id: String::new(),
            content: self.draft.markdown.clone(),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        self.save_rx = Some(rx);
        self.phase = SavePhase::Saving;
        let api_base = api_base.to_string();
        let token = access_token.to_string();
        let team_id = team_id.to_string();
        std::thread::spawn(move || {
            let result =
                create_document_blocking(&api_base, &token, &team_id, &req).map(|d| d.id);
            let _ = tx.send(result);
        });
    }

    /// 每帧泵一次后台保存结果。
    pub fn poll_save(&mut self) {
        if self.phase != SavePhase::Saving {
            return;
        }
        let Some(rx) = &self.save_rx else {
            self.phase = SavePhase::Idle;
            return;
        };
        match rx.try_recv() {
            Ok(Ok(doc_id)) => {
                self.saved_doc_id = Some(doc_id);
                self.phase = SavePhase::Idle;
                self.save_rx = None;
            }
            Ok(Err(e)) => {
                // 失败：草稿留在编辑框，错误可见，不丢内容。
                self.error = Some(e);
                self.phase = SavePhase::Idle;
                self.save_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.error = Some("Save worker lost".to_string());
                self.phase = SavePhase::Idle;
                self.save_rx = None;
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme) {
        if !self.open {
            return;
        }
        self.poll_save();
        let mut should_close = false;
        let mut do_copy = false;
        let mut do_save = false;
        let r = ctx.screen_rect();
        let sw = r.width().max(360.0);
        let sh = r.height().max(280.0);
        let modal_size = egui::vec2(
            (sw * 0.56).clamp(560.0, 900.0),
            (sh * 0.64).clamp(440.0, 720.0),
        );
        let default_pos = egui::pos2(
            r.min.x + (sw - modal_size.x) * 0.5,
            r.min.y + (sh - modal_size.y) * 0.5,
        );
        let title = crate::i18n::tr(ctx, "Postmortem draft", "复盘草稿").to_string();
        chrome::modal_window("postmortem_draft", theme, ctx)
            .id(egui::Id::new("postmortem_draft_window"))
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
                    if self.generating {
                        ui.add_space(12.0);
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(
                                ctx,
                                "Generating draft from this session…",
                                "正在根据本会话生成草稿…",
                            ))
                            .color(theme.text_secondary()),
                        );
                        return;
                    }
                    // 标题编辑
                    ui.add_space(4.0);
                    let title_w = content_w;
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(ctx, "Title", "标题"))
                                .color(theme.text_secondary()),
                        );
                    });
                    let title_id = ui.make_persistent_id("postmortem_title_edit");
                    let title_edit = egui::TextEdit::singleline(&mut self.draft.title)
                        .id(title_id)
                        .desired_width(title_w)
                        .font(egui::TextStyle::Body);
                    ui.add(title_edit);
                    // 正文编辑（Markdown，等宽）
                    ui.add_space(8.0);
                    let body_h = modal_size.y * 0.52;
                    let body_id = ui.make_persistent_id("postmortem_body_edit");
                    let body_edit = egui::TextEdit::multiline(&mut self.draft.markdown)
                        .id(body_id)
                        .desired_width(content_w)
                        .desired_rows(12)
                        .code_editor()
                        .font(egui::TextStyle::Monospace);
                    ui.add_sized([content_w, body_h], body_edit);
                    // 错误条
                    if let Some(err) = &self.error {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(err).color(theme.red_color()));
                    }
                    if let Some(doc_id) = &self.saved_doc_id {
                        ui.add_space(6.0);
                        let saved_msg = if doc_id.is_empty() {
                            crate::i18n::tr(
                                ctx,
                                "Saved to MistDocs",
                                "已保存到 MistDocs",
                            )
                            .to_string()
                        } else {
                            format!(
                                "{}{doc_id}",
                                crate::i18n::tr(
                                    ctx,
                                    "Saved to MistDocs (doc ",
                                    "已保存到 MistDocs（文档 "
                                )
                            )
                        };
                        ui.label(egui::RichText::new(saved_msg).color(theme.green_color()));
                    }
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // 主按钮：保存到 MistDocs（必须经预览；无自动保存路径）
                        let can_save = self.phase == SavePhase::Idle
                            && self.saved_doc_id.is_none()
                            && self.draft.validation_error().is_none();
                        let save_label = if self.phase == SavePhase::Saving {
                            crate::i18n::tr(ctx, "Saving…", "保存中…").to_string()
                        } else {
                            crate::i18n::tr(ctx, "Save to MistDocs", "保存到 MistDocs").to_string()
                        };
                        if ui
                            .add_enabled(
                                can_save,
                                egui::Button::new(
                                    egui::RichText::new(save_label)
                                        .color(theme.text_primary())
                                        .strong(),
                                )
                                .fill(theme.accent_color())
                                .min_size(egui::vec2(150.0, 30.0)),
                            )
                            .clicked()
                        {
                            do_save = true;
                        }
                        // 次按钮：仅复制 Markdown
                        if ui
                            .button(crate::i18n::tr(ctx, "Copy Markdown", "仅复制 Markdown"))
                            .clicked()
                        {
                            do_copy = true;
                        }
                    });
                });
            });
        if do_copy {
            if let Ok(mut cb) = Clipboard::new() {
                let _ = cb.set_text(self.draft.markdown.clone());
            }
        }
        if do_save {
            self.pending_save = true;
        }
        if should_close && self.phase != SavePhase::Saving {
            self.open = false;
        }
    }

    /// 宿主取走保存请求并用自己的 TeamService 执行（对话框不持有登录态）。
    pub fn take_pending_save(&mut self) -> bool {
        std::mem::replace(&mut self.pending_save, false)
    }

    /// 生成失败/为空时由宿主写入错误条（#67）；草稿内容不变，不产生半截文档。
    pub fn show_error(&mut self, msg: &str) {
        self.error = Some(msg.to_string());
    }
}
