//! 终端 shell 启发式着色与本机状态行 ANSI（FUNCTIONAL_SPEC §2.3.2），配色随 [`Theme`] 派生。

use crate::ui::theme::Theme;
use egui::Color32;

/// FUNCTIONAL_SPEC §2.3.2：提示行命令段 / 输出行相对默认前景的亮度系数。
pub const TERMINAL_COMMAND_DIM_FACTOR: f32 = 0.9;
pub const TERMINAL_OUTPUT_DIM_FACTOR: f32 = 0.4;

/// 块状光标闪烁周期（秒），与常见终端 ~530ms 一致。
pub const TERMINAL_CURSOR_BLINK_PERIOD_SECS: f64 = 0.53;

/// 终端 ScrollArea 纵向条（§2.3.4：宽 4px、轨道约 `rgba(255,255,255,0.06)`）。
pub const TERMINAL_SCROLL_BAR_WIDTH: f32 = 4.0;
/// 255 * 0.06 ≈ 15，与 `Theme::fg_high_a15` 一致。
pub const TERMINAL_SCROLL_BAR_TRACK_ALPHA: u8 = 15;

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
            default_fg: theme.text_primary(),
            terminal_bg: theme.bg_terminal_color(),
            prompt_arrow: theme.green_color(),
            path_hint: theme.accent_color(),
            user_error: theme.red_color(),
            user_info: if theme.is_light_theme() {
                theme.accent_color()
            } else {
                theme.text_primary()
            },
            user_success: theme.green_color(),
            user_warn: theme.amber_color(),
            command_dim_factor: theme.terminal_command_dim_factor(),
            output_dim_factor: theme.terminal_output_dim_factor(),
            search_match_fg: theme.text_primary(),
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
