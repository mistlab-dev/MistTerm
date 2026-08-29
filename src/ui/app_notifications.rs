use super::MistTermApp;
use eframe::egui;
use std::time::{Duration, Instant};

/// Leading marker for transient error status styling (invisible); avoids locale-sensitive `starts_with`.
pub(crate) const STATUS_ERROR_MARKER: char = '\u{200b}';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToastKind {
    Info,
    Success,
    Warn,
    Error,
}

/// Toast 主按钮动作（需用户确认的提示）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToastAction {
    /// 打开 SSH 配置导入对话框。
    OpenSshImport,
    /// 重连指定标签的 SSH（连接失败 / 自动重连放弃后）。
    ReconnectTab { tab_idx: usize },
    /// 将审计拦截后的合规片段建议插入终端（内容在 `pending_suggested_snippet`）。
    InsertSuggestedSnippet,
    /// 将合规片段建议沉底到个人库（团队片段副本 / 去重后写入）。
    SaveSuggestedSnippet,
}

#[derive(Debug, Clone)]
pub(super) struct ActiveToast {
    kind: ToastKind,
    /// 标题行（级别语义，如「命令被禁止」）。
    title: String,
    /// 正文行（命令预览、详情等）。
    text: String,
    /// `None` = 需用户确认，不自动消失。
    until: Option<Instant>,
    action: Option<ToastAction>,
    action_label: Option<String>,
    secondary_action: Option<ToastAction>,
    secondary_action_label: Option<String>,
}

#[inline]
pub(crate) fn status_message_body(msg: &str) -> &str {
    msg.strip_prefix(STATUS_ERROR_MARKER).unwrap_or(msg)
}

pub(crate) fn status_message_wrap_error(display: impl Into<String>) -> String {
    let s = display.into();
    if s.starts_with(STATUS_ERROR_MARKER) {
        return s;
    }
    format!("{STATUS_ERROR_MARKER}{s}")
}

/// 根据文案推断是否为错误类通知（兼容旧 `status_message` 赋值）。
pub(crate) fn status_message_looks_like_error(msg: &str) -> bool {
    let body = status_message_body(msg);
    msg.starts_with(STATUS_ERROR_MARKER)
        || body.starts_with("Expression error")
        || body.starts_with("表达式错误")
        || body.starts_with("Insert failed")
        || body.starts_with("插入失败")
        || body.starts_with("Upload failed")
        || body.starts_with("上传失败")
        || body.starts_with("File upload failed")
        || body.starts_with("文件上传失败")
        || body.starts_with("Save failed")
        || body.starts_with("保存失败")
        || body.starts_with("Failed to parse credential")
        || body.starts_with("解析凭据失败")
        || body.starts_with("Failed to update session")
        || body.starts_with("更新会话失败")
        || body.starts_with("Failed to open")
        || body.starts_with("无法打开")
        || body.starts_with("Failed to prepare")
        || body.starts_with("无法准备")
        || (body.starts_with("ZMODEM") && body.contains("failed"))
        || (body.starts_with("SCP ") && body.contains("failed"))
        || (body.contains("ZMODEM") && body.contains("失败"))
        || (body.starts_with("SCP ") && body.contains("失败"))
        || body.contains(" failed")
        || body.contains("失败")
}

/// 提示文案颜色：错误类用主题红，其余用弱文字色。
pub(super) fn status_message_text_color(
    msg: &str,
    theme: &crate::ui::theme::Theme,
) -> egui::Color32 {
    if status_message_looks_like_error(msg) {
        theme.red_color()
    } else {
        theme.color_caption_text()
    }
}

/// 级别默认标题（无显式 title 时使用）。
pub(crate) fn default_toast_title(ctx: &egui::Context, kind: ToastKind) -> String {
    match kind {
        ToastKind::Error => crate::i18n::tr(ctx, "Error", "错误").to_string(),
        ToastKind::Warn => crate::i18n::tr(ctx, "Warning", "警告").to_string(),
        ToastKind::Success => crate::i18n::tr(ctx, "Success", "完成").to_string(),
        ToastKind::Info => crate::i18n::tr(ctx, "Notice", "提示").to_string(),
    }
}

fn status_payload(title: &str, text: &str) -> String {
    if title.is_empty() {
        text.to_string()
    } else if text.is_empty() {
        title.to_string()
    } else {
        format!("{title}\n{text}")
    }
}

