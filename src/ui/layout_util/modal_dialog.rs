//! Modal, dialog, form-content, and scroll-area sizing calculations.

use eframe::egui;

use super::dock_geometry::{
    clamp_f32, dock_panel_content_width, screen_width, SIDE_PANEL_MAX_WIDTH_PX,
};

const HUGE: f32 = 10_000.0;

/// 表单内容区：从父级 cap 两侧各减去的 inset([`finite_content_width`])。
const CONTENT_CAP_TRIM: f32 = 20.0;
const CONTENT_CAP_FLOOR: f32 = 1.0;
const CONTENT_FALLBACK_FRAC: f32 = 0.52;
/// 输入框最小宽度 = cap × 此比例(窄 cap 时避免 `lo > hi` panic)。
const CONTENT_FIELD_MIN_FRAC: f32 = 0.67;

/// 右 dock 内表单/图表宽度上限 = 当前 panel 宽度
#[inline]
pub fn finite_content_width_in_panel(ui: &egui::Ui, inset_each_side: f32, fallback: f32) -> f32 {
    let cap = dock_panel_content_width(ui, 48.0, SIDE_PANEL_MAX_WIDTH_PX);
    finite_content_width_inset(ui, inset_each_side, fallback, cap)
}

/// 居中弹窗类 `Window::default_width`(新建会话、克隆仓库等)。
#[inline]
pub fn modal_default_width(ctx: &egui::Context) -> f32 {
    (screen_width(ctx) * 0.36).clamp(320.0, 600.0)
}

/// 底部锚定条带(如终端搜索)的默认宽度。
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

/// 弹窗**首次**打开时在 `screen_rect` 内居中的左上角坐标。
/// 须配合 [`egui::Window::default_pos`] 使用；勿用 [`.anchor`](egui::Window::anchor)，
/// anchor 每帧按约束区重算，拖拽后会弹回居中。
#[inline]
pub fn modal_center_pos(ctx: &egui::Context, size: egui::Vec2) -> egui::Pos2 {
    modal_center_pos_clamped(ctx, size, 0.0, 0.0)
}

/// 在 [`modal_center_pos`] 基础上把窗口钳在顶栏/底边安全区内，避免标题被裁切。
#[inline]
pub fn modal_center_pos_clamped(
    ctx: &egui::Context,
    size: egui::Vec2,
    top_inset: f32,
    bottom_inset: f32,
) -> egui::Pos2 {
    let r = ctx.screen_rect();
    let w = size.x.max(1.0);
    let h = size.y.max(1.0);
    let side = 8.0;
    let mut x = r.min.x + (r.width() - w) * 0.5;
    let mut y = r.min.y + (r.height() - h) * 0.5;
    x = x.clamp(r.min.x + side, (r.max.x - w - side).max(r.min.x + side));
    let y_min = r.min.y + top_inset.max(0.0);
    let y_max = (r.max.y - h - bottom_inset.max(0.0)).max(y_min);
    y = y.clamp(y_min, y_max);
    egui::pos2(x, y)
}

/// 快速选择器 / 变量对话框等居中窗口的默认尺寸。
#[inline]
pub fn centered_window_default_size(ctx: &egui::Context, w_frac: f32, h_frac: f32) -> [f32; 2] {
    let r = ctx.screen_rect();
    let sw = r.width().max(360.0);
    let sh = r.height().max(280.0);
    [
        (sw * w_frac).clamp(380.0, 900.0),
        (sh * h_frac).clamp(260.0, 800.0),
    ]
}

/// 新建会话弹窗(名称 / 主机 / 端口 / 用户名 / 密码 / SSH 密钥)。
#[inline]
pub fn modal_new_session_size(ctx: &egui::Context) -> egui::Vec2 {
    let r = ctx.screen_rect();
    let sw = r.width().max(360.0);
    egui::vec2((sw * 0.36).clamp(340.0, 480.0), 390.0)
}

/// 新建 / 编辑会话弹窗尺寸(§8.4.1)。
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

/// 偏好设置弹窗(§8.4.2)：高度不超过视口减去 `top_inset` / `bottom_inset`。
#[inline]
pub fn modal_pref_size_in_viewport(
    ctx: &egui::Context,
    top_inset: f32,
    bottom_inset: f32,
) -> egui::Vec2 {
    let r = ctx.screen_rect();
    let sw = r.width().max(360.0);
    let sh = r.height().max(280.0);
    let max_h = (sh - top_inset.max(0.0) - bottom_inset.max(0.0)).max(280.0);
    let ideal_h = (sh * 0.62).clamp(480.0, 780.0);
    egui::vec2((sw * 0.44).clamp(420.0, 600.0), ideal_h.min(max_h))
}

/// 偏好设置弹窗默认尺寸(关于等复用；含基础边距)。
#[inline]
pub fn modal_pref_size(ctx: &egui::Context) -> egui::Vec2 {
    modal_pref_size_in_viewport(ctx, 24.0, 24.0)
}

