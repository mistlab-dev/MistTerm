//! 标签条 / 顶部标题栏 / 状态栏 / 活动栏（#398 自 chrome.rs 拆出）。
//! 纯移动，零行为变更。

use crate::ui::icons::{self, IconId};
use crate::ui::theme::Theme;
use eframe::egui::{
    self, Button, Color32, CursorIcon, Painter, Response, RichText, Sense, Stroke, Ui, Widget,
};
use super::*;

/// Tab 栏图标按钮(关闭 / 新建)：固定点击区、悬停底。
pub fn tab_bar_icon_button(ui: &mut Ui, theme: &Theme, id: IconId, tooltip: &str) -> Response {
    theme_icon_hit(
        ui,
        theme,
        id,
        theme.size_tab_bar_icon_btn(),
        theme.size_icon_glyph(),
        theme.color_tab_bar_icon(),
        theme.color_tab_bar_icon_hover(),
    )
    .on_hover_text(tooltip)
}


/// 标签栏「新建 Tab」按钮(与 Tab 芯片同高、垂直居中)
pub fn tab_bar_new_tab_button(ui: &mut Ui, theme: &Theme) -> Response {
    let row_h = theme.size_tab_bar_row_h();
    let icon = theme.size_tab_bar_icon_btn();
    let accel = crate::platform::accel("T");
    // #377 软注：新建会话快捷键是 ⌘N / Ctrl+N，按平台取文案避免混拼。
    let accel_n = crate::platform::accel("N");
    let tooltip = match crate::i18n::language(ui.ctx()) {
        crate::i18n::UiLanguage::En => format!(
            "New session · {accel_n} — pick which connection to open ({accel} opens the current selection)",
        ),
        crate::i18n::UiLanguage::Zh => format!(
            "新建会话 · {accel_n} — 点此选择连哪台（{accel} 打开当前选中的连接）",
        ),
    };
    ui.allocate_ui_with_layout(
        egui::vec2(icon, row_h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| tab_bar_icon_button(ui, theme, IconId::Plus, tooltip.as_str()),
    )
    .inner
}


/// 标题栏连接区展示数据(§三)
#[derive(Clone)]
pub struct TitleBarConnection {
    pub server_text: String,
    pub status_label: String,
    pub online: bool,
    pub connecting: bool,
}

pub(crate) fn paint_top_strip(ui: &mut Ui, rect: egui::Rect, theme: &Theme) {
    ui.painter().rect_filled(rect, 0.0, theme.chrome_bar_fill());
}


/// 顶栏：仅菜单行(连接信息在 Tab / 底栏，避免与顶栏重复)
pub fn render_top_chrome_panel(
    ui: &mut Ui,
    theme: &Theme,
    show_in_window_menu: bool,
    mut draw_menu: impl FnMut(&mut Ui),
    pending_ssh_imports: usize,
    show_ssh_import_chip: bool,
) -> TitleBarChromeResult {
    let width = ui.available_width();
    let h = ui.available_height().min(theme.menu_bar_height());
    let origin = ui.cursor().min;
    let rect = egui::Rect::from_min_size(origin, egui::vec2(width, h));
    ui.allocate_exact_size(rect.size(), egui::Sense::hover());

    paint_top_strip(ui, rect, theme);
    let mut out = TitleBarChromeResult::default();
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.set_clip_rect(rect);
        let content_h = h;
        ui.set_min_height(content_h);
        ui.style_mut().spacing.interact_size.y = content_h;
        egui::menu::MenuBar::new().ui(ui, |ui| {
            if show_in_window_menu {
                ui.spacing_mut().item_spacing.x = theme.spacing_menu_bar_gap();
                draw_menu(ui);
            }
            if show_ssh_import_chip && pending_ssh_imports > 0 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.add_space(theme.spacing_title_bar_x());
                    out = ssh_import_chip_actions(ui, theme, pending_ssh_imports);
                });
            }
        });
    });
    out
}


/// 顶栏菜单行上的 SSH 导入 chip 等动作
#[derive(Default)]
pub struct TitleBarChromeResult {
    pub open_ssh_import: bool,
    pub dismiss_ssh_import: bool,
}

