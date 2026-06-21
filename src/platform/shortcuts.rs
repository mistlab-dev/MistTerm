//! 跨平台快捷键展示与检测（macOS ⌘，Windows/Linux Ctrl）。
//!
//! 逻辑快捷键在 UI 层用 `egui::Modifiers::command || ctrl`；本模块只统一**用户可见文案**。
//!
//! **终端聚焦时仍生效的「标签/分屏」快捷键**不得占用 readline 常用 Ctrl 单键（A/E/K/N/R/T/U/W 等）；
//! Win/Linux 侧与 Windows Terminal 一致，优先使用 Ctrl+Shift+*；macOS 侧用 ⌘（shell 仍用 Ctrl）。

/// 主修饰键在 UI 中的短标签：`⌘` 或 `Ctrl`。
pub fn primary_modifier_label() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "⌘"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "Ctrl"
    }
}

fn display_key(key: &str) -> String {
    if key.len() == 1 {
        key.to_ascii_lowercase()
    } else {
        key.to_string()
    }
}

/// 如 `⌘ + n` / `Ctrl + n`。
pub fn accel(key: &str) -> String {
    format!("{} + {}", primary_modifier_label(), display_key(key))
}

/// 主修饰键 + 字面后缀（如 `1–9`、`Tab`、`,`）。
pub fn accel_literal(suffix: &str) -> String {
    format!("{} + {}", primary_modifier_label(), suffix)
}

/// 如 `⌘ + Shift + j` / `Ctrl + Shift + j`。
pub fn accel_shift(key: &str) -> String {
    format!(
        "{} + Shift + {}",
        primary_modifier_label(),
        display_key(key)
    )
}

/// 终端命令历史：各平台均为 Ctrl + R（与 shell 习惯一致，不用 ⌘）。
pub fn terminal_history_accel() -> &'static str {
    "Ctrl + R"
}

/// 主修饰键 + Enter（如 AI 输入框发送）。
pub fn accel_enter() -> String {
    format!("{} + Enter", primary_modifier_label())
}

/// 终端中断（Ctrl + C，各平台一致）。
pub fn terminal_interrupt_accel() -> &'static str {
    "Ctrl + C"
}

/// 帮助/关于中的「主修饰键 + 单键」说明行。
pub fn help_line(key: &str, description: &str) -> String {
    format!("{} — {}", accel(key), description)
}

/// 关闭当前终端标签：macOS 用 ⌘W；Win/Linux 用 Ctrl+Shift+W（避免与 shell 的 Ctrl+W 删词冲突）。
pub fn close_tab_accel() -> String {
    #[cfg(target_os = "macos")]
    {
        accel("w")
    }
    #[cfg(not(target_os = "macos"))]
    {
        accel_shift("w")
    }
}

pub fn close_tab_help_line(description: &str) -> String {
    format!("{} — {}", close_tab_accel(), description)
}

/// 新建终端标签：macOS ⌘T；Win/Linux Ctrl+Shift+T（Ctrl+T 留给 shell transpose-chars）。
pub fn new_tab_accel() -> String {
    #[cfg(target_os = "macos")]
    {
        accel("t")
    }
    #[cfg(not(target_os = "macos"))]
    {
        accel_shift("t")
    }
}

pub fn new_tab_help_line(description: &str) -> String {
    format!("{} — {}", new_tab_accel(), description)
}

/// 分屏窗格切换焦点：macOS ⌘⌥←/→；Win/Linux Ctrl+Shift+←/→（Alt+←/→ 留给 shell 按词移动）。
pub fn split_pane_focus_accel() -> String {
    #[cfg(target_os = "macos")]
    {
        format!("{} + Option + ←/→", primary_modifier_label())
    }
    #[cfg(not(target_os = "macos"))]
    {
        format!("{} + Shift + ←/→", primary_modifier_label())
    }
}

pub fn split_pane_focus_help_line(description: &str) -> String {
    format!("{} — {}", split_pane_focus_accel(), description)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tab_accel_uses_shift_on_non_mac() {
        #[cfg(not(target_os = "macos"))]
        assert!(new_tab_accel().contains("Shift"));
        #[cfg(target_os = "macos")]
        assert!(!new_tab_accel().contains("Shift"));
    }

    #[test]
    fn close_tab_accel_uses_shift_on_non_mac() {
        #[cfg(not(target_os = "macos"))]
        assert!(close_tab_accel().contains("Shift"));
    }
}
