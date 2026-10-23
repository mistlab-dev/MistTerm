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
             {} — shell reverse-i-search when the terminal is focused (not the command history panel); when unfocused, Mist command history. Also Tools → Command History\n\
             {} — interrupt the foreground program (SIGINT), even if text is selected\n\
             {} — copy the terminal selection\n\
             {} — paste into the terminal\n\
             {} — Select all (terminal)\n\
             {} — AI assistant panel\n\
             {} — send terminal selection to AI{}",
            s::primary_modifier_label(),
            s::help_line("N", "New connection"),
            s::help_line("E", "Edit selected session"),
            s::new_tab_help_line("New terminal tab"),
            s::local_shell_help_line("Local shell tab"),
            s::close_tab_help_line("Close current tab"),
            s::accel_literal("1–9"),
            s::accel_literal("Tab"),
            s::help_line(
                "J",
                "Focus connection search (terminal not focused; focused Ctrl+J goes to the shell)",
            ),
            s::help_line(
                "K",
                "Focus snippet search (terminal not focused; focused Ctrl+K goes to the shell)",
            ),
            s::help_line("B", "Immersive mode (hide rail & menus, tabs in title bar)"),
            format!(
                "{} — Quick snippet picker (terminal not focused)",
                s::accel_shift("J")
            ),
            s::accel("F"),
            s::accel_literal(","),
            s::accel("H"),
            s::terminal_history_accel(),
            s::terminal_interrupt_accel(),
            s::terminal_copy_accel(),
            s::terminal_paste_accel(),
            s::accel_shift("A"),
            s::accel_shift("I"),
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
             {} — 终端聚焦时交给 shell 反向搜索 (不是命令历史面板); 失焦时打开 Mist 命令历史。面板也在「工具 → 命令历史」\n\
             {} — 中断前台程序 (SIGINT), 有选区也一样\n\
             {} — 复制终端选区\n\
             {} — 粘贴到终端\n\
             {} — 全选(终端)\n\
             {} — AI 助手面板\n\
             {} — 终端选区发送到 AI{}",
            s::primary_modifier_label(),
            s::help_line("N", "新建连接"),
            s::help_line("E", "编辑所选会话"),
            s::new_tab_help_line("新终端标签"),
            s::local_shell_help_line("本机终端标签"),
            s::close_tab_help_line("关闭当前标签"),
            s::accel_literal("1–9"),
            s::accel_literal("Tab"),
            s::help_line("J", "聚焦连接搜索 (终端失焦时; 聚焦时 Ctrl+J 发给 shell)",),
            s::help_line("K", "聚焦片段搜索 (终端失焦时; 聚焦时 Ctrl+K 发给 shell)",),
            s::help_line("B", "沉浸式模式 (藏活动栏与菜单, 标签进顶栏)"),
            s::accel_shift("J").to_owned() + " — 快速片段选择器 (终端失焦时)",
            s::accel("F"),
            s::accel_literal(","),
            s::accel("H"),
            s::terminal_history_accel(),
            s::terminal_interrupt_accel(),
            s::terminal_copy_accel(),
            s::terminal_paste_accel(),
            s::accel_shift("A"),
            s::accel_shift("I"),
            s::accel_shift("L"),
            mac_extra_zh(),
        )
    }

    match crate::i18n::language(ctx) {
        UiLanguage::En => en(),
        UiLanguage::Zh => zh(),
    }
}