/// VS Code 风格 Tab 底栏指示线(2 物理像素 accent)
pub(crate) fn paint_vscode_tab_bottom_indicator(
    painter: &egui::Painter,
    ctx: &egui::Context,
    rect: egui::Rect,
    theme: &Theme,
) {
    let h = theme.tab_indicator_height(ctx);
    let bottom = theme.snap_y_to_pixel(ctx, rect.bottom());
    let top = theme.snap_y_to_pixel(ctx, rect.bottom() - h);
    let bar = egui::Rect::from_min_max(
        egui::pos2(rect.left(), top),
        egui::pos2(rect.right(), bottom),
    );
    // 略淡于满色 accent，避免标签底指示条刺眼
    painter.rect_filled(bar, 0.0, theme.accent_dim_color());
}


/// 终端区会话 Tab：整块底色(圆点 + 标题 + 关闭)，对齐 proto `.tab`。
pub struct SessionTabChipResult {
    pub response: Response,
    pub close_clicked: bool,
}

/// 标签右侧关闭槽位(与 [`session_tab_chip`] 内关闭按钮对齐)。
pub(crate) fn pointer_hovers_tab_close_slot(ctx: &egui::Context, inner: egui::Rect, close_slot: f32) -> bool {
    let close_rect = egui::Rect::from_min_size(
        egui::pos2(
            inner.max.x - close_slot,
            inner.center().y - close_slot * 0.5,
        ),
        egui::vec2(close_slot, close_slot),
    );
    ctx.pointer_hover_pos()
        .is_some_and(|p| close_rect.contains(p))
}


/// 状态圆点半径上限（needs_attention 时 3.0，其余 2.5）。
const STATUS_DOT_R_MAX: f32 = 3.0;
/// 进度环半径与描边：`OUTER` 含半个描边，是环的外缘。
const PROGRESS_RING_RADIUS: f32 = 3.5;
const PROGRESS_RING_STROKE: f32 = 1.5;
const PROGRESS_RING_OUTER: f32 = PROGRESS_RING_RADIUS + PROGRESS_RING_STROKE * 0.5;
/// 圆点与进度环之间的间隙。
const PROGRESS_RING_GAP: f32 = 1.0;
/// 圆点占据的横向槽位（圆心 2.5 + 最大半径），环从它右侧开始排。
const STATUS_DOT_SLOT: f32 = 2.5 + STATUS_DOT_R_MAX;
/// 环圆心相对 `inner.min.x` 的偏移：紧贴圆点外侧，左缘不与点边界相交。
const PROGRESS_RING_OFFSET_X: f32 =
    STATUS_DOT_SLOT + PROGRESS_RING_GAP + PROGRESS_RING_OUTER;

/// 标签左侧状态槽位宽度：无进度时只放圆点，有进度时右侧再并排放一个不重叠的环。
pub(crate) fn status_slot_width(has_progress: bool) -> f32 {
    if has_progress {
        PROGRESS_RING_OFFSET_X + PROGRESS_RING_OUTER + 0.5
    } else {
        5.0
    }
}


/// 进度环圆心（绝对 x）：与圆点同一条中线，横向并排不重叠。
pub(crate) fn progress_ring_center_x(inner_min_x: f32) -> f32 {
    inner_min_x + PROGRESS_RING_OFFSET_X
}


