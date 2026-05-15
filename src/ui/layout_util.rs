//! 统一处理 egui 在部分布局阶段给出的 **无限可用宽度**，并给输入框留出左右留白，避免贴边与撑破布局。
//!
//! ## 响应式约定（与常见 egui / 桌面 App 一致）
//!
//! - **`SidePanel` 的 default / min / max**：用**当前窗口宽度**的比例算出像素，再夹在合理区间；用户拖拽后由 egui 记忆，不是「写死一列」。
//! - **表单 `TextEdit`**：宽度优先用「父级 `max_rect` 与 `available_width`」，[`finite_content_width`] 的上限随父级变，而不是固定 900px。
//! - **ScrollArea 高度**：用 `available_height()` 或屏幕比例，避免固定 300/420 在大屏过小、小屏溢出。

use eframe::egui;

const HUGE: f32 = 10_000.0;

/// 无法从 egui 读到有效屏宽时的回退值。
const SCREEN_WIDTH_FALLBACK: f32 = 1024.0;
/// 低于此宽度视为无效，改用 [`SCREEN_WIDTH_FALLBACK`]。
const SCREEN_WIDTH_MIN_VALID: f32 = 64.0;

/// 左栏拖拽：相对窗口宽度的比例区间（[`left_sidebar_drag_clamp`]）。
const LEFT_SIDEBAR_MIN_FRAC: f32 = 0.11;
const LEFT_SIDEBAR_MAX_FRAC: f32 = 0.32;
const LEFT_SIDEBAR_COLLAPSE_MIN_FRAC: f32 = 0.45;

/// 侧栏 default/min/max 不可同时满足时的最小拖拽跨度（px）。
const PANEL_DRAG_MIN_GAP: f32 = 44.0;
const PANEL_MIN_SPAN: f32 = 120.0;

/// 表单内容区：从父级 cap 两侧各减去的 inset（[`finite_content_width`]）。
const CONTENT_CAP_TRIM: f32 = 20.0;
const CONTENT_CAP_FLOOR: f32 = 1.0;
const CONTENT_FALLBACK_FRAC: f32 = 0.52;
/// 输入框最小宽度 = cap × 此比例（窄 cap 时避免 `lo > hi` panic）。
const CONTENT_FIELD_MIN_FRAC: f32 = 0.67;

#[derive(Clone, Copy)]
struct PanelWidthSpec {
    default_frac: f32,
    min_frac: f32,
    max_frac: f32,
}

/// 由窗口宽度比例算出 `(default, min, max)`，并保证 `min ≤ default ≤ max`。
fn panel_widths_from_spec(w: f32, spec: PanelWidthSpec) -> (f32, f32, f32) {
    let default_w = w * spec.default_frac;
    let mut min_w = w * spec.min_frac;
    let mut max_w = w * spec.max_frac;
    if min_w >= default_w {
        min_w = (default_w - PANEL_DRAG_MIN_GAP).max(PANEL_MIN_SPAN.min(w * spec.min_frac));
    }
    if max_w <= default_w {
        max_w = default_w + PANEL_DRAG_MIN_GAP.max(w * 0.12);
    }
    if min_w >= max_w {
        max_w = min_w + PANEL_MIN_SPAN.min(w * 0.2);
    }
    (default_w, min_w, max_w)
}

/// `f32::clamp` 在 `lo > hi` 时会 panic；布局窄窗/∞ 宽度时比例算出的上下界可能颠倒。
#[inline]
pub fn clamp_f32(v: f32, lo: f32, hi: f32) -> f32 {
    if lo.is_finite() && hi.is_finite() && lo <= hi {
        v.clamp(lo, hi)
    } else if lo.is_finite() && hi.is_finite() {
        v.clamp(hi, lo)
    } else if lo.is_finite() {
        lo
    } else if hi.is_finite() {
        hi
    } else {
        v
    }
}

/// egui 默认 `Frame::side_top_panel` 的 **水平** inner margin（左右各，与垂直 2px 无关）。
/// 自定义侧栏 `frame` 时须在回调里传入实际左侧 inner margin。
pub const EGUI_SIDE_PANEL_FRAME_MARGIN_X: f32 = 8.0;

/// 在 `SidePanel::show` **之后**用其返回的 `response` 记录右栏外缘左 x（整槽位矩形）。
/// 勿在回调开头用 `ui.max_rect()` / `clip_rect`：自定义 `frame.inner_margin` 时首帧常仍为整窗坐标，会把中央区裁错。
/// 多侧栏并存时取 **min(x)**（最靠主区的一侧）。
#[inline]
pub fn record_right_dock_panel(response: &egui::Response, acc: &mut Option<f32>) {
    let x = response.rect.min.x;
    if !x.is_finite() {
        return;
    }
    *acc = Some(match *acc {
        None => x,
        Some(v) => v.min(x),
    });
}

/// 将当前 Ui 的 clip 右缘收紧到右栏外缘（防止中央后绘盖住 dock）。
#[inline]
pub fn clip_ui_before_right_dock(ui: &mut egui::Ui, right_dock_outer_left_x: Option<f32>) {
    let Some(dock_left) = right_dock_outer_left_x else {
        return;
    };
    if !dock_left.is_finite() {
        return;
    }
    let mut clip = ui.clip_rect();
    if dock_left > clip.min.x {
        clip.max.x = clip.max.x.min(dock_left);
        ui.set_clip_rect(clip);
    }
}

