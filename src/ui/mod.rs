//! UI 模块
//!
//! 提供跨平台 GUI 界面，基于 egui + eframe

pub mod app;
pub mod terminal;
pub mod sidebar;
pub mod dialogs;
pub mod git_sync;
pub mod theme;
pub mod monitor_panel;

pub use app::MistTermApp;
pub use theme::{Theme, ThemeManager};
pub use monitor_panel::MonitorPanel;