/// `needs_attention`：#93 未聚焦标签有 BEL / SessionEnded 时画 accent 圆点。
pub fn session_tab_chip(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    active: bool,
    online: bool,
    needs_attention: bool,
    show_close: bool,
    env_color: Option<egui::Color32>,
    progress: Option<crate::terminal::sem_osc::ProgressReport>,
) -> SessionTabChipResult {
    let close_slot = theme.size_tab_bar_icon_btn();
    let pad_x = theme.spacing_tab_x();
    let pad_y = theme.spacing_tab_y();
    let gap_dot = theme.spacing_tab_dot_text();
    let gap_close = theme.spacing_tab_icon_gap();
    let label_color = if active {
        theme.text_primary()
    } else if theme.uses_modern_palette() {
        // #130 实机巡检：暗夜未选中 0.55 仅 ~3.07:1，小字偏暗；抬到 0.72
        // （仍低于选中态 text_primary），晨曦/浅色路径不动。
        theme.text_secondary().gamma_multiply(0.72)
    } else {
        theme.text_tertiary()
    };
    let label_galley = ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(
            label.to_owned(),
            egui::FontId::proportional(theme.font_size_tab_label()),
            label_color,
        )
    });
    // 宽度随内容：状态槽位（圆点，有进度时再加一个独立的进度环）+ 标题 + 间距 + ×。
    let slot_w = status_slot_width(progress.is_some());
    let content_w = slot_w + gap_dot + label_galley.size().x + gap_close + close_slot;
    let tab_w = (content_w + pad_x * 2.0).max(72.0);
    let size = egui::vec2(tab_w, theme.size_tab_min_h());
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let inner = rect.shrink2(egui::vec2(pad_x, pad_y));
    let close_slot_hot = pointer_hovers_tab_close_slot(ui.ctx(), inner, close_slot);
    // 子控件(×)会抢走外层 hover；用关闭槽位命中避免 × 显隐来回切换。
    let tab_hot = response.hovered() || close_slot_hot;
    let modern = theme.uses_modern_palette();
    let fill = if let Some(c) = env_color {
        // 未选中尽量淡，避免多标签糊成一整块；当前标签更实一点
        let a = if active {
            72u8
        } else if tab_hot {
            22
        } else {
            8
        };
        if theme.is_light_theme() {
            // 浅底上半透明叠色几乎看不出来(当前标签像没选中)；按比例混成实色。
            let t = a as f32 / 255.0 * 1.6;
            let base = theme.bg_tab_bar_color();
            let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
            Color32::from_rgb(mix(base.r(), c.r()), mix(base.g(), c.g()), mix(base.b(), c.b()))
        } else {
            Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
        }
    } else if modern {
        if tab_hot && !active {
            theme.color_widget_hover_fill()
        } else {
            Color32::TRANSPARENT
        }
    } else if active {
        theme.color_tab_active_fill()
    } else if tab_hot {
        theme.color_tab_inactive_hover_fill()
    } else {
        theme.color_tab_inactive_fill()
    };
    // 所有主题统一：上圆下方(与浅色主题同半径)，底边贴住指示线/内容区。
    let rounding = theme.tab_rounding();
    let stroke = if modern || active {
        egui::Stroke::NONE
    } else {
        egui::Stroke::new(1.0, theme.color_tab_inactive_stroke())
    };
    ui.painter().rect(rect, rounding, fill, stroke, egui::StrokeKind::Middle);
    // 左侧色条：当前标签实色，未选中变淡，避免连成一片
    if let Some(c) = env_color {
        let bar_w = 3.0;
        let bar = egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + bar_w, rect.bottom()),
        );
        let bar_rounding = egui::CornerRadius {
            nw: rounding.nw,
            ne: 0,
            sw: rounding.sw,
            se: 0,
        };
        let bar_color = if active {
            c.gamma_multiply(0.95)
        } else if tab_hot {
            Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 110)
        } else {
            Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 55)
        };
        ui.painter().rect_filled(bar, bar_rounding, bar_color);
    }
    if active {
        paint_vscode_tab_bottom_indicator(ui.painter(), ui.ctx(), rect, theme);
    }
    let mut close_clicked = false;
    // 圆点/标题/× 都按 tab 矩形的垂直中线绝对定位(标题按字形墨迹居中)，
    // 不靠嵌套 horizontal 的行高推算，避免 macOS 标题栏里内容偏下。
    let cy = rect.center().y;
    // 未聚焦且需注意：accent 圆点盖过在线色；聚焦标签不显示（状态已清）。
    let (status_color, status_r) = if needs_attention && !active {
        (theme.accent_color(), 3.0)
    } else if online {
        (theme.green_color(), 2.5)
    } else {
        (theme.color_tab_offline_dot(), 2.5)
    };
    let dot_center = egui::pos2(inner.min.x + 2.5, cy);
    ui.painter().circle_filled(dot_center, status_r, status_color);
    // #392：OSC 9;4 进度指示——圆点右侧独立槽位里的微型环，与圆点同高居中；
    // error/warning 用对应色。以前贴在点的右上角（cx+5 / cy-4），外缘 4.25 会压住
    // 半径为 3.0 的圆点，且整体比文字行高 4px，所以改成并排 + 居中对齐。
    if let Some(p) = progress {
        let ring_c = match p.state {
            crate::terminal::sem_osc::ProgressState::Error => theme.red_color(),
            crate::terminal::sem_osc::ProgressState::Warning => theme.amber_color(),
            _ => theme.accent_color(),
        };
        let ring_cx = progress_ring_center_x(inner.min.x);
        let ring_cy = cy;
        match p.progress {
            Some(pct) => {
                // 确定进度：底环淡色 + 进度弧。
                let radius = PROGRESS_RING_RADIUS;
                let bg = Color32::from_rgba_unmultiplied(ring_c.r(), ring_c.g(), ring_c.b(), 60);
                ui.painter().circle_stroke(
                    egui::pos2(ring_cx, ring_cy),
                    radius,
                    egui::Stroke::new(PROGRESS_RING_STROKE, bg),
                );
                let frac = pct as f32 / 100.0;
                let start = std::f32::consts::FRAC_PI_2; // 12 点方向起
                let end = start - std::f32::consts::TAU * frac;
                let center = egui::Pos2::new(ring_cx, ring_cy);
                let steps = ((frac * 24.0).ceil() as usize).max(2);
                let pts: Vec<egui::Pos2> = (0..=steps)
                    .map(|i| {
                        let ang = start + (end - start) * (i as f32 / steps as f32);
                        center + radius * egui::vec2(ang.cos(), -ang.sin())
                    })
                    .collect();
                ui.painter().add(egui::Shape::line(
                    pts,
                    egui::Stroke::new(PROGRESS_RING_STROKE, ring_c),
                ));
            }
            None => {
                // 不定进度：实心小圆点占位（静态，闪烁动画成本高不做）。
                ui.painter().circle_filled(
                    egui::pos2(ring_cx, ring_cy),
                    PROGRESS_RING_RADIUS - PROGRESS_RING_STROKE * 0.5,
                    ring_c,
                );
            }
        }
    }
    let ink = label_galley
        .rows
        .first()
        .map(|r| r.visuals.mesh_bounds)
        .filter(|b| b.is_positive())
        .unwrap_or_else(|| egui::Rect::from_min_size(egui::Pos2::ZERO, label_galley.size()));
    let label_x = inner.min.x + slot_w + gap_dot;
    let label_w = label_galley.size().x;
    ui.painter().galley(
        egui::pos2(label_x, cy - ink.center().y),
        label_galley,
        egui::Color32::WHITE,
    );
    // × 紧跟标题；始终占位，仅切换绘制，避免显隐导致 hover 闪烁。
    let close_visible = show_close || active || tab_hot;
    let close_tooltip = format!(
        "{} · {}",
        crate::i18n::tr(ui.ctx(), "Close tab", "关闭标签"),
        crate::platform::accel("W")
    );
    let close_rect = egui::Rect::from_center_size(
        egui::pos2(label_x + label_w + gap_close + close_slot * 0.5, cy),
        egui::vec2(close_slot, close_slot),
    );
    let mut close_ui = ui.new_child(egui::UiBuilder::new().max_rect(close_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let close_resp = theme_icon_hit_revealed(
        &mut close_ui,
        theme,
        IconId::Close,
        close_slot,
        theme.size_icon_glyph(),
        theme.color_tab_bar_icon(),
        theme.color_tab_bar_icon_hover(),
        close_visible,
    );
    let close_resp = if close_visible {
        close_resp.on_hover_text(close_tooltip.as_str())
    } else {
        close_resp
    };
    if close_visible && close_resp.clicked() {
        close_clicked = true;
    }
    SessionTabChipResult {
        response,
        close_clicked,
    }
}


#[inline]
pub(crate) fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    ((a as f32) * (1.0 - t) + (b as f32) * t).round() as u8
}


