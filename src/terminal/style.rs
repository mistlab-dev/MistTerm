//! 终端 shell 启发式着色与本机状态行 ANSI（FUNCTIONAL_SPEC §2.3.2），配色随 [`Theme`] 派生。

use crate::ui::theme::Theme;
use egui::Color32;

/// FUNCTIONAL_SPEC §2.3.2：提示行命令段 / 输出行相对默认前景的亮度系数。
/// 与 Windows Terminal / PowerShell 对齐：默认不做压暗（1.0）。
pub const TERMINAL_COMMAND_DIM_FACTOR: f32 = 1.0;
pub const TERMINAL_OUTPUT_DIM_FACTOR: f32 = 1.0;

/// 已连接时 UI 刷新间隔（毫秒）。
/// 业界常见：有输出时约 16–33ms（60–30 FPS）；过低会空转 CPU，过高则 `top`/vim 发钝。
pub const TERMINAL_LIVE_REPAINT_MS: u64 = 33;

/// 块状光标闪烁周期（秒），与 xterm 默认约 530ms 一致。
pub const TERMINAL_CURSOR_BLINK_PERIOD_SECS: f64 = 0.53;

/// 终端 ScrollArea 纵向条（§2.3.4：宽 4px、轨道约 `rgba(255,255,255,0.06)`）。
pub const TERMINAL_SCROLL_BAR_WIDTH: f32 = 4.0;
/// 255 * 0.06 ≈ 15，与 `Theme::fg_high_a15` 一致。
pub const TERMINAL_SCROLL_BAR_TRACK_ALPHA: u8 = 15;

/// 终端行高相对字号的上限系数。
/// egui `row_height` = ascent - descent + line_gap；Consolas 等 line_gap 偏大时会出现「字贴顶、下空一截」。
/// 夹到 `font_size * FACTOR` 与 Windows Terminal 常见 1.0–1.2 行距接近。
pub const TERMINAL_LINE_HEIGHT_FACTOR: f32 = 1.1;

/// 由当前主题派生的终端 shell 着色参数（供 [`crate::terminal::Terminal::get_layout_job`] 与 UI feed 共用）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerminalShellStyle {
    pub default_fg: Color32,
    pub terminal_bg: Color32,
    pub prompt_arrow: Color32,
    pub path_hint: Color32,
    pub user_error: Color32,
    pub user_info: Color32,
    pub user_success: Color32,
    pub user_warn: Color32,
    pub command_dim_factor: f32,
    pub output_dim_factor: f32,
    /// 查找命中高亮（由 [`Theme::list_row_selected_bg`] 等派生）
    pub search_match_fg: Color32,
    pub search_match_bg: Color32,
}

/// 去掉行内空白，便于匹配被 VTE 拉开的 CJK 状态文案。
pub fn line_compact(line: &str) -> String {
    line.chars().filter(|c| !c.is_whitespace()).collect()
}

impl TerminalShellStyle {
    pub fn from_theme(theme: &Theme) -> Self {
        Self {
            default_fg: theme.terminal_default_fg(),
            terminal_bg: theme.bg_terminal_color(),
            prompt_arrow: theme.green_color(),
            path_hint: theme.accent_color(),
            user_error: theme.red_color(),
            user_info: if theme.is_light_theme() {
                theme.accent_color()
            } else {
                theme.terminal_default_fg()
            },
            user_success: theme.green_color(),
            user_warn: theme.amber_color(),
            command_dim_factor: theme.terminal_command_dim_factor(),
            output_dim_factor: theme.terminal_output_dim_factor(),
            search_match_fg: theme.terminal_default_fg(),
            search_match_bg: theme.list_row_selected_bg(),
        }
    }
}

/// 粗体 truecolor SGR（`38;2;r;g;b`），供本机写入 PTY 的 feed 行使用。
pub fn truecolor_sgr_bold(color: Color32) -> String {
    format!("1;38;2;{};{};{}", color.r(), color.g(), color.b())
}

fn feed_ansi_line(sgr: &str, body: &str) -> String {
    format!("\r\n\x1b[{sgr}m{body}\x1b[0m\r\n")
}

pub fn format_user_error_line(theme: &Theme, message: &str) -> String {
    let s = TerminalShellStyle::from_theme(theme);
    feed_ansi_line(
        &truecolor_sgr_bold(s.user_error),
        &format!("错误：{message}"),
    )
}

pub fn format_user_info_line(theme: &Theme, message: &str) -> String {
    let s = TerminalShellStyle::from_theme(theme);
    feed_ansi_line(&truecolor_sgr_bold(s.user_info), message)
}

pub fn format_user_success_line(theme: &Theme, message: &str) -> String {
    let s = TerminalShellStyle::from_theme(theme);
    feed_ansi_line(&truecolor_sgr_bold(s.user_success), message)
}

pub fn format_user_warn_line(theme: &Theme, message: &str) -> String {
    let s = TerminalShellStyle::from_theme(theme);
    feed_ansi_line(&truecolor_sgr_bold(s.user_warn), message)
}

