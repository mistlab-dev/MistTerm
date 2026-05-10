//! 统一处理 egui 在部分布局阶段给出的 **无限可用宽度**，并给输入框留出左右留白，避免贴边与撑破布局。
//!
//! ## 响应式约定（与常见 egui / 桌面 App 一致）
//!
//! - **`SidePanel` 的 default / min / max**：用**当前窗口宽度**的比例算出像素，再夹在合理区间；用户拖拽后由 egui 记忆，不是「写死一列」。
//! - **表单 `TextEdit`**：宽度优先用「父级 `max_rect` 与 `available_width`」，[`finite_content_width`] 的上限随父级变，而不是固定 900px。
//! - **ScrollArea 高度**：用 `available_height()` 或屏幕比例，避免固定 300/420 在大屏过小、小屏溢出。

use eframe::egui;

const HUGE: f32 = 10_000.0;

/// egui 默认 `Frame::side_top_panel` 的 **水平** inner margin（左右各，与垂直 2px 无关）。
/// 自定义侧栏 `frame` 时须在回调里传入实际左侧 inner margin。
pub const EGUI_SIDE_PANEL_FRAME_MARGIN_X: f32 = 8.0;

/// 在右侧 `SidePanel` 的 `.show` 回调 **第一行**（任意子布局之前）调用，累积「侧栏槽真实左缘」的最小 x。
/// egui 用 Frame 的 `response.rect.min.x` 收束主区，子布局未吃满槽位时该值会右移，中央区会多画一条竖带盖住侧栏；
/// 此处用 `content_ui.max_rect().min.x - frame_inner_margin_left` 还原 **`panel_rect.min.x`**。多侧栏并存时取 **min(x)**（最靠主区的一侧）。
#[inline]
pub fn record_right_dock_outer_left(
    ui: &egui::Ui,
    frame_inner_margin_left: f32,
    acc: &mut Option<f32>,
) {
    let x = ui.max_rect().min.x - frame_inner_margin_left;
    if !x.is_finite() {
        return;
    }
    *acc = Some(match *acc {
        None => x,
        Some(v) => v.min(x),
    });
}

/// 右栏/工具侧栏的宽度档位（比例系数在 [`side_panel_widths`] 内）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidePanelProfile {
    /// 命令片段列表（偏窄）
    Fragment,
    /// 凭证库、云端同步、SFTP 等（中等）
    Standard,
    /// 系统监控（图表略宽）
    Monitor,
    /// Git 同步（中等略窄）
    GitSync,
}

#[inline]
fn screen_width(ctx: &egui::Context) -> f32 {
    let w = ctx.screen_rect().width();
    if w.is_finite() && w > 64.0 {
        w
    } else {
        1024.0
    }
}

/// 左栏会话列表拖拽范围：随窗口宽度变化（小窗不强行拉满，大窗不必死锁在 320）。
#[inline]
pub fn left_sidebar_drag_clamp(ctx: &egui::Context) -> (f32, f32) {
    let w = screen_width(ctx);
    let lo = (w * 0.11).clamp(160.0, 220.0);
    let hi = (w * 0.32).clamp(260.0, 520.0);
    if lo + 48.0 >= hi {
        (160.0, (w * 0.45).min(480.0).max(240.0))
    } else {
        (lo, hi)
    }
}

/// `SidePanel` 的 `(default_width, min_width, max_width)`，按窗口宽度比例取值。
#[inline]
pub fn side_panel_widths(ctx: &egui::Context, profile: SidePanelProfile) -> (f32, f32, f32) {
    let w = screen_width(ctx);
    let (d_pct, min_pct, max_pct) = match profile {
        // 命令片段列表以标题 + 截断命令为主，过宽挤占终端；默认略窄且限制可拖拽上限
        SidePanelProfile::Fragment => (0.158, 0.11, 0.30),
        SidePanelProfile::Standard => (0.235, 0.17, 0.46),
        SidePanelProfile::Monitor => (0.215, 0.165, 0.50),
        SidePanelProfile::GitSync => (0.205, 0.155, 0.44),
    };
    let default_w = (w * d_pct).clamp(200.0, 520.0);
    let mut min_w = (w * min_pct).clamp(140.0, 380.0);
    let mut max_w = (w * max_pct).clamp(280.0, 900.0);
    if min_w >= default_w {
        min_w = (default_w - 44.0).max(120.0);
    }
    if max_w <= default_w {
        max_w = default_w + 140.0;
    }
    if min_w >= max_w {
        max_w = min_w + 120.0;
    }
    (default_w, min_w, max_w)
}

/// 居中弹窗类 `Window::default_width`（新建会话、克隆仓库等）。
#[inline]
pub fn modal_default_width(ctx: &egui::Context) -> f32 {
    (screen_width(ctx) * 0.36).clamp(320.0, 600.0)
}

/// 底部锚定条带（如终端搜索）的默认宽度。
#[inline]
pub fn floating_bar_default_width(ctx: &egui::Context) -> f32 {
    (screen_width(ctx) * 0.42).clamp(440.0, 760.0)
}

/// 片段库主窗口：`(default [w,h], min [w,h])`，按屏幕尺寸比例。
#[inline]
pub fn fragment_library_window_bounds(ctx: &egui::Context) -> ([f32; 2], [f32; 2]) {
    let r = ctx.screen_rect();
    let sw = r.width().max(360.0);
    let sh = r.height().max(280.0);
    let default = [
        (sw * 0.74).clamp(520.0, 1200.0),
        (sh * 0.80).clamp(400.0, 960.0),
    ];
    let min_sz = [
        (sw * 0.50).clamp(360.0, 900.0),
        (sh * 0.42).clamp(300.0, 720.0),
    ];
    (default, min_sz)
}