/// 标题栏 macOS 风格红绿灯(装饰；真实关/最小化/最大化由系统窗口按钮处理)
pub fn title_bar_traffic_lights(ui: &mut Ui, theme: &Theme) {
    let r = theme.radius_traffic_light();
    let gap = 7.0;
    let slot_w = r * 2.0 * 3.0 + gap * 2.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(slot_w, r * 2.0), egui::Sense::hover());
    let cy = rect.center().y;
    let mut x = rect.left() + r;
    for color in [
        Color32::from_rgb(255, 95, 86),
        Color32::from_rgb(255, 189, 46),
        Color32::from_rgb(39, 201, 63),
    ] {
        ui.painter().circle_filled(egui::pos2(x, cy), r, color);
        x += r * 2.0 + gap;
    }
}


/// 自绘标题栏窗口按钮(Win/Linux 无边框沉浸式用)。
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowCaptionActions {
    pub minimize: bool,
    pub toggle_maximize: bool,
    pub close: bool,
}

pub fn window_caption_buttons(
    ui: &mut Ui,
    theme: &Theme,
    maximized: bool,
    bar_h: f32,
) -> WindowCaptionActions {
    let mut out = WindowCaptionActions::default();
    let btn_w = 40.0;
    let btn_h = bar_h;
    let icon = theme.size_icon_glyph().min(12.0);
    let idle = theme.color_toolbar_glyph_idle();
    let hover = theme.color_toolbar_glyph_hover();

    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.horizontal(|ui| {
            // Minimize
            let (r, resp) = ui.allocate_exact_size(egui::vec2(btn_w, btn_h), Sense::click());
            if resp.hovered() {
                ui.painter()
                    .rect_filled(r, 0.0, theme.fg_high_alpha(18));
            }
            let c = if resp.hovered() { hover } else { idle };
            let y = r.center().y;
            ui.painter().line_segment(
                [
                    egui::pos2(r.center().x - icon * 0.45, y),
                    egui::pos2(r.center().x + icon * 0.45, y),
                ],
                Stroke::new(1.2, c),
            );
            if resp.clicked() {
                out.minimize = true;
            }

            // Maximize / Restore
            let (r, resp) = ui.allocate_exact_size(egui::vec2(btn_w, btn_h), Sense::click());
            if resp.hovered() {
                ui.painter()
                    .rect_filled(r, 0.0, theme.fg_high_alpha(18));
            }
            let c = if resp.hovered() { hover } else { idle };
            let s = icon * 0.85;
            let rect = egui::Rect::from_center_size(r.center(), egui::vec2(s, s));
            if maximized {
                // restore: two overlapping squares
                let a = rect.translate(egui::vec2(1.5, -1.5));
                let b = rect.translate(egui::vec2(-1.5, 1.5));
                ui.painter().rect_stroke(a, 0.0, Stroke::new(1.1, c), egui::StrokeKind::Inside);
                ui.painter().rect_filled(
                    b,
                    0.0,
                    theme.chrome_bar_fill(),
                );
                ui.painter().rect_stroke(b, 0.0, Stroke::new(1.1, c), egui::StrokeKind::Inside);
            } else {
                ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.1, c), egui::StrokeKind::Inside);
            }
            if resp.clicked() {
                out.toggle_maximize = true;
            }

            // Close
            let (r, resp) = ui.allocate_exact_size(egui::vec2(btn_w, btn_h), Sense::click());
            if resp.hovered() {
                ui.painter()
                    .rect_filled(r, 0.0, Color32::from_rgb(196, 43, 28));
            }
            let c = if resp.hovered() {
                Color32::WHITE
            } else {
                idle
            };
            let icon_rect =
                egui::Rect::from_center_size(r.center(), egui::vec2(icon + 2.0, icon + 2.0));
            icons::paint_icon(ui, icon_rect, IconId::Close, c, icon + 2.0);
            if resp.clicked() {
                out.close = true;
            }
        });
    });
    out
}


