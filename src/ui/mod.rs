//! UI 模块
//!
//! 提供跨平台 GUI 界面，基于 egui + eframe

pub mod app;
pub mod terminal;
pub mod sidebar;
pub mod dialogs;

pub use app::MistTermApp;
