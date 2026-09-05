use eframe::egui;

/// FUNCTIONAL_SPEC §7 快捷键单一真源(关于页与帮助共用；随平台显示 ⌘ 或 Ctrl)。
pub(crate) fn mistterm_functional_spec_shortcuts(ctx: &egui::Context) -> String {
    use crate::i18n::UiLanguage;
    use crate::platform::shortcuts as s;

    fn mac_extra_en() -> String {
        #[cfg(target_os = "macos")]
        {
            "\n⌘ + q — Quit app\n⌘ + h — Hide app\n⌘ + m — Minimize window".to_string()
        }
        #[cfg(not(target_os = "macos"))]
        {
            String::new()
        }
    }

    fn mac_extra_zh() -> String {
        #[cfg(target_os = "macos")]
        {
            "\n⌘ + q — 退出应用\n⌘ + h — 隐藏应用\n⌘ + m — 最小化窗口".to_string()
        }
        #[cfg(not(target_os = "macos"))]
        {
            String::new()
        }
    }

    fn en() -> String {
        format!(
            "Keyboard shortcuts (primary: {})\n\
             {}\n\
             {}\n\
             {}\n\
             {}\n\
             {} — switch to tab N\n\
             {} — next tab (Shift reverses)\n\
             {}\n\
             {}\n\
             {}\n\
             {}\n\
             {} — search in terminal viewport\n\
             {} — Preferences\n\
             {} — About & this cheatsheet\n\
             {} — command history (in terminal)\n\
             {} — AI assistant panel\n\
             {} — send terminal selection to AI{}",
            s::primary_modifier_label(),
            s::help_line("N", "New session"),
            s::help_line("E", "Edit selected session"),
            s::new_tab_help_line("New terminal tab"),
            s::close_tab_help_line("Close current tab"),
            s::accel_literal("1–9"),
            s::accel_literal("Tab"),
            s::help_line("J", "Focus connection search"),
            s::help_line("K", "Focus snippet search"),
            s::help_line("B", "Show/hide activity rail"),
            format!("{} — Quick snippet picker", s::accel_shift("J")),
            s::accel("F"),
            s::accel_literal(","),
            s::accel("H"),
            s::terminal_history_accel(),
            s::accel_shift("A"),
            s::accel_shift("L"),
            mac_extra_en(),
        )
    }

    fn zh() -> String {
        format!(
            "键盘快捷键(主修饰键：{})\n\
             {}\n\
             {}\n\
             {}\n\
             {}\n\
             {} — 切换第 N 个标签\n\
             {} — 下一标签；加 Shift 为上一标签\n\
             {}\n\
             {}\n\
             {}\n\
             {}\n\
             {} — 终端内搜索\n\
             {} — 偏好设置\n\
             {} — 关于与本说明\n\
             {} — 命令历史(终端内)\n\
             {} — AI 助手面板\n\
             {} — 终端选区发送到 AI{}",
            s::primary_modifier_label(),
            s::help_line("N", "新建会话"),
            s::help_line("E", "编辑所选会话"),
            s::new_tab_help_line("新终端标签"),
            s::close_tab_help_line("关闭当前标签"),
            s::accel_literal("1–9"),
            s::accel_literal("Tab"),
            s::help_line("J", "聚焦连接搜索"),
            s::help_line("K", "聚焦片段搜索"),
            s::help_line("B", "显示/隐藏活动栏"),
            s::accel_shift("J").to_owned() + " — 快速片段选择器",
            s::accel("F"),
            s::accel_literal(","),
            s::accel("H"),
            s::terminal_history_accel(),
            s::accel_shift("A"),
            s::accel_shift("L"),
            mac_extra_zh(),
        )
    }

    match crate::i18n::language(ctx) {
        UiLanguage::En => en(),
        UiLanguage::Zh => zh(),
    }
}
