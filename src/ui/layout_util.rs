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
/// 左右 dock 可拖拽上限（设计规范 §3.4 / §8.1）
const SIDE_PANEL_MAX_WIDTH_PX: f32 = 320.0;
const SIDE_PANEL_MIN_WIDTH_PX: f32 = 160.0;
const LEFT_SIDEBAR_DEFAULT_FRAC: f32 = 0.18;
const LEFT_SIDEBAR_MIN_PX: f32 = 160.0;
const LEFT_SIDEBAR_MAX_PX: f32 = 320.0;

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
    let min_w = min_w.max(SIDE_PANEL_MIN_WIDTH_PX);
    let max_w = max_w.min(SIDE_PANEL_MAX_WIDTH_PX).max(min_w + PANEL_DRAG_MIN_GAP);
    let default_w = default_w.clamp(min_w, max_w);
    (default_w, min_w, max_w)
}

/// 左侧连接栏默认宽度：屏宽 18%，夹在 160～320px。
#[inline]
pub fn default_sidebar_width(ctx: &egui::Context) -> f32 {
    (screen_width(ctx) * LEFT_SIDEBAR_DEFAULT_FRAC).clamp(LEFT_SIDEBAR_MIN_PX, LEFT_SIDEBAR_MAX_PX)
}

/// 持久化/拖拽后的左栏宽度合法区间。
#[inline]
pub fn clamp_sidebar_width(w: f32) -> f32 {
    clamp_f32(w, LEFT_SIDEBAR_MIN_PX, LEFT_SIDEBAR_MAX_PX)
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

/// 命令片段 `SidePanel` 的 id（与 `egui::SidePanel::right` 一致，供 [`side_panel_state_rect`] 读取）。
pub const FRAGMENT_PANEL_ID: &str = "fragment_panel";
pub const MONITOR_PANEL_ID: &str = "monitor_panel";
pub const AI_PANEL_ID: &str = "ai_panel";

/// 读取上一帧 `SidePanel` 落盘的槽位矩形（用于 Central 之后 Foreground 重绘）。
#[inline]
pub fn side_panel_state_rect(ctx: &egui::Context, panel_id: &str) -> Option<egui::Rect> {
    egui::containers::panel::PanelState::load(ctx, egui::Id::new(panel_id)).map(|s| s.rect)
}

/// 将侧栏槽位右缘钉在 `screen` 右缘（`PanelState` 仅为内层内容矩形，勿直接作 Area 外框）。
/// `screen_inset` 取 [`Theme::spacing_right_dock_screen_inset`].
#[inline]
pub fn pin_rect_to_screen_right_edge(
    content: egui::Rect,
    screen: egui::Rect,
    width: f32,
    screen_inset: f32,
) -> egui::Rect {
    let w = width.max(1.0);
    let inset = screen_inset.max(0.0);
    let max_x = screen.max.x - inset;
    let min_x = (max_x - w).max(screen.min.x);
    let min_y = content.min.y.max(screen.min.y);
    let max_y = content.max.y.min(screen.max.y);
    egui::Rect::from_min_max(egui::pos2(min_x, min_y), egui::pos2(max_x, max_y))
}

/// 在 [`SidePanel`] 槽位基础上内缩，保证右边框完整落在可见区内。
#[inline]
pub fn inset_slot_for_foreground_paint(
    slot: egui::Rect,
    screen: egui::Rect,
    screen_inset: f32,
) -> egui::Rect {
    let mut r = slot.intersect(screen);
    if !r.is_positive() {
        return r;
    }
    let inset = screen_inset.max(0.0);
    if r.max.x > screen.max.x - inset {
        r.max.x = screen.max.x - inset;
    }
    if r.max.x <= r.min.x {
        r.max.x = r.min.x + 48.0;
    }
    r
}

/// Foreground 槽位：优先本帧 `SidePanel` 内 `ui.max_rect()`（与布局占位一致）；否则回退钉右缘。
#[inline]
pub fn right_dock_foreground_slot(
    panel_slot_rect: Option<egui::Rect>,
    ctx: &egui::Context,
    panel_id: &str,
    profile: SidePanelProfile,
    layout_content_rect: Option<egui::Rect>,
    screen_inset: f32,
) -> Option<egui::Rect> {
    if let Some(slot) = panel_slot_rect.filter(|r| r.is_positive() && r.width() >= 48.0) {
        return Some(slot.intersect(ctx.screen_rect()));
    }
    right_dock_slot_rect(ctx, panel_id, profile, layout_content_rect, screen_inset)
}

/// 右 dock Foreground 槽位：宽取本帧布局内容或 `PanelState`，**右缘对齐屏右**（整块侧栏可见）。
#[inline]
pub fn right_dock_slot_rect(
    ctx: &egui::Context,
    panel_id: &str,
    profile: SidePanelProfile,
    layout_content_rect: Option<egui::Rect>,
    screen_inset: f32,
) -> Option<egui::Rect> {
    let screen = ctx.screen_rect();
    let _ = profile;
    let content = layout_content_rect
        .filter(|r| r.is_positive())
        .or_else(|| side_panel_state_rect(ctx, panel_id))?;
    // 右 dock 若已使用 exact_width/统一列宽，不应再按 profile 二次夹取。
    let w = content.width().max(1.0);
    if w < 48.0 || !w.is_finite() {
        return None;
    }
    let slot = pin_rect_to_screen_right_edge(content, screen, w, screen_inset);
    if !slot.is_positive() {
        return None;
    }
    Some(slot)
}

/// Foreground 重绘用矩形（兼容旧名；优先 [`right_dock_slot_rect`] + 本帧布局 rect）。
#[inline]
pub fn side_panel_foreground_rect(
    ctx: &egui::Context,
    panel_id: &str,
    screen_inset: f32,
) -> Option<egui::Rect> {
    let profile = if panel_id == MONITOR_PANEL_ID {
        SidePanelProfile::Monitor
    } else if panel_id == FRAGMENT_PANEL_ID {
        SidePanelProfile::Fragment
    } else {
        return side_panel_state_rect(ctx, panel_id).and_then(|r| {
            if r.is_positive() {
                Some(r.intersect(ctx.screen_rect()))
            } else {
                None
            }
        });
    };
    right_dock_slot_rect(ctx, panel_id, profile, None, screen_inset)
}

/// Foreground 内正文宽（扣 `region_panel_frame` 水平 inner margin，勿信 Area 内 clip≈整窗）。
#[inline]
pub fn side_panel_foreground_inner_width(slot: egui::Rect, margin: egui::Margin) -> f32 {
    (slot.width() - margin.left - margin.right).max(48.0)
}

/// Foreground 正文宽：来自 SidePanel 槽位 `inner`（随拖拽变化），夹在 profile 的 min/max 内。
#[inline]
pub fn right_dock_foreground_content_width(
    ctx: &egui::Context,
    inner_width: f32,
    profile: SidePanelProfile,
) -> f32 {
    let (_, min_w, max_w) = side_panel_widths(ctx, profile);
    clamp_f32(inner_width, min_w, max_w)
}

/// 将 Ui 锁在右 dock 正文宽（防 Foreground 内 ∞ `available_width` 撑大 SidePanel）。
#[inline]
pub fn constrain_ui_to_right_dock_body(ui: &mut egui::Ui, body_w: f32) -> f32 {
    let w = body_w.max(48.0);
    ui.set_min_width(w);
    ui.set_max_width(w);
    w
}

/// 右 dock / ScrollArea 子 Ui：以**当前**可用宽为上限（已含滚动条占位），勿信 ∞ 的 `available_width`。
#[inline]
pub fn set_width_to_available(ui: &mut egui::Ui) -> f32 {
    let mut w = ui.available_width();
    if !w.is_finite() || w > HUGE {
        w = ui.max_rect().width();
    }
    let cap = ui.max_rect().width();
    if cap.is_finite() && cap > 1.0 && cap < HUGE {
        w = w.min(cap);
    }
    if !w.is_finite() || w < 1.0 {
        w = SIDE_PANEL_MIN_WIDTH_PX;
    }
    ui.set_max_width(w);
    w
}

/// 在 `SidePanel::show` **之后**用槽位矩形记录右栏外缘左 x。
/// 优先 [`right_dock_slot_rect`]（右缘对齐屏）；勿用内层 `response.rect` 的 min.x 当外缘（易偏左、中央区裁错）。
#[inline]
pub fn record_right_dock_panel_rect(rect: &egui::Rect, acc: &mut Option<f32>) {
    if !rect.is_positive() {
        return;
    }
    let x = rect.min.x;
    if rect.width() < 48.0 {
        return;
    }
    *acc = Some(match *acc {
        None => x,
        Some(v) => v.min(x),
    });
}

/// 兼容：用 `response.rect` 记录（仅当尚无 [`right_dock_slot_rect`] 时作回退）。
#[inline]
pub fn record_right_dock_panel(response: &egui::Response, acc: &mut Option<f32>) {
    record_right_dock_panel_rect(&response.rect, acc);
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

/// 主内容区底缘 y（状态栏顶线；主区/右 dock 不得越过）。
#[inline]
pub fn workspace_bottom_y(screen: egui::Rect, status_bar_height: f32) -> f32 {
    (screen.max.y - status_bar_height.max(0.0)).max(screen.min.y)
}

/// 将矩形底缘裁到状态栏之上（防止侧栏/终端/右 dock 盖住底栏）。
#[inline]
pub fn clamp_rect_above_status_bar(
    mut rect: egui::Rect,
    screen: egui::Rect,
    status_bar_height: f32,
) -> egui::Rect {
    let bottom = workspace_bottom_y(screen, status_bar_height);
    if rect.max.y > bottom {
        rect.max.y = bottom;
    }
    if rect.min.y >= bottom {
        rect.max.y = bottom;
        rect.min.y = (bottom - 1.0).max(screen.min.y);
    }
    rect
}

/// 工作区内缩一圈 padding（列布局与终端宽度以此矩形为准）。
#[inline]
pub fn work_area_inner_rect(work: egui::Rect, pad: f32) -> egui::Rect {
    if pad <= 0.0 || !pad.is_finite() {
        return work;
    }
    work.shrink(pad)
}

/// 右/左 SidePanel 内一行可用宽：`max_rect` 为槽位宽；`clip_rect` 在根 Ui 常为整窗，不可优先取 clip。
#[inline]
pub fn side_panel_row_width(ui: &egui::Ui) -> f32 {
    let max_r = ui.max_rect().width();
    let clip_w = ui.clip_rect().width();
    let mut w = if max_r.is_finite() && max_r > 8.0 && max_r < HUGE {
        max_r
    } else if clip_w.is_finite() && clip_w > 8.0 && clip_w < HUGE {
        clip_w
    } else {
        ui.available_width()
    };
    if max_r.is_finite() && clip_w.is_finite() && clip_w < max_r {
        w = w.min(clip_w);
    }
    if w.is_finite() && w > 8.0 {
        w
    } else {
        SIDE_PANEL_MIN_WIDTH_PX
    }
}

/// 右 dock 内真实内容宽（勿用 2000px 上限，否则图表撑出侧栏被窗口裁切）
#[inline]
pub fn dock_panel_content_width(ui: &egui::Ui, min_w: f32, max_w: f32) -> f32 {
    let w = side_panel_row_width(ui);
    clamp_f32(w, min_w, max_w)
}

/// 右 dock 内表单/图表宽度上限 = 当前 panel 宽度
#[inline]
pub fn finite_content_width_in_panel(ui: &egui::Ui, inset_each_side: f32, fallback: f32) -> f32 {
    let cap = dock_panel_content_width(ui, 48.0, SIDE_PANEL_MAX_WIDTH_PX);
    finite_content_width_inset(ui, inset_each_side, fallback, cap)
}

/// 中央区工作矩形：须在 `CentralPanel::show` 回调内用 `ui.max_rect()`，再按右栏外缘与底栏顶线收紧
#[inline]
pub fn central_work_rect_in_ui(
    ui: &egui::Ui,
    right_dock_outer_left_x: Option<f32>,
    status_bar_height: f32,
) -> egui::Rect {
    let mut r = ui.max_rect().intersect(ui.clip_rect());
    if let Some(dock_left) = right_dock_outer_left_x {
        if dock_left.is_finite() && dock_left > r.min.x {
            r.max.x = r.max.x.min(dock_left);
        }
    }
    clamp_rect_above_status_bar(r, ui.ctx().screen_rect(), status_bar_height)
}

/// 兼容旧调用；优先 [`central_work_rect_in_ui`]
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
            default_frac: 0.18,
            min_frac: 0.11,
            max_frac: 0.30,
        },
        SidePanelProfile::Standard => PanelWidthSpec {
            default_frac: 0.22,
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

/// 新建 / 编辑会话弹窗尺寸（§8.4.1）。
#[inline]
pub fn modal_edit_size(ctx: &egui::Context) -> egui::Vec2 {
    let r = ctx.screen_rect();
    let sw = r.width().max(360.0);
    let sh = r.height().max(280.0);
    egui::vec2(
        (sw * 0.36).clamp(340.0, 520.0),
        (sh * 0.48).clamp(360.0, 540.0),
    )
}

/// 偏好设置弹窗（§8.4.2）。
#[inline]
pub fn modal_pref_size(ctx: &egui::Context) -> egui::Vec2 {
    let r = ctx.screen_rect();
    let sw = r.width().max(360.0);
    let sh = r.height().max(280.0);
    egui::vec2(
        (sw * 0.40).clamp(380.0, 560.0),
        (sh * 0.42).clamp(320.0, 560.0),
    )
}

/// 关于弹窗（§8.4.3）。
#[inline]
pub fn modal_about_size(ctx: &egui::Context) -> egui::Vec2 {
    let r = ctx.screen_rect();
    let sw = r.width().max(360.0);
    let sh = r.height().max(280.0);
    egui::vec2(
        (sw * 0.38).clamp(360.0, 520.0),
        (sh * 0.44).clamp(340.0, 540.0),
    )
}

/// 快速片段选择器（§8.4.4）。
#[inline]
pub fn modal_quick_fragment_size(ctx: &egui::Context) -> egui::Vec2 {
    let r = ctx.screen_rect();
    let sw = r.width().max(360.0);
    let sh = r.height().max(280.0);
    egui::vec2(
        (sw * 0.42).clamp(360.0, 560.0),
        (sh * 0.32).clamp(220.0, 380.0),
    )
}

/// Clone 仓库弹窗（§8.4.5）。
#[inline]
pub fn modal_clone_size(ctx: &egui::Context) -> egui::Vec2 {
    let r = ctx.screen_rect();
    let sw = r.width().max(360.0);
    let sh = r.height().max(280.0);
    egui::vec2(
        (sw * 0.38).clamp(340.0, 520.0),
        (sh * 0.26).clamp(180.0, 320.0),
    )
}

/// 删除确认等小弹窗（§8.4.6）。
#[inline]
pub fn modal_confirm_size(ctx: &egui::Context) -> egui::Vec2 {
    let r = ctx.screen_rect();
    let sw = r.width().max(360.0);
    let sh = r.height().max(280.0);
    egui::vec2(
        (sw * 0.36).clamp(320.0, 480.0),
        (sh * 0.24).clamp(160.0, 280.0),
    )
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
        clamp_f32, inset_slot_for_foreground_paint, pin_rect_to_screen_right_edge,
        terminal_column_width, work_area_inner_rect, CONTENT_FALLBACK_FRAC,
        CONTENT_FIELD_MIN_FRAC,
    };
    use crate::ui::chrome::right_dock_slot_content_rect;
    use crate::ui::theme::Theme;

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

    #[test]
    fn work_area_inner_rect_shrinks_by_pad() {
        let work = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 800.0));
        let inner = work_area_inner_rect(work, 8.0);
        assert_eq!(inner.min, egui::pos2(8.0, 8.0));
        assert_eq!(inner.max, egui::pos2(992.0, 792.0));
    }

    #[test]
    fn side_panel_row_width_prefers_max_over_window_clip() {
        // 模拟 SidePanel 根：max=320（槽位），clip=1200（整窗）
        let max_r = egui::Rect::from_min_max(egui::pos2(880.0, 0.0), egui::pos2(1200.0, 800.0));
        let clip_r = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1200.0, 800.0));
        let max_r_w = max_r.width();
        let clip_w = clip_r.width();
        let mut w = max_r_w;
        if clip_w < max_r_w {
            w = w.min(clip_w);
        }
        assert_eq!(w, 320.0);
    }

    #[test]
    fn work_area_inner_rect_zero_pad_is_identity() {
        let work = egui::Rect::from_min_max(egui::pos2(10.0, 20.0), egui::pos2(110.0, 220.0));
        let inner = work_area_inner_rect(work, 0.0);
        assert_eq!(inner, work);
    }

    #[test]
    fn pin_rect_to_screen_right_edge_ignores_content_min_x_drift() {
        let screen =
            egui::Rect::from_min_max(egui::pos2(0.0, 28.0), egui::pos2(1200.0, 800.0));
        // 内层内容若按整窗宽排版，min.x 可能偏左
        let content =
            egui::Rect::from_min_max(egui::pos2(0.0, 28.0), egui::pos2(1200.0, 800.0));
        let inset = Theme::dark().spacing_right_dock_screen_inset();
        let slot = pin_rect_to_screen_right_edge(content, screen, 320.0, inset);
        assert!((slot.max.x - (screen.max.x - inset)).abs() < 0.01);
        assert!((slot.width() - 320.0).abs() < 0.01);
        assert!((slot.min.x - (880.0 - inset)).abs() < 0.01);
    }

    #[test]
    fn right_dock_slot_content_rect_symmetric_margin() {
        let theme = Theme::dark();
        let slot =
            egui::Rect::from_min_max(egui::pos2(100.0, 20.0), egui::pos2(400.0, 320.0));
        let inner = right_dock_slot_content_rect(slot, &theme);
        let m = theme.region_content_margin();
        assert!((inner.min.x - (slot.min.x + m.left)).abs() < 0.01);
        assert!((inner.max.x - (slot.max.x - m.right)).abs() < 0.01);
        assert!((inner.width() - (slot.width() - m.left - m.right)).abs() < 0.01);
    }

    #[test]
    fn inset_slot_for_foreground_paint_shrinks_right_edge() {
        let screen =
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));
        let slot =
            egui::Rect::from_min_max(egui::pos2(700.0, 28.0), egui::pos2(1000.0, 572.0));
        let inset = Theme::dark().spacing_right_dock_screen_inset();
        let paint = inset_slot_for_foreground_paint(slot, screen, inset);
        assert!((paint.max.x - (screen.max.x - inset)).abs() < 0.01);
        assert!(paint.max.x < slot.max.x);
    }
}
