//! 平台相关能力（字体、快捷键文案、系统 shell、路径、macOS 菜单等）

pub mod docs;
pub mod fonts;
pub mod paths;
pub mod shell;
pub mod shortcuts;
#[cfg(target_os = "macos")]
mod macos_app_name;
#[cfg(target_os = "macos")]
mod macos_ime;

#[cfg(target_os = "macos")]
pub mod macos_menu;

pub use docs::{
    docs_directory, reveal_docs_directory, reveal_docs_folder_menu_action_label_pair,
    reveal_docs_folder_menu_hint_en, reveal_docs_folder_menu_hint_zh,
    reveal_docs_folder_success_pair,
};
pub use fonts::{cjk_font_loaded, configure_egui_fonts};
pub use paths::{default_ssh_config_path, home_dir};
pub use shell::{open_file, reveal_directory};
pub use shortcuts::{accel, accel_shift, help_line, primary_modifier_label, terminal_history_accel};
#[cfg(target_os = "macos")]
pub use macos_app_name::{
    fix_menu_bar_application_title, set_application_display_name, APP_DISPLAY_NAME,
};

/// 启动时尽量切到英文键盘布局（macOS：`com.apple.keylayout.ABC`）；其它平台为空操作。
#[cfg(target_os = "macos")]
pub fn apply_preferred_english_input_source() {
    macos_ime::select_abc_keyboard_layout();
}

#[cfg(not(target_os = "macos"))]
pub fn apply_preferred_english_input_source() {}