/// 标题栏可拖拽空白区：按住拖动窗口；双击切换最大化。
pub fn title_bar_drag_region(
    ui: &mut Ui,
    ctx: &egui::Context,
    size: egui::Vec2,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    if resp.double_clicked() {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    } else if resp.is_pointer_button_down_on() {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    let _ = rect;
    resp
}


/// 状态栏内容区可用高度(与底栏 Panel 内边距一致)。
pub fn status_bar_content_height(theme: &Theme) -> f32 {
    theme.chrome_bar_content_height(theme.status_bar_height())
}


/// 状态栏文字徽章(统一字号；由父级 `Align::Center` 负责垂直对齐)。
pub fn status_text_chip(ui: &mut Ui, theme: &Theme, text: &str, color: Color32) -> Response {
    theme
        .frame_status_chip()
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .size(theme.font_size_status_bar())
                    .color(color),
            );
        })
        .response
}


/// 状态栏工具图标
pub fn status_tool_icon(ui: &mut Ui, theme: &Theme, id: IconId) -> Response {
    let h = status_bar_content_height(theme);
    let hit = theme.size_icon_glyph().max(20.0);
    ui.allocate_ui_with_layout(
        egui::vec2(hit, h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            theme_icon_hit(
                ui,
                theme,
                id,
                hit,
                theme.size_icon_glyph(),
                theme.color_toolbar_glyph_idle(),
                theme.color_toolbar_glyph_hover(),
            )
        },
    )
    .inner
}


/// Activity Rail 图标按钮(选中时 accent 底)。
pub fn activity_rail_button(
    ui: &mut Ui,
    theme: &Theme,
    id: IconId,
    selected: bool,
    tooltip: &str,
    caption: &str,
) -> Response {
    activity_rail_button_with_badge(ui, theme, id, selected, tooltip, caption, None)
}


