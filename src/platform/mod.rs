//! 平台相关能力（字体、快捷键文案、系统 shell、路径、macOS 菜单等）

pub mod app_name;
pub mod docs;
mod font_enum;
pub mod fonts;
pub mod logging;
pub mod paths;
pub mod quit_watchdog;
pub mod shell;
pub mod shortcuts;
pub mod url_handler;
pub mod version;
#[cfg(target_os = "macos")]
mod macos_launch;
#[cfg(target_os = "macos")]
mod macos_app_name;
#[cfg(target_os = "macos")]
mod macos_ime;

#[cfg(target_os = "macos")]
pub mod macos_menu;

// eframe 0.23 不重新导出该 trait。编译器对 `with_option_as_alt` 的建议导入是这一条；
// `winit` 必须是 macOS 的直接依赖，否则 E0433，trait 也不在作用域（E0599）。
#[cfg(target_os = "macos")]
use winit::platform::macos::{OptionAsAlt, WindowBuilderExtMacOS};

pub use app_name::APP_DISPLAY_NAME;
pub use docs::{
    github_feature_request_url, github_new_issue_url, github_new_issue_url_with_body, DOCS_INDEX_URL,
    GITHUB_ISSUES_URL,
};
pub use fonts::{
    cjk_font_loaded, clamp_terminal_font_size, configure_egui_fonts, configure_terminal_font,
    font_family_eq, list_system_mono_fonts, start_system_mono_font_scan,
    system_mono_fonts_if_ready, take_deferred_terminal_font_apply, terminal_font_apply_deferred,
    terminal_preset_family_name, terminal_preset_installed, ui_caption_font_id, ui_font_id,
    SystemMonoFont, TerminalFontPreset, TerminalFontSelection, CJK_UI_FONT_NAME,
    DEFAULT_TERMINAL_FONT_SIZE, TERMINAL_FONT_SIZE_MAX, TERMINAL_FONT_SIZE_MIN,
};
pub use logging::init_runtime_logging;
pub use paths::{default_ssh_config_path, home_dir, home_dir_display_hint};
pub use quit_watchdog::arm_quit_watchdog;
pub use shell::{open_file, open_url, reveal_directory};
pub use version::{print_version_line, APP_VERSION};
pub use shortcuts::{
    accel, accel_enter, accel_literal, accel_shift, close_tab_accel, close_tab_help_line, help_line,
    local_shell_accel, local_shell_help_line, new_tab_accel, new_tab_help_line,
    primary_modifier_label, split_pane_focus_accel, split_pane_focus_help_line,
    terminal_copy_accel, terminal_history_accel, terminal_interrupt_accel, terminal_paste_accel,
    paste_history_accel,
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

/// 启动 MistTerm GUI 界面
pub fn run_gui() {
    log::info!("MistTerm GUI starting");

    #[cfg(target_os = "macos")]
    {
        set_application_display_name();
        activate_gui_application();
    }

    apply_preferred_english_input_source();

    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        maximized: false,
        initial_window_size: Some(eframe::egui::vec2(1200.0, 820.0)),
        max_window_size: None,
        app_id: Some("mistterm".to_string()),
        icon_data: Some(crate::ui::icons::app_window_icon_data()),
        ..Default::default()
    };

    // macOS：内容延伸到标题栏，标签和红绿灯同一行(透明标题栏)。
    // Option 当作 Alt：键位字符进事件队列并带上 Alt，终端再编成 Meta（ESC 前缀）。
    // 不改成可配置的键位表。
    #[cfg(target_os = "macos")]
    {
        options.fullsize_content = true;
        options.window_builder = Some(Box::new(|builder| {
            builder.with_option_as_alt(OptionAsAlt::Both)
        }));
    }

    if let Err(e) = eframe::run_native(
        APP_DISPLAY_NAME,
        options,
        Box::new(|cc| {
            if !configure_egui_fonts(
                &cc.egui_ctx,
                TerminalFontPreset::default(),
            ) {
                log::warn!("CJK font not loaded; Chinese UI text may show as tofu");
            }
            crate::ui::icons::UiIcons::install(&cc.egui_ctx);
            Box::new(crate::ui::MistTermApp::new(cc))
        }),
    ) {
        eprintln!("Failed to launch GUI: {e}");
        std::process::exit(1);
    }
}
