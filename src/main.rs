//! MistTerm - 异步 SSH 终端
//!
//! Windows 使用 GUI 子系统，避免启动时额外弹出控制台窗口（见 `windows_subsystem`）。
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

//! 架构分层见 `mistterm` 库（`src/lib.rs`）；本文件仅为 GUI 入口。

use eframe::egui;
use mistterm::ui::MistTermApp;

fn main() -> eframe::Result<()> {
    // macOS：嵌入 Info.plist，使菜单栏/Dock 显示 Mist 而非可执行文件名 mistterm
    #[cfg(target_os = "macos")]
    embed_plist::embed_info_plist!("../Info.plist");

    mistterm::platform::init_runtime_logging();

    log::info!("Mist starting");

    // macOS：菜单栏显示名（避免显示可执行文件名 mistterm）
    #[cfg(target_os = "macos")]
    {
        mistterm::platform::set_application_display_name();
        mistterm::platform::activate_gui_application();
    }

    // macOS：启动时尝试切到「ABC」英文键盘布局（需在系统里启用过该输入源）
    mistterm::platform::apply_preferred_english_input_source();

    // 默认约 1200×820；不限制 max 宽高，以便系统「最大化」与宽屏铺满（设计稿 1440 为内容参考，非硬上限）
    let options = eframe::NativeOptions {
        maximized: false,
        initial_window_size: Some(egui::vec2(1200.0, 820.0)),
        max_window_size: None,
        app_id: Some("mistterm".to_string()),
        icon_data: Some(mistterm::ui::icons::app_window_icon_data()),
        ..Default::default()
    };

    eframe::run_native(
        mistterm::platform::APP_DISPLAY_NAME,
        options,
        Box::new(|cc| {
            if !mistterm::platform::configure_egui_fonts(
                &cc.egui_ctx,
                mistterm::platform::TerminalFontPreset::default(),
            ) {
                log::warn!("CJK font not loaded; Chinese UI text may show as tofu");
            }
            mistterm::ui::icons::UiIcons::install(&cc.egui_ctx);
            Box::new(MistTermApp::new(cc))
        }),
    )
}
