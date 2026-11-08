//! 弹窗 / 侧栏标题与操作按钮的统一视觉（关闭 ×、侧栏 ◀ 收起、主次按钮）。
//! 颜色与尺寸均来自 [`Theme`]，本模块不硬编码样式。
//!
//! #398 拆分：原单文件 5689 行按主题切成子模块；本文件只做 re-export，
//! 全部符号路径 `crate::ui::chrome::*` 不变，调用点零改动。

mod buttons;
mod dock;
mod forms;
mod modals;
mod panels;
mod tabs;

// ---- re-export：保持原 `chrome::符号` 路径不变 ----

pub use buttons::*;

pub use dock::*;

pub use forms::*;

pub use modals::*;

pub use panels::*;

pub use tabs::*;