/// 活动栏按钮；`badge` 为右上角告警数角标（#251）。
pub fn activity_rail_button_with_badge(
    ui: &mut Ui,
    theme: &Theme,
    id: IconId,
    selected: bool,
    tooltip: &str,
    caption: &str,
    badge: Option<usize>,
) -> Response {
    let side = theme.size_activity_rail_btn();
    let label_h = if caption.is_empty() { 0.0 } else { 14.0 };
    let size = egui::vec2(side, side + label_h);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let icon_rect = egui::Rect::from_min_size(rect.min, egui::vec2(side, side));
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    if selected || hovered || pressed {
        let a = if selected {
            48
        } else if pressed {
            40
        } else {
            28
        };
        ui.painter().rect_filled(
            icon_rect,
            theme.radius_list_item(),
            theme.accent_alpha(a),
        );
    }
    let color = if selected {
        theme.accent_color()
    } else if hovered {
        theme.color_toolbar_glyph_hover()
    } else {
        theme.color_toolbar_glyph_idle()
    };
    let icon_px = theme.size_activity_rail_icon().max(18.0);
    let glyph = egui::Rect::from_center_size(icon_rect.center(), egui::vec2(icon_px, icon_px));
    icons::paint_icon(ui, glyph, id, color, icon_px);
    if let Some(n) = badge.filter(|n| *n > 0) {
        paint_activity_rail_badge(ui, theme, icon_rect, n);
    }
    if !caption.is_empty() {
        let label_color = if selected {
            theme.accent_color()
        } else {
            theme.text_secondary()
        };
        ui.painter().text(
            egui::pos2(icon_rect.center().x, rect.bottom() - 1.0),
            egui::Align2::CENTER_BOTTOM,
            caption,
            egui::FontId::proportional(10.0),
            label_color,
        );
    }
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response.on_hover_text(tooltip)
}


