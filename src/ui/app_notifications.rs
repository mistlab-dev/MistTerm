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

#[derive(Debug, Clone)]
pub(super) struct ActiveToast {
    kind: ToastKind,
    text: String,
    until: Instant,
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

/// 底栏 / 提示文案颜色：错误类用主题红，其余用弱文字色（避免顶栏大块告警色）
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
    /// 统一通知入口：所有用户可见提示走 Toast（并同步 `status_message` 供诊断快照）。
    pub(crate) fn push_toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        let raw = text.into();
        let text = status_message_body(&raw).trim().to_string();
        if text.is_empty() {
            self.active_toast = None;
            self.status_message.clear();
            return;
        }
        let secs = match kind {
            ToastKind::Error => 6,
            ToastKind::Warn => 5,
            ToastKind::Success | ToastKind::Info => 4,
        };
        self.status_message = match kind {
            ToastKind::Error => status_message_wrap_error(text.clone()),
            _ => text.clone(),
        };
        self.active_toast = Some(ActiveToast {
            kind,
            text,
            until: Instant::now() + Duration::from_secs(secs),
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
                self.active_toast = None;
            } else {
                let kind = if status_message_looks_like_error(&self.status_message) {
                    ToastKind::Error
                } else {
                    ToastKind::Info
                };
                let text = body.to_string();
                let secs = match kind {
                    ToastKind::Error => 6,
                    ToastKind::Warn => 5,
                    ToastKind::Success | ToastKind::Info => 4,
                };
                self.active_toast = Some(ActiveToast {
                    kind,
                    text,
                    until: Instant::now() + Duration::from_secs(secs),
                });
            }
        }
        if let Some(toast) = &self.active_toast {
            if Instant::now() >= toast.until {
                self.clear_toast();
            }
        }
    }

    pub(crate) fn show_status_toast(&self, ctx: &egui::Context, theme: &crate::ui::theme::Theme) {
        let Some(toast) = &self.active_toast else {
            return;
        };
        egui::Area::new(egui::Id::new("status_toast"))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                crate::ui::chrome::paint_status_toast(ui, theme, &toast.text, toast.kind);
            });
    }
}