/// 中央区在注册完左右 `SidePanel` 后的工作矩形（egui 已扣除侧栏槽位）。
#[inline]
pub fn central_work_rect(ctx: &egui::Context, right_dock_outer_left_x: Option<f32>) -> egui::Rect {
    let mut r = ctx.available_rect();
    if let Some(dock_left) = right_dock_outer_left_x {
        if dock_left.is_finite() && dock_left > r.min.x {
            r.max.x = r.max.x.min(dock_left);
        }
    }
    r
}

/// 中央区内终端列宽度：以工作区右缘与右栏外缘左 x 的较小值为准，避免 `available_width` 为 ∞ 时盖住右栏。
#[inline]
pub fn terminal_column_width(
    col_left: f32,
    work_max_x: f32,
    right_dock_outer_left_x: Option<f32>,
) -> f32 {
    if !col_left.is_finite() || !work_max_x.is_finite() || work_max_x <= col_left {
        return 1.0;
    }
    let mut right_edge = work_max_x;
    if let Some(dock_left) = right_dock_outer_left_x {
        if dock_left.is_finite() && dock_left > col_left {
            right_edge = right_edge.min(dock_left);
        }
    }
    (right_edge - col_left).max(1.0)
}

/// 中央区内某一列（如终端列）可用宽度（兼容旧调用；优先用 [`terminal_column_width`] + `work`）。
#[inline]
pub fn column_width_before_right_dock(
    ui: &egui::Ui,
    right_dock_outer_left_x: Option<f32>,
) -> f32 {
    let col_left = ui.max_rect().min.x;
    let work_max_x = ui.clip_rect().max.x;
    terminal_column_width(col_left, work_max_x, right_dock_outer_left_x)
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
    if w.is_finite() && w > SCREEN_WIDTH_MIN_VALID {
        w
    } else {
        SCREEN_WIDTH_FALLBACK
    }
}

/// 左栏会话列表拖拽范围：随窗口宽度变化（小窗不强行拉满，大窗不必死锁在 320）。
#[inline]
pub fn left_sidebar_drag_clamp(ctx: &egui::Context) -> (f32, f32) {
    let w = screen_width(ctx);
    let lo = w * LEFT_SIDEBAR_MIN_FRAC;
    let hi = w * LEFT_SIDEBAR_MAX_FRAC;
    if lo + PANEL_DRAG_MIN_GAP >= hi {
        let collapse_hi = w * LEFT_SIDEBAR_COLLAPSE_MIN_FRAC;
        (lo.max(PANEL_MIN_SPAN), collapse_hi.max(lo + PANEL_DRAG_MIN_GAP))
    } else {
        (lo, hi)
    }
}

/// `SidePanel` 的 `(default_width, min_width, max_width)`，按窗口宽度比例取值。
#[inline]
pub fn side_panel_widths(ctx: &egui::Context, profile: SidePanelProfile) -> (f32, f32, f32) {
    let w = screen_width(ctx);
    let spec = match profile {
        // 命令片段列表以标题 + 截断命令为主，过宽挤占终端；默认略窄且限制可拖拽上限
        SidePanelProfile::Fragment => PanelWidthSpec {
            default_frac: 0.158,
            min_frac: 0.11,
            max_frac: 0.30,
        },
        SidePanelProfile::Standard => PanelWidthSpec {
            default_frac: 0.235,
            min_frac: 0.17,
            max_frac: 0.46,
        },
        SidePanelProfile::Monitor => PanelWidthSpec {
            default_frac: 0.215,
            min_frac: 0.165,
            max_frac: 0.50,
        },
        SidePanelProfile::GitSync => PanelWidthSpec {
            default_frac: 0.205,
            min_frac: 0.155,
            max_frac: 0.44,
        },
    };
    panel_widths_from_spec(w, spec)
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
    cap = (cap - CONTENT_CAP_TRIM).max(CONTENT_CAP_FLOOR);
    let field_lo = cap * CONTENT_FIELD_MIN_FRAC;
    let fallback_mid = clamp_f32(cap * CONTENT_FALLBACK_FRAC, field_lo, cap);
    finite_content_width_inset(ui, CONTENT_CAP_TRIM * 0.5, fallback_mid, cap)
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
    let lo = max_width * CONTENT_FIELD_MIN_FRAC;
    clamp_f32(w, lo, max_width)
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
        clamp_f32(w, 64.0, cap.max(64.0))
    } else {
        clamp_f32(w, 64.0, 4096.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_f32, terminal_column_width, CONTENT_FALLBACK_FRAC, CONTENT_FIELD_MIN_FRAC,
    };

    #[test]
    fn clamp_f32_narrow_cap_does_not_panic() {
        let cap = 120.0;
        let lo = cap * CONTENT_FIELD_MIN_FRAC;
        let mid = clamp_f32(cap * CONTENT_FALLBACK_FRAC, lo, cap);
        assert!(mid >= lo && mid <= cap);
    }

    #[test]
    fn clamp_f32_normal_order() {
        assert_eq!(clamp_f32(150.0, 80.0, 200.0), 150.0);
    }

    #[test]
    fn terminal_column_width_stops_at_dock_left() {
        let col_left = 200.0_f32;
        let work_max = 900.0_f32;
        let dock_left = 600.0_f32;
        let w = terminal_column_width(col_left, work_max, Some(dock_left));
        assert_eq!(w, 400.0);
    }

    #[test]
    fn terminal_column_width_ignores_work_past_dock() {
        let col_left = 200.0_f32;
        let work_max = 900.0_f32;
        let dock_left = 600.0_f32;
        let w = terminal_column_width(col_left, work_max, Some(dock_left));
        assert!(col_left + w <= dock_left + 0.01);
    }
}