pub(crate) fn paint_activity_rail_badge(ui: &mut Ui, theme: &Theme, icon_rect: egui::Rect, count: usize) {
    let label = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    let font = egui::FontId::proportional(9.0);
    let galley = ui.painter().layout_no_wrap(label, font.clone(), egui::Color32::WHITE);
    let pad_x = 4.0;
    let pad_y = 1.5;
    let bw = (galley.size().x + pad_x * 2.0).max(14.0);
    let bh = (galley.size().y + pad_y * 2.0).max(12.0);
    let center = egui::pos2(icon_rect.right() - 2.0, icon_rect.top() + 2.0);
    let badge_rect = egui::Rect::from_center_size(center, egui::vec2(bw, bh));
    ui.painter().rect_filled(
        badge_rect,
        bh * 0.5,
        theme.color_status_negative_text(),
    );
    ui.painter().galley(
        egui::pos2(
            badge_rect.center().x - galley.size().x * 0.5,
            badge_rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        Color32::WHITE,
    );
}


/// Rail 隐藏时的左缘恢复条：点击显示活动栏。
pub fn activity_rail_reveal_strip(ui: &mut Ui, theme: &Theme, tooltip: &str) -> Response {
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, theme.chrome_bar_fill());
    let (rect, response) = ui.allocate_exact_size(full.size(), Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.painter()
            .rect_filled(rect, 0.0, theme.accent_alpha(36));
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    // 中央细 chevron(›)示意「点此展开」
    let mid = rect.center();
    let stroke = egui::Stroke::new(
        1.25,
        if hovered {
            theme.accent_color()
        } else {
            theme.color_toolbar_glyph_idle()
        },
    );
    let painter = ui.painter();
    let x = mid.x - 0.5;
    let y = mid.y;
    painter.line_segment(
        [egui::pos2(x - 1.5, y - 5.0), egui::pos2(x + 1.5, y)],
        stroke,
    );
    painter.line_segment(
        [egui::pos2(x + 1.5, y), egui::pos2(x - 1.5, y + 5.0)],
        stroke,
    );
    response.on_hover_text(tooltip)
}


/// 右下角 Toast 交互结果。
#[derive(Debug, Clone, Copy, Default)]
pub struct StatusToastActions {
    pub primary: bool,
    pub secondary: bool,
    pub dismiss: bool,
}

pub(crate) fn toast_kind_accent(theme: &Theme, kind: crate::ui::app::ToastKind) -> Color32 {
    match kind {
        crate::ui::app::ToastKind::Error => theme.red_color(),
        crate::ui::app::ToastKind::Warn => theme.amber_color(),
        crate::ui::app::ToastKind::Success => theme.green_color(),
        crate::ui::app::ToastKind::Info => theme.accent_color(),
    }
}


/// 测算 Toast 外框尺寸(标题 + 正文；用于 `Area::fixed_pos`)。
pub(crate) fn measure_status_toast_size(
    ctx: &egui::Context,
    theme: &Theme,
    title: &str,
    text: &str,
    action_label: Option<&str>,
    secondary_label: Option<&str>,
    show_dismiss: bool,
) -> egui::Vec2 {
    if title.is_empty() && text.is_empty() {
        return egui::Vec2::ZERO;
    }
    let title_font = egui::FontId::proportional(theme.font_size_ui_control());
    let body_font = egui::FontId::proportional(theme.font_size_caption());
    let max_text_w = theme.toast_max_text_width();
    let btn_h = theme.toast_action_btn_h();
    let has_actions = action_label.is_some() || secondary_label.is_some();
    let dismiss_only = show_dismiss && !has_actions;
    let dismiss_reserve = if dismiss_only { 22.0 } else { 0.0 };
    let title_galley = if title.is_empty() {
        None
    } else {
        Some(ctx.fonts_mut(|f| {
            f.layout(title.to_owned(), title_font, Color32::WHITE, max_text_w)
        }))
    };
    let body_galley = if text.is_empty() {
        None
    } else {
        Some(ctx.fonts_mut(|f| {
            f.layout(text.to_owned(), body_font, Color32::WHITE, max_text_w)
        }))
    };
    let gap = if title_galley.is_some() && body_galley.is_some() {
        theme.toast_title_body_gap()
    } else {
        0.0
    };
    let text_w = title_galley
        .as_ref()
        .map(|g| g.size().x)
        .unwrap_or(0.0)
        .max(body_galley.as_ref().map(|g| g.size().x).unwrap_or(0.0));
    let text_h = title_galley.as_ref().map(|g| g.size().y).unwrap_or(0.0)
        + gap
        + body_galley.as_ref().map(|g| g.size().y).unwrap_or(0.0);
    let pad = egui::vec2(14.0, 10.0);
    let actions_h = if has_actions { btn_h + 8.0 } else { 0.0 };
    let extra_w = if secondary_label.is_some() { 72.0 } else { 0.0 };
    egui::vec2(
        (text_w + pad.x * 2.0 + dismiss_reserve + extra_w).max(theme.toast_min_width()),
        text_h + pad.y * 2.0 + actions_h,
    )
}

/// 在当前 `Ui` 原点绘制 Toast(标题 + 正文；级别底色)。
pub(crate) fn paint_status_toast(
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    text: &str,
    kind: crate::ui::app::ToastKind,
    action_label: Option<&str>,
    secondary_label: Option<&str>,
    show_dismiss: bool,
) -> StatusToastActions {
    if title.is_empty() && text.is_empty() {
        return StatusToastActions::default();
    }
    let accent = toast_kind_accent(theme, kind);
    let fill = theme.toast_fill(accent);
    let stroke = theme.toast_stroke_color(accent);
    let title_fg = theme.toast_title_color(accent);
    let body_fg = theme.color_caption_text();
    let title_font = egui::FontId::proportional(theme.font_size_ui_control());
    let body_font = egui::FontId::proportional(theme.font_size_caption());
    let max_text_w = theme.toast_max_text_width();
    let btn_h = theme.toast_action_btn_h();
    let has_actions = action_label.is_some() || secondary_label.is_some();
    let dismiss_only = show_dismiss && !has_actions;
    let dismiss_reserve = if dismiss_only { 22.0 } else { 0.0 };
    let title_galley = if title.is_empty() {
        None
    } else {
        Some(ui.painter().layout(title.to_owned(), title_font, title_fg, max_text_w))
    };
    let body_galley = if text.is_empty() {
        None
    } else {
        Some(ui.painter().layout(text.to_owned(), body_font, body_fg, max_text_w))
    };
    let gap = if title_galley.is_some() && body_galley.is_some() {
        theme.toast_title_body_gap()
    } else {
        0.0
    };
    let text_w = title_galley
        .as_ref()
        .map(|g| g.size().x)
        .unwrap_or(0.0)
        .max(body_galley.as_ref().map(|g| g.size().x).unwrap_or(0.0));
    let text_h = title_galley.as_ref().map(|g| g.size().y).unwrap_or(0.0)
        + gap
        + body_galley.as_ref().map(|g| g.size().y).unwrap_or(0.0);
    let pad = egui::vec2(14.0, 10.0);
    let actions_h = if has_actions { btn_h + 8.0 } else { 0.0 };
    let extra_w = if secondary_label.is_some() { 72.0 } else { 0.0 };
    let size = egui::vec2(
        (text_w + pad.x * 2.0 + dismiss_reserve + extra_w).max(theme.toast_min_width()),
        text_h + pad.y * 2.0 + actions_h,
    );
    let (rect, _) = ui.allocate_exact_size(size, Sense::click());
    let primary = std::cell::Cell::new(false);
    let secondary = std::cell::Cell::new(false);
    let dismiss = std::cell::Cell::new(false);
    let painter = ui.painter();
    painter.rect(
        rect,
        theme.radius_list_item(),
        fill,
        egui::Stroke::new(1.0, stroke),
        egui::StrokeKind::Middle,
    );
    let mut text_pos = rect.min + pad;
    if let Some(g) = title_galley {
        let h = g.size().y;
        painter.galley(text_pos, g, egui::Color32::WHITE);
        text_pos.y += h + gap;
    }
    if let Some(g) = body_galley {
        painter.galley(text_pos, g, egui::Color32::WHITE);
    }

    if dismiss_only {
        let close_rect = egui::Rect::from_min_size(
            egui::pos2(rect.max.x - pad.x - 18.0, rect.min.y + pad.y - 2.0),
            egui::vec2(18.0, 18.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(close_rect), |ui| {
            if icon_button(ui, theme, IconId::Close, theme.color_caption_text())
                .on_hover_text(crate::i18n::tr(ui.ctx(), "Dismiss", "关闭"))
                .clicked()
            {
                dismiss.set(true);
            }
        });
    } else if has_actions {
        let row_y = rect.max.y - pad.y - btn_h;
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
                egui::pos2(rect.min.x + pad.x, row_y),
                egui::pos2(rect.max.x - pad.x, rect.max.y - pad.y),
            )),
            |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if show_dismiss
                        && icon_button(ui, theme, IconId::Close, theme.color_caption_text())
                            .on_hover_text(crate::i18n::tr(ui.ctx(), "Dismiss", "关闭"))
                            .clicked()
                    {
                        dismiss.set(true);
                    }
                    if let Some(label) = action_label {
                        ui.add_space(6.0);
                        if chrome_small_accent_button(ui, theme, label).clicked() {
                            primary.set(true);
                        }
                    }
                    if let Some(label) = secondary_label {
                        ui.add_space(6.0);
                        if panel_outlined_toolbar_button_with_icon_ex(
                            ui,
                            theme,
                            IconId::Fragment,
                            label,
                            true,
                        )
                        .clicked()
                        {
                            secondary.set(true);
                        }
                    }
                });
            },
        );
    }
    StatusToastActions {
        primary: primary.get(),
        secondary: secondary.get(),
        dismiss: dismiss.get(),
    }
}

