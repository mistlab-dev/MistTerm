//! 跨平台快捷键展示与检测（macOS ⌘，Windows/Linux Ctrl）。
//!
//! 逻辑快捷键在 UI 层用 `egui::Modifiers::command || ctrl`；本模块只统一**用户可见文案**。

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
