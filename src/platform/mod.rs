//! 平台相关能力（字体、快捷键文案、系统 shell、路径、macOS 菜单等）

pub mod app_name;
pub mod docs;
pub mod fonts;
pub mod logging;
pub mod paths;
pub mod shell;
pub mod shortcuts;
#[cfg(target_os = "macos")]
mod macos_launch;
#[cfg(target_os = "macos")]
mod macos_app_name;
#[cfg(target_os = "macos")]
mod macos_ime;

#[cfg(target_os = "macos")]
pub mod macos_menu;

pub use app_name::APP_DISPLAY_NAME;
pub use docs::{github_feature_request_url, github_new_issue_url, DOCS_INDEX_URL, GITHUB_ISSUES_URL};
pub use fonts::{
    cjk_font_loaded, clamp_terminal_font_size, configure_egui_fonts, TerminalFontPreset,
    DEFAULT_TERMINAL_FONT_SIZE, TERMINAL_FONT_SIZE_MAX, TERMINAL_FONT_SIZE_MIN,
};
pub use logging::init_runtime_logging;
pub use paths::{default_ssh_config_path, home_dir, home_dir_display_hint};
pub use shell::{open_file, open_url, reveal_directory};
pub use shortcuts::{
    accel, accel_enter, accel_literal, accel_shift, close_tab_accel, close_tab_help_line, help_line,
    new_tab_accel, new_tab_help_line, primary_modifier_label, split_pane_focus_accel,
    split_pane_focus_help_line, terminal_history_accel, terminal_interrupt_accel,
};
#[cfg(target_os = "macos")]
pub use macos_launch::activate_gui_application;
#[cfg(target_os = "macos")]
pub use macos_app_name::{fix_menu_bar_application_title, set_application_display_name};

/// 启动时尽量切到英文键盘布局（macOS：`com.apple.keylayout.ABC`）；其它平台为空操作。
#[cfg(target_os = "macos")]
pub fn apply_preferred_english_input_source() {
    macos_ime::select_abc_keyboard_layout();
}

#[cfg(not(target_os = "macos"))]
pub fn apply_preferred_english_input_source() {}
