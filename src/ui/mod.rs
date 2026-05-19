//! UI 层（egui 呈现与用户输入）
//!
//! ## 分层约定
//!
//! ```text
//!   ui/          ← 布局、绘制、事件（本目录）
//!     chrome/    ← 弹窗/侧栏控件样式，颜色来自 theme
//!     theme/     ← 设计 token（颜色、字号、间距、Frame 工厂）
//!     layout_util/ ← 侧栏宽度、∞ 宽度 clamp 等纯布局
//!     app.rs     ← 业务状态、tick、快捷键（布局见 workspace.rs）
//!     workspace.rs ← 顶栏 / 右 dock / 底栏 / 中央三列 / 弹窗（见 docs/product/LAYOUT.md）
//!     *_panel.rs ← 单块侧栏/弹窗
//!   core/        ← 业务规则与数据（无 egui）
//!   ssh/         ← 连接、传输、ZMODEM
//!   terminal/    ← VTE/ANSI（目标：与 egui 解耦，渲染在 ui/terminal）
//! ```
//!
//! **不要**在面板内写：自动重连退避、片段展开、≥10MB 上传策略等——应使用 `crate::core`。

pub mod layout_util;
pub mod icons;
pub mod chrome;
pub mod app;
pub mod terminal;
pub mod sidebar;
pub mod dialogs;
pub mod git_sync;
pub mod monitor_panel;
pub mod theme;
pub mod sftp_panel;
pub mod fragment_library;
pub mod credential_panel;
pub mod cloud_sync_panel;
pub mod ssh_config_import_dialog;
pub mod command_history_overlay;
pub mod session_log_dialog;
pub mod audit_log_dialog;
pub mod vault_form;
pub mod help_docs_dialog;

pub use app::MistTermApp;
pub use help_docs_dialog::{HelpDocsDialog, HelpPage};
pub use theme::{Theme, ThemeManager};
pub use monitor_panel::MonitorPanel;
