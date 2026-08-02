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
}

#[derive(Debug, Clone)]
pub(super) struct ActiveToast {
    kind: ToastKind,
    text: String,
    /// `None` = 需用户确认，不自动消失。
    until: Option<Instant>,
    action: Option<ToastAction>,
    action_label: Option<String>,
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
        let raw = text.into();
        let text = status_message_body(&raw).trim().to_string();
        if text.is_empty() {
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
        self.status_message = match kind {
            ToastKind::Error => status_message_wrap_error(text.clone()),
            _ => text.clone(),
        };
        self.active_toast = Some(ActiveToast {
            kind,
            text,
            until: Some(Instant::now() + Duration::from_secs(Self::toast_duration_secs(kind))),
            action: None,
            action_label: None,
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
        let raw = text.into();
        let text = status_message_body(&raw).trim().to_string();
        if text.is_empty() {
            return;
        }
        let action_label = action_label.into();
        if let Some(toast) = &self.active_toast {
            if toast.action == Some(action)
                && toast.until.is_none()
                && toast.text == text
                && toast.action_label.as_deref() == Some(action_label.as_str())
            {
                return;
            }
        }
        self.status_message = match kind {
            ToastKind::Error => status_message_wrap_error(text.clone()),
            _ => text.clone(),
        };
        self.active_toast = Some(ActiveToast {
            kind,
            text,
            until: None,
            action: Some(action),
            action_label: Some(action_label),
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
        let text = match crate::i18n::language(ctx) {
            crate::i18n::UiLanguage::En => {
                format!("Detected {pending} pending SSH Host block(s)")
            }
            crate::i18n::UiLanguage::Zh => format!("检测到 {pending} 个未导入的 SSH 配置"),
        };
        let action_label = crate::i18n::tr(ctx, "Import", "导入").to_string();
        self.push_action_toast(
            ToastKind::Warn,
            text,
            ToastAction::OpenSshImport,
            action_label,
        );
    }

    /// 刷新 Toast 过期；并桥接仍直接写 `status_message` 的旧路径。
    pub(crate) fn tick_status_toast(&mut self) {
        let toast_text = self
            .active_toast
            .as_ref()
            .map(|t| t.text.as_str())
            .unwrap_or("");
        let body = status_message_body(&self.status_message);
        if body != toast_text {
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
                    text,
                    until: Some(
                        Instant::now() + Duration::from_secs(Self::toast_duration_secs(kind)),
                    ),
                    action: None,
                    action_label: None,
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
        let show_dismiss = toast.action.is_some()
            || matches!(toast.kind, ToastKind::Error | ToastKind::Warn);
        let mut primary = false;
        let mut dismiss = false;
        egui::Area::new(egui::Id::new("status_toast"))
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                let actions = crate::ui::chrome::paint_status_toast(
                    ui,
                    theme,
                    &toast.text,
                    toast.kind,
                    toast.action_label.as_deref(),
                    show_dismiss,
                );
                primary = actions.primary;
                dismiss = actions.dismiss;
            });

        if primary {
            if let Some(action) = toast.action {
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
                }
            }
        } else if dismiss {
            if toast.action == Some(ToastAction::OpenSshImport) {
                self.ssh_import_banner_dismissed = true;
                self.title_ssh_import_dismissed = true;
            }
            self.clear_toast();
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
        self.push_action_toast(
            ToastKind::Error,
            text,
            ToastAction::ReconnectTab { tab_idx },
            action_label,
        );
    }
}
