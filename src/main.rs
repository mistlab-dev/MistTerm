//! MistTerm - 异步 SSH 终端
//! 
//! 架构分层:
//! - UI 层 (ui/): egui 界面
//! - 核心层 (core/): 会话管理、连接管理
//! - SSH 层 (ssh/): SSH 连接和通信
//! - 终端层 (terminal/): 终端模拟和 ANSI 解析

mod core;
mod ssh;
mod terminal;
mod ui;

use ui::MistTermApp;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    
    let options = eframe::NativeOptions {
        maximized: true,
        ..Default::default()
    };
    
    eframe::run_native("MistTerm", options, Box::new(|_cc| Box::new(MistTermApp::default())))
}
