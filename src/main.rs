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

mod core;
mod ssh;
mod terminal;
mod ui;
mod lrzsz;
mod sync;
mod security;

use ui::MistTermApp;

fn main() -> eframe::Result<()> {
    // 初始化日志
    tracing_subscriber::fmt::init();
    
    let options = eframe::NativeOptions {
        maximized: true,
        ..Default::default()
    };
    
    eframe::run_native("MistTerm", options, Box::new(|cc| Box::new(MistTermApp::new(cc))))
}
