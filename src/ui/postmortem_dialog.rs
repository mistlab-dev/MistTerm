//! 排障复盘草稿预览（#67）：AI 生成的 Markdown 草稿可编辑预览，
//! 人确认后「保存到 MistDocs」（用户 JWT + create_doc）。无自动保存路径。
//! 预览落在右侧 dock（非独立 modal），与其它辅助栏互斥。

use crate::core::postmortem::{
    create_document_blocking, postmortem_title, CreateDocumentRequest, PostmortemDraft,
};
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
    /// 宿主每帧同步的登录态：未登录时隐藏保存主按钮（已批方案：只保留复制，#67 review）。
    pub team_logged_in: bool,
    /// 本帧 SidePanel 槽位（Central 之后 Foreground 重绘用）。
    last_panel_slot_rect: Option<egui::Rect>,
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
            team_logged_in: false,
            last_panel_slot_rect: None,
        }
    }
}

impl PostmortemDialog {
    /// 打开预览：由 AI 生成完成后调用（草稿已在本地，未经确认不落库）。
    pub fn open_with_draft(
        &mut self,
        host: &str,
        date: &str,
        markdown: String,
        references: Vec<String>,
    ) {
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

    pub fn last_panel_slot_rect(&self) -> Option<egui::Rect> {
        self.last_panel_slot_rect
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

    /// 仅占右栏布局槽（正文在 Central 之后 [`show_foreground_panel`] 绘制）。
    pub fn show_side_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        right_dock_outer_left: &mut Option<f32>,
        dock_col_w: f32,
    ) {
        if !self.open {
            self.last_panel_slot_rect = None;
            return;
        }
        let (def_w, min_w, max_w) = layout_util::right_dock_resize_bounds(dock_col_w);
        ctx.root_ui(|root| {
            egui::Panel::right("postmortem_draft_panel")
                .default_size(def_w)
                .min_size(min_w)
                .max_size(max_w)
                .resizable(true)
                .frame(chrome::right_dock_placeholder_frame(theme))
                .show(root, |ui| {
                    let h = ui.available_height().max(1.0);
                    let w = ui.available_width().max(1.0);
                    ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
                });
        });
        let dock_inset = theme.spacing_right_dock_screen_inset();
        // #206 S1：Panel::show 不再返回 response；槽位改由 PanelState 落盘矩形推得。
        let slot = layout_util::side_panel_state_rect(ctx, "postmortem_draft_panel")
            .map(|r| {
                let screen = ctx.content_rect();
                let mut s = r;
                s.min.x = screen.max.x - s.width();
                s.max.x = screen.max.x - dock_inset;
                s
            })
            .unwrap_or(egui::Rect::NOTHING);
        chrome::paint_right_dock_slot_gap(ctx, theme, slot);
        self.last_panel_slot_rect = Some(slot);
        if let Some(slot) = self.last_panel_slot_rect {
            layout_util::record_right_dock_panel_rect(&slot, right_dock_outer_left);
        }
    }

    /// Central 之后绘制复盘草稿正文（右抽屉内嵌编辑）。
    pub fn show_foreground_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        close_panel: &mut bool,
    ) {
        if !self.open {
            return;
        }
        self.poll_save();

        let screen = ctx.content_rect();
        let dock_inset = theme.spacing_right_dock_screen_inset();
        let Some(slot) = layout_util::right_dock_foreground_slot(
            self.last_panel_slot_rect,
            ctx,
            "postmortem_draft_panel",
            layout_util::SidePanelProfile::Standard,
            None,
            dock_inset,
        ) else {
            return;
        };
        let geom = chrome::prepare_right_dock_foreground_geom(slot, screen, theme);
        let layer_id = chrome::right_dock_foreground_layer_id("mistterm_postmortem_fg");
        chrome::paint_right_dock_foreground_shell(ctx, layer_id, geom.paint, theme);

        let mut do_copy = false;
        let mut do_save = false;
        chrome::show_right_dock_foreground_body(
            "mistterm_postmortem_fg",
            ctx,
            theme,
            &geom,
            layout_util::SidePanelProfile::Standard,
            |ui, body_w| {
                let panel_w = body_w.max(1.0);
                ui.set_max_width(panel_w);
                let mut header_closed = false;
                theme.frame_right_dock_header_band().show(ui, |ui| {
                    header_closed = chrome::dock_panel_title_close_only(
                        ui,
                        theme,
                        crate::ui::icons::IconId::File,
                        crate::i18n::tr(ctx, "Postmortem draft", "复盘草稿"),
                        crate::i18n::tr(ctx, "Close postmortem draft", "关闭复盘草稿"),
                    );
                });
                if header_closed && self.phase != SavePhase::Saving {
                    *close_panel = true;
                }
                chrome::right_dock_header_divider(ui, theme);

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

                let content_w = (panel_w - 8.0).max(120.0);
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(crate::i18n::tr(ctx, "Title", "标题"))
                        .color(theme.text_secondary()),
                );
                let title_id = ui.make_persistent_id("postmortem_title_edit");
                ui.add(
                    egui::TextEdit::singleline(&mut self.draft.title)
                        .id(title_id)
                        .desired_width(content_w)
                        .font(egui::TextStyle::Body),
                );

                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(crate::i18n::tr(ctx, "Body (Markdown)", "正文（Markdown）"))
                        .color(theme.text_secondary()),
                );
                let body_id = ui.make_persistent_id("postmortem_body_edit");
                let scroll_h = layout_util::scroll_area_fill_height(ui, 96.0);
                egui::ScrollArea::vertical()
                    .id_salt("postmortem_body_scroll")
                    .max_height(scroll_h.max(160.0))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.draft.markdown)
                                .id(body_id)
                                .desired_width(content_w)
                                .desired_rows(18)
                                .code_editor()
                                .font(egui::TextStyle::Monospace),
                        );
                    });

                if let Some(err) = &self.error {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(err).color(theme.red_color()));
                }
                if let Some(doc_id) = &self.saved_doc_id {
                    ui.add_space(6.0);
                    let saved_msg = if doc_id.is_empty() {
                        crate::i18n::tr(ctx, "Saved to MistDocs", "已保存到 MistDocs").to_string()
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
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(crate::i18n::tr(ctx, "Copy Markdown", "仅复制 Markdown"))
                        .clicked()
                    {
                        do_copy = true;
                    }
                    if self.team_logged_in {
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
                                .min_size(egui::vec2(140.0, 28.0)),
                            )
                            .clicked()
                        {
                            do_save = true;
                        }
                    }
                });
            },
        );

        if do_copy {
            if let Ok(mut cb) = Clipboard::new() {
                let _ = cb.set_text(self.draft.markdown.clone());
            }
        }
        if do_save {
            self.pending_save = true;
        }
        if *close_panel && self.phase != SavePhase::Saving {
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