/// 状态栏工具按钮：图标 + 短标签(比纯图标更易识别)。
pub fn status_tool_button(
    ui: &mut Ui,
    theme: &Theme,
    id: IconId,
    label: &str,
    tooltip: &str,
) -> Response {
    let bar_h = status_bar_content_height(theme);
    let icon_px = theme.size_icon_glyph().max(18.0);
    let font = egui::FontId::proportional(theme.font_size_status_bar());
    let idle = theme.color_toolbar_glyph_idle();
    let text_w = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), idle)
        .size()
        .x;
    let pad_x = 6.0;
    let w = pad_x + icon_px + 4.0 + text_w + pad_x;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(w, bar_h), Sense::click());
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    let color = if hovered || pressed {
        theme.color_toolbar_glyph_hover()
    } else {
        idle
    };
    if hovered || pressed {
        ui.painter().rect_filled(
            rect,
            theme.radius_list_item(),
            theme.accent_alpha(if pressed { 45 } else { 25 }),
        );
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    paint_icon_caption_row_in_rect(
        ui,
        rect,
        id,
        label,
        icon_px,
        4.0,
        theme.font_size_status_bar(),
        color,
        color,
        pad_x,
        false,
    );
    response.on_hover_text(tooltip)
}


/// 状态栏带小图标的文字 chip(如自动重连)
pub fn status_icon_chip(ui: &mut Ui, theme: &Theme, id: IconId, text: &str) {
    theme.frame_status_chip().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let px = theme.font_size_status_bar();
            let (r, _) = ui.allocate_exact_size(egui::vec2(px, px), egui::Sense::hover());
            icons::paint_icon(ui, r, id, theme.color_caption_text(), px);
            ui.label(
                RichText::new(text)
                    .size(theme.font_size_status_bar())
                    .color(theme.text_primary()),
            );
        });
    });
}

