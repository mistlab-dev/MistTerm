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

/// 如 `⌘N` / `Ctrl+N`（不含 `+`，与现有菜单排版一致）。
pub fn accel(key: &str) -> String {
    format!("{}{}", primary_modifier_label(), key)
}

/// 如 `⌘⇧J` / `Ctrl+Shift+J`。
pub fn accel_shift(key: &str) -> String {
    #[cfg(target_os = "macos")]
    {
        format!("⌘⇧{}", key)
    }
    #[cfg(not(target_os = "macos"))]
    {
        format!("Ctrl+Shift+{}", key)
    }
}

/// 终端命令历史：各平台均为 Ctrl+R（与 shell 习惯一致，不用 ⌘）。
pub fn terminal_history_accel() -> &'static str {
    "Ctrl+R"
}

/// 帮助/关于中的「主修饰键 + 单键」说明行。
pub fn help_line(key: &str, description: &str) -> String {
    format!("{} — {}", accel(key), description)
}