/// 快速选择器 / 变量对话框等居中窗口的默认尺寸。
#[inline]
pub fn centered_window_default_size(ctx: &egui::Context, w_frac: f32, h_frac: f32) -> [f32; 2] {
    let r = ctx.screen_rect();
    let sw = r.width().max(360.0);
    let sh = r.height().max(280.0);
    [(sw * w_frac).clamp(380.0, 900.0), (sh * h_frac).clamp(260.0, 800.0)]
}

/// 「填写片段变量」等表单弹窗：`fixed_size` 用屏幕比例夹在合理区间。
#[inline]
pub fn fragment_vars_modal_size(ctx: &egui::Context) -> egui::Vec2 {
    let r = ctx.screen_rect();
    let sw = r.width().max(340.0);
    let sh = r.height().max(260.0);
    egui::vec2(
        (sw * 0.46).clamp(360.0, 520.0),
        (sh * 0.52).clamp(300.0, 560.0),
    )
}

/// 对话框内纵向滚动区最大高度：剩余屏高的一部分，避免写死 300/420。
#[inline]
pub fn dialog_scroll_max_height(ctx: &egui::Context, chrome_reserve: f32) -> f32 {
    let h = ctx.screen_rect().height();
    if !h.is_finite() {
        return 320.0;
    }
    let inner = (h - chrome_reserve).max(80.0);
    inner.clamp(160.0, (h * 0.62).min(720.0))
}

/// 侧栏/面板内 `ScrollArea`：吃掉当前 `Ui` 剩余高度（减去顶部控件占位）。
#[inline]
pub fn scroll_area_fill_height(ui: &egui::Ui, reserve_top: f32) -> f32 {
    let mut h = ui.available_height() - reserve_top;
    if !h.is_finite() || h > HUGE {
        h = ui.max_rect().height() - reserve_top;
    }
    if !h.is_finite() || h < 48.0 {
        h = finite_content_height(ui, 200.0, 900.0);
    }
    h.clamp(100.0, 4000.0)
}

/// 典型弹窗/表单行：左右留白后宽度不超过**当前父级**，随容器伸缩。
#[inline]
pub fn finite_content_width(ui: &egui::Ui) -> f32 {
    let mut cap = ui.max_rect().width();
    if !cap.is_finite() || cap > HUGE {
        cap = ui.available_width();
    }
    if !cap.is_finite() {
        cap = 640.0;
    }
    cap = (cap - 20.0).max(120.0);
    let fallback_mid = (cap * 0.52).clamp(200.0, cap);
    finite_content_width_inset(ui, 10.0, fallback_mid, cap)
}

/// 从当前 `Ui` 取可用宽度，减去左右 `inset`，失败时用 `fallback`，并夹在 `[80, max_width]`。
#[inline]
pub fn finite_content_width_inset(
    ui: &egui::Ui,
    inset_each_side: f32,
    fallback: f32,
    max_width: f32,
) -> f32 {
    let mut w = ui.available_width() - 2.0 * inset_each_side;
    if !w.is_finite() || w > HUGE {
        w = ui.max_rect().width() - 2.0 * inset_each_side;
    }
    if !w.is_finite() || w < 32.0 {
        w = fallback;
    }
    w.clamp(80.0, max_width)
}

/// 侧栏、工具条等仅需要「不是 ∞」的宽度，仍保留少量边距。
#[inline]
pub fn finite_avail_minus(ui: &egui::Ui, subtract: f32, fallback: f32, max_w: f32) -> f32 {
    let mut w = ui.available_width() - subtract;
    if !w.is_finite() || w > HUGE {
        w = ui.max_rect().width() - subtract;
    }
    if !w.is_finite() || w < 24.0 {
        w = fallback;
    }
    w.clamp(48.0, max_w)
}

/// 与 [`finite_content_width_inset`] 类似，用于纵向分配（侧栏、滚动区等）。
#[inline]
pub fn finite_content_height(ui: &egui::Ui, fallback: f32, max_h: f32) -> f32 {
    let mut h = ui.available_height();
    if !h.is_finite() || h > HUGE {
        h = ui.max_rect().height();
    }
    if !h.is_finite() || h < 1.0 {
        h = fallback;
    }
    h.clamp(40.0, max_h)
}

/// 供 `TextEdit` / 多行编辑：宽度**绝不超出**当前 `Ui` 的 `max_rect`（勿用 `clip_rect`，在根区域常与整窗同宽，会误放大）。
#[inline]
pub fn textedit_width_in_parent(ui: &egui::Ui, subtract: f32) -> f32 {
    let mut w = ui.available_width() - subtract;
    if !w.is_finite() || w > HUGE {
        w = ui.max_rect().width() - subtract;
    }
    if !w.is_finite() || w < 32.0 {
        w = 200.0;
    }
    let cap = ui.max_rect().width();
    if cap.is_finite() && cap > 16.0 {
        w = w.min(cap - 8.0);
    }
    if cap.is_finite() {
        w.clamp(64.0, cap.max(64.0))
    } else {
        w.clamp(64.0, 4096.0)
    }
}
