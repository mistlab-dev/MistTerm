//! 平台相关能力（输入法、窗口、系统菜单等）

#[cfg(target_os = "macos")]
mod macos_app_name;
#[cfg(target_os = "macos")]
mod macos_ime;

#[cfg(target_os = "macos")]
pub mod macos_menu;

pub mod docs;

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