/// 关于弹窗(§8.4.3)默认尺寸(无内容测量时的回退)。
#[inline]
pub fn modal_about_size(ctx: &egui::Context) -> egui::Vec2 {
    let r = ctx.screen_rect();
    let sw = r.width().max(360.0);
    let sh = r.height().max(280.0);
    egui::vec2(
        (sw * 0.44).clamp(520.0, 680.0),
        (sh * 0.44).clamp(340.0, 540.0),
    )
}

/// 关于弹窗：按标题、版本与快捷键最长行测量宽高，避免灰色内容区过宽或过窄。
pub fn modal_about_size_for_content(
    ctx: &egui::Context,
    theme: &crate::ui::theme::Theme,
    about_title: &str,
    subtitle: &str,
    version_line: &str,
    shortcuts: &str,
) -> egui::Vec2 {
    let screen = ctx.screen_rect();
    let sw = screen.width().max(360.0);
    let sh = screen.height().max(280.0);

    let measure = |text: &str, font: egui::FontId| -> egui::Vec2 {
        ctx.fonts(|fonts| {
            fonts
                .layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)
                .size()
        })
    };

    let header_title_font = egui::FontId::proportional(theme.font_size_panel_header_title());
    let prominent_font = egui::FontId::proportional(theme.font_size_prominent());
    let panel_font = egui::FontId::proportional(theme.font_size_panel_title());
    let mono_font = egui::FontId::monospace(theme.font_size_small());

    let shortcuts_lines: Vec<&str> = shortcuts.lines().collect();
    let shortcuts_line_sizes: Vec<egui::Vec2> = shortcuts_lines
        .iter()
        .map(|line| measure(line, mono_font.clone()))
        .collect();

    let shortcuts_content_w = shortcuts_line_sizes
        .iter()
        .map(|size| size.x)
        .fold(0.0_f32, f32::max);

    let content_text_w = shortcuts_content_w
        .max(measure(crate::platform::APP_DISPLAY_NAME, prominent_font.clone()).x)
        .max(measure(subtitle, panel_font.clone()).x)
        .max(measure(version_line, panel_font.clone()).x);

    let inset_mx = theme.spacing_search_input_x() * 2.0;
    let modal_mx = theme.spacing_modal_content_x() * 2.0;
    let content_w = content_text_w + inset_mx + modal_mx;

    let header_title_w =
        theme.size_icon_glyph() + theme.spacing_sm() + measure(about_title, header_title_font).x;
    let header_min_w = header_title_w + theme.size_panel_header_control_h() + modal_mx;

    let width = content_w
        .max(header_min_w)
        .max(modal_about_size(ctx).x)
        .clamp(520.0, (sw - 48.0).min(680.0));

    let item_spacing_y = ctx.style().spacing.item_spacing.y;
    let line_h = shortcuts_line_sizes
        .first()
        .map(|size| size.y)
        .unwrap_or(12.0);
    let shortcuts_h = if shortcuts_lines.is_empty() {
        0.0
    } else {
        shortcuts_lines.len() as f32 * line_h
            + shortcuts_lines.len().saturating_sub(1) as f32 * item_spacing_y
    };
    const SHORTCUTS_SCROLL_MAX_H: f32 = 200.0;
    let scroll_h = shortcuts_h.min(SHORTCUTS_SCROLL_MAX_H);

    let modal_my = theme.spacing_modal_content_y() * 2.0;
    let inset_my = theme.spacing_search_input_y() * 2.0;
    let height = modal_my
        + theme.size_panel_header_row_h()
        + theme.spacing_modal_header_after_sep()
        + measure(crate::platform::APP_DISPLAY_NAME, prominent_font).y
        + item_spacing_y
        + measure(subtitle, panel_font.clone()).y
        + theme.spacing_md()
        + inset_my
        + measure(version_line, panel_font).y
        + theme.spacing_panel_gap()
        + scroll_h
        + 2.0;

    let max_h = modal_about_size(ctx).y.min(sh - 48.0);
    egui::vec2(width, height.clamp(220.0, max_h))
}

/// 快速片段选择器(§8.4.4)。
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

/// Clone 仓库弹窗(§8.4.5)。
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

/// 删除确认等小弹窗(§8.4.6)。
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

/// 侧栏/面板内 `ScrollArea`：吃掉当前 `Ui` 剩余高度(减去顶部控件占位)。
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

/// 与 [`finite_content_width_inset`] 类似，用于纵向分配(侧栏、滚动区等)。
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

/// 供 `TextEdit` / 多行编辑：宽度**绝不超出**当前 `Ui` 的 `max_rect`(勿用 `clip_rect`，在根区域常与整窗同宽，会误放大)。
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
    use super::{CONTENT_FALLBACK_FRAC, CONTENT_FIELD_MIN_FRAC};
    use crate::ui::layout_util::clamp_f32;

    #[test]
    fn clamp_f32_narrow_cap_does_not_panic() {
        let cap = 120.0;
        let lo = cap * CONTENT_FIELD_MIN_FRAC;
        let mid = clamp_f32(cap * CONTENT_FALLBACK_FRAC, lo, cap);
        assert!(mid >= lo && mid <= cap);
    }
}