impl MistTermApp {
    fn toast_duration_secs(kind: ToastKind) -> u64 {
        // 业界常见：Info ~3–5s，Warn/Error 略长；过长会挡操作，过短读不完。
        match kind {
            ToastKind::Error => 8,
            ToastKind::Warn => 7,
            ToastKind::Success | ToastKind::Info => 5,
        }
    }

    /// 统一通知入口：所有用户可见提示走 Toast（并同步 `status_message` 供诊断快照）。
    pub(crate) fn push_toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.push_toast_titled(kind, String::new(), text);
    }

    /// 标题 + 正文 Toast（标题空则绘制时用级别默认标题）。
    pub(crate) fn push_toast_titled(
        &mut self,
        kind: ToastKind,
        title: impl Into<String>,
        text: impl Into<String>,
    ) {
        let title = title.into().trim().to_string();
        let raw = text.into();
        let text = status_message_body(&raw).trim().to_string();
        if title.is_empty() && text.is_empty() {
            self.active_toast = None;
            self.status_message.clear();
            return;
        }
        // 需确认的 Toast 不被瞬时提示覆盖（过期后会再同步）。
        if self
            .active_toast
            .as_ref()
            .is_some_and(|t| t.action.is_some() && t.until.is_none())
        {
            return;
        }
        let payload = status_payload(&title, &text);
        self.status_message = match kind {
            ToastKind::Error => status_message_wrap_error(payload),
            _ => payload,
        };
        self.active_toast = Some(ActiveToast {
            kind,
            title,
            text,
            until: Some(Instant::now() + Duration::from_secs(Self::toast_duration_secs(kind))),
            action: None,
            action_label: None,
            secondary_action: None,
            secondary_action_label: None,
        });
    }

    /// 需用户确认的 Toast：不自动消失，带主操作与关闭。
    pub(crate) fn push_action_toast(
        &mut self,
        kind: ToastKind,
        text: impl Into<String>,
        action: ToastAction,
        action_label: impl Into<String>,
    ) {
        self.push_dual_action_toast(kind, text, action, action_label, None, None::<String>);
    }

    pub(crate) fn push_action_toast_titled(
        &mut self,
        kind: ToastKind,
        title: impl Into<String>,
        text: impl Into<String>,
        action: ToastAction,
        action_label: impl Into<String>,
    ) {
        self.push_dual_action_toast_titled(
            kind,
            title,
            text,
            action,
            action_label,
            None,
            None::<String>,
        );
    }

    /// 需用户确认的 Toast：主操作 + 可选次要操作（如「用到终端」+「存到个人库」）。
    pub(crate) fn push_dual_action_toast(
        &mut self,
        kind: ToastKind,
        text: impl Into<String>,
        action: ToastAction,
        action_label: impl Into<String>,
        secondary_action: Option<ToastAction>,
        secondary_action_label: Option<impl Into<String>>,
    ) {
        self.push_dual_action_toast_titled(
            kind,
            String::new(),
            text,
            action,
            action_label,
            secondary_action,
            secondary_action_label,
        );
    }

    pub(crate) fn push_dual_action_toast_titled(
        &mut self,
        kind: ToastKind,
        title: impl Into<String>,
        text: impl Into<String>,
        action: ToastAction,
        action_label: impl Into<String>,
        secondary_action: Option<ToastAction>,
        secondary_action_label: Option<impl Into<String>>,
    ) {
        let title = title.into().trim().to_string();
        let raw = text.into();
        let text = status_message_body(&raw).trim().to_string();
        if title.is_empty() && text.is_empty() {
            return;
        }
        let action_label = action_label.into();
        let secondary_action_label = secondary_action_label.map(|s| s.into());
        if let Some(toast) = &self.active_toast {
            if toast.action == Some(action)
                && toast.until.is_none()
                && toast.title == title
                && toast.text == text
                && toast.action_label.as_deref() == Some(action_label.as_str())
                && toast.secondary_action == secondary_action
                && toast.secondary_action_label == secondary_action_label
            {
                return;
            }
        }
        let payload = status_payload(&title, &text);
        self.status_message = match kind {
            ToastKind::Error => status_message_wrap_error(payload),
            _ => payload,
        };
        self.active_toast = Some(ActiveToast {
            kind,
            title,
            text,
            until: None,
            action: Some(action),
            action_label: Some(action_label),
            secondary_action,
            secondary_action_label,
        });
    }

    pub(crate) fn notify_info(&mut self, text: impl Into<String>) {
        self.push_toast(ToastKind::Info, text);
    }

    pub(crate) fn notify_success(&mut self, text: impl Into<String>) {
        self.push_toast(ToastKind::Success, text);
    }

    pub(crate) fn notify_warn(&mut self, text: impl Into<String>) {
        self.push_toast(ToastKind::Warn, text);
    }

    pub(crate) fn notify_error(&mut self, text: impl Into<String>) {
        self.push_toast(ToastKind::Error, text);
    }

    pub(crate) fn notify_info_titled(
        &mut self,
        title: impl Into<String>,
        text: impl Into<String>,
    ) {
        self.push_toast_titled(ToastKind::Info, title, text);
    }

    pub(crate) fn notify_warn_titled(
        &mut self,
        title: impl Into<String>,
        text: impl Into<String>,
    ) {
        self.push_toast_titled(ToastKind::Warn, title, text);
    }

    pub(crate) fn notify_error_titled(
        &mut self,
        title: impl Into<String>,
        text: impl Into<String>,
    ) {
        self.push_toast_titled(ToastKind::Error, title, text);
    }

    /// 兼容旧赋值：自动区分错误 / 普通提示。
    pub(crate) fn notify_auto(&mut self, text: impl Into<String>) {
        let s = text.into();
        if status_message_looks_like_error(&s) {
            self.notify_error(s);
        } else {
            self.notify_info(s);
        }
    }

    pub(crate) fn clear_toast(&mut self) {
        self.active_toast = None;
        self.status_message.clear();
        self.pending_suggested_snippet = None;
    }

    /// 有待导入且用户未关闭时，展示可操作 Toast（替代侧栏横幅 / 顶栏 chip）。
    pub(crate) fn sync_ssh_import_action_toast(&mut self, ctx: &egui::Context) {
        let pending = self.ssh_pending_import_count();
        if pending == 0 || self.ssh_import_banner_dismissed {
            if self
                .active_toast
                .as_ref()
                .is_some_and(|t| t.action == Some(ToastAction::OpenSshImport))
            {
                self.clear_toast();
            }
            return;
        }
        if self
            .active_toast
            .as_ref()
            .is_some_and(|t| t.action.is_none() && t.until.is_some())
        {
            // 瞬时 Toast 优先展示；结束后下一帧再同步待导入。
            return;
        }
        let title = crate::i18n::tr(ctx, "SSH import", "SSH 导入").to_string();
        let text = match crate::i18n::language(ctx) {
            crate::i18n::UiLanguage::En => {
                format!("Detected {pending} pending Host block(s)")
            }
            crate::i18n::UiLanguage::Zh => format!("检测到 {pending} 个未导入的 Host 配置"),
        };
        let action_label = crate::i18n::tr(ctx, "Import", "导入").to_string();
        self.push_action_toast_titled(
            ToastKind::Warn,
            title,
            text,
            ToastAction::OpenSshImport,
            action_label,
        );
    }

    /// 刷新 Toast 过期；并桥接仍直接写 `status_message` 的旧路径。
    pub(crate) fn tick_status_toast(&mut self) {
        let toast_payload = self
            .active_toast
            .as_ref()
            .map(|t| status_payload(&t.title, &t.text))
            .unwrap_or_default();
        let body = status_message_body(&self.status_message);
        if body != toast_payload {
            if body.is_empty() {
                if self
                    .active_toast
                    .as_ref()
                    .is_some_and(|t| t.action.is_none())
                {
                    self.active_toast = None;
                }
            } else if self
                .active_toast
                .as_ref()
                .map(|t| t.action.is_none())
                .unwrap_or(true)
            {
                let kind = if status_message_looks_like_error(&self.status_message) {
                    ToastKind::Error
                } else {
                    ToastKind::Info
                };
                let text = body.to_string();
                self.active_toast = Some(ActiveToast {
                    kind,
                    title: String::new(),
                    text,
                    until: Some(
                        Instant::now() + Duration::from_secs(Self::toast_duration_secs(kind)),
                    ),
                    action: None,
                    action_label: None,
                    secondary_action: None,
                    secondary_action_label: None,
                });
            }
        }
        if let Some(toast) = &self.active_toast {
            if let Some(until) = toast.until {
                if Instant::now() >= until {
                    self.clear_toast();
                }
            }
        }
    }

    pub(crate) fn show_status_toast(&mut self, ctx: &egui::Context, theme: &crate::ui::theme::Theme) {
        let Some(toast) = self.active_toast.clone() else {
            return;
        };
        let title = if toast.title.is_empty() {
            default_toast_title(ctx, toast.kind)
        } else {
            toast.title.clone()
        };
        let show_dismiss = toast.action.is_some()
            || toast.secondary_action.is_some()
            || matches!(toast.kind, ToastKind::Error | ToastKind::Warn);
        let mut primary = false;
        let mut secondary = false;
        let mut dismiss = false;
        let size = crate::ui::chrome::measure_status_toast_size(
            ctx,
            theme,
            &title,
            &toast.text,
            toast.action_label.as_deref(),
            toast.secondary_action_label.as_deref(),
            show_dismiss,
        );
        if size.x < 1.0 || size.y < 1.0 {
            return;
        }
        let margin = theme.toast_screen_margin();
        let screen = ctx.screen_rect();
        let pos = egui::pos2(
            screen.max.x - size.x - margin,
            screen.max.y - size.y - margin,
        );
        // 必须 fixed_pos + 限制尺寸：否则 Area 默认可点区会铺满从原点到右下，左侧菜单全部点不穿。
        egui::Area::new(egui::Id::new("status_toast"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .constrain_to(screen)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_max_size(size);
                ui.set_min_size(size);
                let actions = crate::ui::chrome::paint_status_toast(
                    ui,
                    theme,
                    &title,
                    &toast.text,
                    toast.kind,
                    toast.action_label.as_deref(),
                    toast.secondary_action_label.as_deref(),
                    show_dismiss,
                );
                primary = actions.primary;
                secondary = actions.secondary;
                dismiss = actions.dismiss;
            });

        if primary {
            if let Some(action) = toast.action {
                self.handle_toast_action(ctx, action);
            }
        } else if secondary {
            if let Some(action) = toast.secondary_action {
                self.handle_toast_action(ctx, action);
            }
        } else if dismiss {
            if toast.action == Some(ToastAction::OpenSshImport) {
                self.ssh_import_banner_dismissed = true;
                self.title_ssh_import_dismissed = true;
            }
            self.clear_toast();
        }
    }

    fn handle_toast_action(&mut self, ctx: &egui::Context, action: ToastAction) {
        match action {
            ToastAction::OpenSshImport => {
                self.clear_toast();
                self.open_ssh_import_dialog(ctx);
            }
            ToastAction::ReconnectTab { tab_idx } => {
                self.clear_toast();
                if tab_idx < self.tabs.len() {
                    self.active_tab = Some(tab_idx);
                    self.reconnect_tab_at(ctx, tab_idx);
                } else {
                    self.reconnect_active_tab(ctx);
                }
            }
            ToastAction::InsertSuggestedSnippet => {
                // 必须在 clear_toast 之前取出（clear 会丢掉 pending）
                let pending = self.pending_suggested_snippet.take();
                self.active_toast = None;
                self.status_message.clear();
                if let Some((tab_idx, fragment)) = pending {
                    if tab_idx < self.tabs.len() {
                        self.active_tab = Some(tab_idx);
                    }
                    self.begin_fragment_insert(ctx, &fragment);
                }
            }
            ToastAction::SaveSuggestedSnippet => {
                let pending = self.pending_suggested_snippet.clone();
                if let Some((_tab_idx, fragment)) = pending {
                    let (is_new, title) = self.save_suggested_snippet_to_personal(&fragment);
                    if let Some(t) = &mut self.active_toast {
                        t.secondary_action = None;
                        t.secondary_action_label = None;
                        let mark = if is_new {
                            crate::i18n::tr(ctx, "saved to library", "已存到个人库")
                        } else {
                            crate::i18n::tr(ctx, "already in library", "个人库已有")
                        };
                        if !t.text.contains(mark) {
                            t.text = format!("{} · {mark}「{title}」", t.text);
                            self.status_message =
                                status_message_wrap_error(status_payload(&t.title, &t.text));
                        }
                    }
                }
            }
        }
    }

    /// SSH 连接失败 / 自动重连放弃：带「重连」主按钮的 Error Toast。
    pub(crate) fn notify_ssh_reconnect_error(
        &mut self,
        ctx: &egui::Context,
        tab_idx: usize,
        text: impl Into<String>,
    ) {
        let action_label = crate::i18n::tr(ctx, "Reconnect", "重连").to_string();
        self.push_action_toast_titled(
            ToastKind::Error,
            crate::i18n::tr(ctx, "Connection failed", "连接失败"),
            text,
            ToastAction::ReconnectTab { tab_idx },
            action_label,
        );
    }
}