pub fn is_user_error_line(line: &str) -> bool {
    line.starts_with("Error:")
        || line.starts_with("错误")
        || line.contains("连接失败")
        || line.contains("认证失败")
        || line.contains("传输失败")
        || line.starts_with('❌')
}

pub fn is_user_info_line(line: &str) -> bool {
    let compact = line_compact(line);
    line.starts_with("Connecting")
        || line.contains("正在连接")
        || compact.contains("正在连接")
        || line.starts_with("Connected")
        || compact.contains("Connecting")
}

pub fn is_user_success_line(line: &str) -> bool {
    let compact = line_compact(line);
    line.starts_with("✅")
        || line.starts_with("已连接")
        || compact.starts_with("已连接")
}

pub fn is_user_warn_line(line: &str) -> bool {
    line.starts_with("Disconnected")
        || line.contains("连接已断开")
        || line.contains("已断开 SSH")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------- line_compact

    #[test]
    fn line_compact_strips_all_ascii_whitespace() {
        assert_eq!(line_compact("a b\tc\nd"), "abcd");
    }

    #[test]
    fn line_compact_strips_unicode_whitespace() {
        assert_eq!(line_compact("　a\u{2003}b "), "ab");
    }

    #[test]
    fn line_compact_empty_input() {
        assert_eq!(line_compact(""), "");
    }

    #[test]
    fn line_compact_preserves_cjk_and_symbols() {
        assert_eq!(line_compact("正在   连 接..."), "正在连接...");
    }

    #[test]
    fn line_compact_only_whitespace_returns_empty() {
        assert_eq!(line_compact("  \t\n  \r\n"), "");
    }

    // --------------------------------------------------------------- truecolor_sgr_bold

    #[test]
    fn truecolor_sgr_bold_formats_rgb() {
        let c = egui::Color32::from_rgb(0xAB, 0xCD, 0xEF);
        assert_eq!(truecolor_sgr_bold(c), "1;38;2;171;205;239");
    }

    #[test]
    fn truecolor_sgr_bold_black() {
        assert_eq!(
            truecolor_sgr_bold(egui::Color32::BLACK),
            "1;38;2;0;0;0"
        );
    }

    #[test]
    fn truecolor_sgr_bold_white() {
        assert_eq!(
            truecolor_sgr_bold(egui::Color32::WHITE),
            "1;38;2;255;255;255"
        );
    }

    // -------------------------------------------------------------- is_user_*_line

    #[test]
    fn is_user_error_line_detected_patterns() {
        assert!(is_user_error_line("Error: something wrong"));
        assert!(is_user_error_line("错误：权限不足"));
        assert!(is_user_error_line("SSH 连接失败，请检查地址"));
        assert!(is_user_error_line("认证失败 (publickey)"));
        assert!(is_user_error_line("文件传输失败：timeout"));
        assert!(is_user_error_line("❌ 主机不可达"));
    }

    #[test]
    fn is_user_error_line_rejects_normal_lines() {
        assert!(!is_user_error_line("root@host:~$ ls -la"));
        assert!(!is_user_error_line("Connected to server"));
        assert!(!is_user_error_line(""));
    }

    #[test]
    fn is_user_info_line_detected_patterns() {
        assert!(is_user_info_line("Connecting to 1.2.3.4:22 ..."));
        assert!(is_user_info_line("正在连接 server.example"));
        assert!(is_user_info_line("  正在 连 接  远程主机  "));
        assert!(is_user_info_line("Connected successfully"));
        assert!(is_user_info_line("Connecting via ProxyCommand"));
    }

    #[test]
    fn is_user_info_line_rejects_normal_lines() {
        assert!(!is_user_info_line("root@host:~$ echo hello"));
        assert!(!is_user_info_line("Disconnected"));
        assert!(!is_user_info_line(""));
    }

    #[test]
    fn is_user_success_line_detected_patterns() {
        assert!(is_user_success_line("✅ 登录成功"));
        assert!(is_user_success_line("已连接到 10.0.0.1"));
        assert!(is_user_success_line(" 已 连 接  (耗时 123ms)"));
    }

    #[test]
    fn is_user_success_line_rejects_normal_lines() {
        assert!(!is_user_success_line("Connecting to host"));
        assert!(!is_user_success_line(""));
    }

    #[test]
    fn is_user_warn_line_detected_patterns() {
        assert!(is_user_warn_line("Disconnected by remote host"));
        assert!(is_user_warn_line("连接已断开 (timeout=30s)"));
        assert!(is_user_warn_line("已断开 SSH 会话"));
    }

    #[test]
    fn is_user_warn_line_rejects_normal_lines() {
        assert!(!is_user_warn_line("Connected to host"));
        assert!(!is_user_warn_line("root@host:~$ exit"));
        assert!(!is_user_warn_line(""));
    }
}
