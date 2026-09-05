//! 统一处理 egui 在部分布局阶段给出的 **无限可用宽度**，并给输入框留出左右留白，避免贴边与撑破布局。
//!
//! ## 响应式约定(与常见 egui / 桌面 App 一致)
//!
//! - **`SidePanel` 的 default / min / max**：用**当前窗口宽度**的比例算出像素，再夹在合理区间；用户拖拽后由 egui 记忆，不是「写死一列」。
//! - **表单 `TextEdit`**：宽度优先用「父级 `max_rect` 与 `available_width`」，[`finite_content_width`] 的上限随父级变，而不是固定 900px。
//! - **ScrollArea 高度**：用 `available_height()` 或屏幕比例，避免固定 300/420 在大屏过小、小屏溢出。

mod dock_geometry;
mod modal_dialog;

pub use dock_geometry::*;
pub use modal_dialog::*;
