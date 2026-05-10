//! MistTerm - 异步 SSH 终端
//! 
//! 架构分层:
//! - UI 层 (ui/): egui 界面
//! - 核心层 (core/): 会话管理、连接管理
//! - SSH 层 (ssh/): SSH 连接和通信
//! - 终端层 (terminal/): 终端模拟和 ANSI 解析
//! - lrzsz 层 (lrzsz/): ZMODEM 文件传输协议
//! - sync 层 (sync/): Git 同步
//! - security 层 (security/): 密钥链管理

pub mod core;
pub mod ssh;
pub mod terminal;
pub mod ui;
pub mod sync;
pub mod security;
pub mod monitor;

use eframe::egui;
use mistterm::ui::MistTermApp;

fn main() -> eframe::Result<()> {
    // 初始化日志 - 输出到控制台，包含 debug 级别
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(true)
        .with_thread_ids(true)
        .init();
    
    log::info!("MistTerm 启动");

    // macOS：启动时尝试切到「ABC」英文键盘布局（需在系统里启用过该输入源）
    mistterm::platform::apply_preferred_english_input_source();

    // docs/product/SPECIFICATION_DETAILED.md §1.1：约 820 高；最大宽约 1440（eframe 0.23 用 initial_window_size）
    let options = eframe::NativeOptions {
        maximized: false,
        initial_window_size: Some(egui::vec2(1200.0, 820.0)),
        max_window_size: Some(egui::vec2(1440.0, 2160.0)),
        app_id: Some("mistterm".to_string()),
        ..Default::default()
    };
    
    eframe::run_native(
        "MistTerm",
        options,
        Box::new(|cc| {
            configure_fonts(&cc.egui_ctx);
            Box::new(MistTermApp::new(cc))
        }),
    )
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    if let Some(cjk_font) = load_cjk_font() {
        let cjk_name = "mistterm-cjk".to_string();
        fonts.font_data.insert(cjk_name.clone(), cjk_font);

        // Proportional 可以优先用 CJK，保证中文 UI 不缺字
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, cjk_name.clone());
        // Monospace 必须把 CJK 放后面，避免把等宽英文挤成“看起来不等宽”
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push(cjk_name);
    }

    ctx.set_fonts(fonts);
}

fn load_cjk_font() -> Option<egui::FontData> {
    let candidates = [
        // macOS
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        // Common Linux distributions
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJKSC-Regular.otf",
    ];

    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            return Some(egui::FontData::from_owned(bytes));
        }
    }

    None
}
