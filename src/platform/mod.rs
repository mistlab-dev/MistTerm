//! 平台相关能力（字体、快捷键文案、系统 shell、路径、macOS 菜单等）

pub mod app_name;
pub mod desktop_notify;
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

// eframe 0.37 重新导出了 winit 的 macOS 扩展 trait（经 eframe::winit）。
// `winit` 直接依赖仍保留：eframe 不重新导出 winit 全部 API，且 [patch] 必须版本对齐。
#[cfg(target_os = "macos")]
use eframe::winit::platform::macos::OptionAsAlt;

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
        // #188 caught-FFI hook：vendor/winit 已重铺为干净 0.30.13 基底（S1），
        // set_caught_draw_rect_panic_hook 随 #205 补丁移除，S3（#208）移植 IME+#188 时一并恢复。
    }

    apply_preferred_english_input_source();

    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        // #206 S1：显式 Glow，勿默默切 wgpu（0.34+ wgpu 成默认；切换另票拍板，见 #204）。
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(egui::vec2(1200.0, 820.0))
            .with_maximized(false)
            .with_app_id("mistterm")
            .with_icon(crate::ui::icons::app_window_icon_data()),
        ..Default::default()
    };

    // macOS：内容延伸到标题栏，标签和红绿灯同一行(透明标题栏)。
    // Option 当作 Alt：winit 0.30 的 `window_builder` 钩子收的是 egui ViewportBuilder，
    // 挂不上 winit WindowAttributes 扩展 → 改为首帧经 `winit_window()` 调
    // `set_option_as_alt`（见 app.rs `apply_macos_option_as_alt_once`）。
    #[cfg(target_os = "macos")]
    {
        options.viewport = options
            .viewport
            .with_fullsize_content_view(true)
            .with_title_shown(false);
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
            Ok(Box::new(crate::ui::MistTermApp::new(cc)))
        }),
    ) {
        // run_native 失败时 stdout/stderr 可能被重定向丢弃（e2e、GUI 子系统），
        // 必须同时进日志文件，否则 headless CI 只能看到「进程没了」而没有原因。
        log::error!("Failed to launch GUI: {e}");
        eprintln!("Failed to launch GUI: {e}");
        // GUI 启动失败也算「没走到正常退出」，不写 stamp（下次启动如果因此有新报告会提示）。
        std::process::exit(1);
    }
    // 正常返回（含 on_exit 后拆卸完成）：stamp 兕底，覆盖所有正常退出路径。
    crate::core::crash_reporter::mark_clean_exit();
}
