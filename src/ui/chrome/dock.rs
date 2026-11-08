//! 右侧 Dock：槽位框、前景层、拖拽把手、SSH 门禁（#398 自 chrome.rs 拆出）。
//! 纯移动，零行为变更。

use crate::ui::icons::{self, IconId};
use crate::ui::theme::Theme;
use eframe::egui::{
    self, Button, Color32, CursorIcon, Painter, Response, RichText, Sense, Stroke, Ui, Widget,
};
use super::*;

/// 右 dock 标题栏关闭(与 [`close_icon_button_with_tooltip`] 相同尺寸；`tooltip` 仅设置一次，避免叠两条提示)
pub fn dock_close_icon_button(ui: &mut Ui, theme: &Theme, tooltip: &str) -> Response {
    close_icon_button_with_tooltip(ui, theme, tooltip)
}


/// 右 dock 标题行与正文之间的分隔(modern：留白 + 极淡发丝线)
pub fn right_dock_header_divider(ui: &mut Ui, theme: &Theme) {
    let bleed = theme.spacing_right_dock_pad_x();
    let w = ui.available_width().max(1.0);
    if theme.uses_modern_palette() {
        ui.add_space(theme.spacing_xs());
        let hairline = theme.hairline_width(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, hairline), egui::Sense::hover());
        ui.painter().hline(
            (rect.min.x - bleed)..=(rect.max.x + bleed),
            theme.snap_y_to_pixel(ui.ctx(), rect.center().y),
            egui::Stroke::new(hairline, theme.color_panel_header_divider()),
        );
        return;
    }
    let bleed = theme.spacing_right_dock_pad_x();
    let w = ui.available_width().max(1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), egui::Sense::hover());
    ui.painter().hline(
        (rect.min.x - bleed)..=(rect.max.x + bleed),
        theme.snap_y_to_pixel(ui.ctx(), rect.center().y),
        egui::Stroke::new(
            theme.hairline_width(ui.ctx()),
            theme.color_dock_header_divider(),
        ),
    );
}


/// 右 dock 左侧让出的 `bg_body` 缝(单 dock 与终端之间、多 dock 之间都看得见)。
pub(crate) fn right_dock_outer_margin(theme: &Theme) -> egui::Margin {
    let mut m = theme.margin_right_dock_screen_outer();
    m.left = theme.spacing_dock_gap() as i8;
    m
}


/// 右 `SidePanel` 占位槽(透明，屏右缘留 `bg_body` 缝)。
pub fn right_dock_placeholder_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::NONE.outer_margin(right_dock_outer_margin(theme))
}


/// 右 `SidePanel` 可见外框(SFTP / 凭证等直绘 dock)。
pub fn right_dock_panel_frame(theme: &Theme) -> egui::Frame {
    theme
        .frame_region_panel_flush_bottom()
        .outer_margin(right_dock_outer_margin(theme))
}


/// 在右 dock 槽位(含左侧 `spacing_dock_gap` 缝)铺 `bg_body`；须用 [`side_panel_place_slot`] 后的矩形。
pub fn paint_right_dock_slot_gap(ctx: &egui::Context, theme: &Theme, slot: egui::Rect) {
    let gap = theme.spacing_dock_gap().max(0.0);
    let bg = egui::Rect::from_min_max(
        egui::pos2(slot.min.x - gap, slot.min.y),
        egui::pos2(slot.max.x, slot.max.y),
    );
    if !bg.is_positive() {
        return;
    }
    let layer_id = egui::LayerId::new(
        egui::Order::Background,
        egui::Id::unique("mistterm_right_dock_slot_bg"),
    );
    let painter = ctx.layer_painter(layer_id);
    painter.rect_filled(bg, 0.0, theme.bg_body_color());
    painter.vline(bg.min.x + 0.5, bg.y_range(), theme.divider_stroke());
}


/// SidePanel 回调内(旧路径)：勿再使用；在 `.show` 之后调 [`paint_right_dock_slot_gap`].
pub fn paint_right_dock_left_gap(ui: &egui::Ui, theme: &Theme) {
    let gap = theme.spacing_dock_gap().max(0.0);
    let inner = ui.max_rect();
    let bg = egui::Rect::from_min_max(
        egui::pos2(inner.min.x - gap, inner.min.y),
        egui::pos2(inner.max.x, inner.max.y),
    );
    if !bg.is_positive() {
        return;
    }
    let layer_id = egui::LayerId::new(
        egui::Order::Background,
        egui::Id::unique("mistterm_right_dock_slot_bg"),
    );
    let painter = ui.ctx().layer_painter(layer_id);
    painter.rect_filled(bg, 0.0, theme.bg_body_color());
    // 缝左侧 1px 分隔线(终端/相邻 dock 与当前 dock 之间)
    painter.vline(bg.min.x + 0.5, bg.y_range(), theme.divider_stroke());
}


/// 右 dock `outer_margin` 与窗口右缘之间的竖条(须铺 `bg_body`，否则会露系统/窗口黑底)。
pub fn paint_right_dock_screen_gutter(ctx: &egui::Context, theme: &Theme, top_chrome_height: f32) {
    let inset = theme.spacing_right_dock_screen_inset();
    if inset < 0.5 || !inset.is_finite() {
        return;
    }
    let screen = ctx.content_rect();
    let y0 = screen.min.y + top_chrome_height.max(0.0);
    let y1 = screen.max.y - theme.status_bar_height();
    if y1 <= y0 {
        return;
    }
    let x0 = (screen.max.x - inset).max(screen.min.x);
    if x0 >= screen.max.x {
        return;
    }
    let gutter = egui::Rect::from_min_max(egui::pos2(x0, y0), screen.max);
    let layer_id = egui::LayerId::new(
        egui::Order::Background,
        egui::Id::unique("mistterm_right_dock_gutter"),
    );
    ctx.layer_painter(layer_id)
        .rect_filled(gutter, 0.0, theme.bg_body_color());
}


/// 左缘略向左扩 2px，盖住 Central `bg_body` 可能压到侧栏左缘的细缝。
pub const RIGHT_DOCK_SHELL_LEFT_BLEED: f32 = 0.0;

/// 右 dock Foreground：先铺满整个槽位(`Frame` 仅包住内容时左侧会透出中央 `bg_body`)。
pub fn paint_right_dock_slot_shell(ui: &mut egui::Ui, slot: egui::Rect, theme: &Theme) {
    paint_right_dock_slot_shell_with_painter(ui.painter(), slot, theme);
}


/// 与 [`paint_right_dock_slot_shell`] 相同，用于在 `Area` 外先铺底色(避免可点层盖住整块槽位)。
/// 右 dock 壳层圆角：顶角不圆(与终端 Tab 条齐平)；贴底栏时底角也为 0。
pub fn right_dock_shell_rounding(theme: &Theme, flush_bottom: bool) -> egui::CornerRadius {
    if flush_bottom {
        egui::CornerRadius::ZERO
    } else {
        let r = theme.radius_panel();
        egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: r as u8,
            se: r as u8,
        }
    }
}


pub fn paint_right_dock_slot_shell_with_painter(
    painter: &Painter,
    slot: egui::Rect,
    theme: &Theme,
) {
    paint_right_dock_slot_shell_with_painter_ex(painter, slot, theme, false);
}


pub fn paint_right_dock_slot_shell_with_painter_ex(
    painter: &Painter,
    slot: egui::Rect,
    theme: &Theme,
    flush_bottom: bool,
) {
    let mut fill = slot;
    fill.min.x -= RIGHT_DOCK_SHELL_LEFT_BLEED;
    let rounding = right_dock_shell_rounding(theme, flush_bottom);
    painter.rect_filled(fill, rounding, theme.color_panel_surface());
    paint_region_panel_shell_border(painter, fill, theme, flush_bottom);
}


/// 槽位扣除 region panel 内边距后的内容矩形(须用 `Margin::shrink_rect`，勿 `shrink2(left+right)`)。
#[inline]
pub fn right_dock_slot_content_rect(slot: egui::Rect, theme: &Theme) -> egui::Rect {
    // #206 S1: Margin::shrink_rect removed in 0.37; shrink manually via leftf/topf etc.
    {
        let m = theme.right_dock_content_margin();
        egui::Rect::from_min_max(
            egui::pos2(slot.left() + m.leftf(), slot.top() + m.topf()),
            egui::pos2(slot.right() - m.rightf(), slot.bottom() - m.bottomf()),
        )
    }
}


/// Central 之后 Foreground 重绘右 dock 用的图层(仅绘制壳层，勿在此注册可点 `Area`)。
#[inline]
pub fn right_dock_foreground_layer_id(id: &'static str) -> egui::LayerId {
    egui::LayerId::new(egui::Order::Middle, egui::Id::unique(id))
}


/// 右 dock Foreground `Area`(可点层)；正文仍在 `inner` 子区域布局。
pub fn right_dock_foreground_body_area(id: &'static str) -> egui::Area {
    egui::Area::new(egui::Id::unique(id))
        .order(egui::Order::Middle)
        .movable(false)
        .interactable(true)
        .constrain(true)
}


/// Foreground 重绘几何：paint 槽位 + 扣除内边距后的正文区。
pub struct RightDockForegroundGeom {
    pub paint: egui::Rect,
    pub inner: egui::Rect,
}

/// 由 SidePanel 槽位计算 Foreground 绘制区(与 [`right_dock_slot_content_rect`] 一致)。
pub fn prepare_right_dock_foreground_geom(
    slot: egui::Rect,
    screen: egui::Rect,
    theme: &Theme,
) -> RightDockForegroundGeom {
    let inset = theme.spacing_right_dock_screen_inset();
    let status_h = theme.status_bar_height();
    const WORK_BOTTOM_GAP: f32 = 1.0;
    let mut slot = crate::ui::layout_util::clamp_rect_above_status_bar(slot, screen, status_h);
    // 顶部贴齐 top_chrome 下沿，避免出现 4px 黑条；底部仅留 1px 与状态栏接缝
    slot.max.y = (slot.max.y - WORK_BOTTOM_GAP).max(slot.min.y + 1.0);
    let paint = crate::ui::layout_util::clamp_rect_above_status_bar(
        crate::ui::layout_util::inset_slot_for_foreground_paint(slot, screen, inset),
        screen,
        status_h,
    );
    let inner = crate::ui::layout_util::clamp_rect_above_status_bar(
        right_dock_slot_content_rect(paint, theme),
        screen,
        status_h,
    );
    RightDockForegroundGeom { paint, inner }
}


/// 铺 Foreground 壳层与右边框(在 `Area` 之前用 `Painter` 调用)。
pub fn paint_right_dock_foreground_shell(
    ctx: &egui::Context,
    layer_id: egui::LayerId,
    paint: egui::Rect,
    theme: &Theme,
) {
    let painter = egui::Painter::new(ctx.clone(), layer_id, paint);
    paint_right_dock_slot_shell_with_painter_ex(&painter, paint, theme, true);
}


/// 标准 Foreground 正文宿主：`Area` 严格限制在 `paint` 内，避免可点区吞掉左侧栏。
pub fn show_right_dock_foreground_body<R>(
    area_id: &'static str,
    ctx: &egui::Context,
    _theme: &Theme,
    geom: &RightDockForegroundGeom,
    _profile: crate::ui::layout_util::SidePanelProfile,
    add_body: impl FnOnce(&mut Ui, f32) -> R,
) -> egui::InnerResponse<R> {
    let screen = ctx.content_rect();
    // 正文与壳层用同一块槽位。以前槽位一超过屏宽 55% 就把可点区钉回右侧 42%，
    // 壳层仍铺满拖宽后的区域，终端又被裁掉，中间就剩一块黑。
    let paint = geom.paint.intersect(screen);
    let inner = egui::Rect::from_min_max(
        egui::pos2(
            (geom.inner.min.x - geom.paint.min.x) + paint.min.x,
            (geom.inner.min.y - geom.paint.min.y) + paint.min.y,
        ),
        egui::pos2(
            (geom.inner.max.x - geom.paint.min.x) + paint.min.x,
            (geom.inner.max.y - geom.paint.min.y) + paint.min.y,
        ),
    )
    .intersect(paint);
    let paint_size = paint.size();
    let body_w = inner.width().max(48.0);
    right_dock_foreground_body_area(area_id)
        .constrain_to(paint)
        .fixed_pos(paint.min)
        .show(ctx, |ui| {
            ui.set_min_size(paint_size);
            ui.set_max_size(paint_size);
            ui.set_clip_rect(paint);
            let local_inner = egui::Rect::from_min_max(
                ui.min_rect().min + (inner.min.to_vec2() - paint.min.to_vec2()),
                ui.min_rect().min + (inner.max.to_vec2() - paint.min.to_vec2()),
            );
            let mut body_ui =
                ui.new_child(egui::UiBuilder::new().max_rect(local_inner).layout(egui::Layout::top_down(egui::Align::Min)));
            body_ui.set_clip_rect(local_inner);
            let cap = body_ui.available_width().max(48.0).min(body_w);
            let w = crate::ui::layout_util::constrain_ui_to_right_dock_body(&mut body_ui, cap);
            add_body(&mut body_ui, w)
        })
}


/// 改宽手柄：独立 `Foreground` 层，须在**全部** dock 正文绘完后、按屏上左→右顺序调用(右邻正文会盖住左缝)。
pub fn show_right_dock_resize_grip_layer(
    ctx: &egui::Context,
    theme: &Theme,
    area_id: &'static str,
    geom: &RightDockForegroundGeom,
) {
    let Some(panel_id) = right_dock_panel_id_for_foreground(area_id) else {
        return;
    };
    let gap = theme.spacing_dock_gap().max(4.0);
    let gutter = egui::Rect::from_min_max(
        egui::pos2(geom.paint.min.x - gap, geom.paint.min.y),
        egui::pos2(
            (geom.paint.min.x + 4.0).min(geom.paint.max.x),
            geom.paint.max.y,
        ),
    );
    if !gutter.is_positive() {
        return;
    }
    egui::Area::new(egui::Id::unique((area_id, "resize_grip_layer")))
        .order(egui::Order::Foreground)
        .movable(false)
        .interactable(true)
        .fixed_pos(gutter.min)
        .show(ctx, |ui| {
            ui.set_min_size(gutter.size());
            ui.set_max_size(gutter.size());
            let response = ui.interact(gutter, ui.scope_id(), egui::Sense::drag());
            if response.hovered() || response.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                let stroke = if response.dragged() {
                    egui::Stroke::new(1.0, theme.accent_color())
                } else {
                    theme.divider_stroke()
                };
                let x = theme.snap_x_to_pixel(ctx, geom.paint.min.x - gap * 0.5);
                ui.painter().vline(x, gutter.y_range(), stroke);
            }
            if response.dragged() {
                // #287: PanelState id is parent_scope.with(salt), not Id::unique(salt).
                let id = crate::ui::layout_util::side_panel_persisted_id(ctx, panel_id);
                if let Some(mut state) = egui::containers::panel::PanelState::load(ctx, id) {
                    let dx = ui.input(|i| i.pointer.delta().x);
                    let current_w = state.outer_rect.width();
                    let screen_w = ctx.content_rect().width();
                    let (_, min_w, max_w) =
                        crate::ui::layout_util::right_dock_resize_bounds_for_screen(
                            current_w,
                            Some(screen_w),
                        );
                    let new_w = (current_w - dx).clamp(min_w, max_w);
                    state.outer_rect.min.x = state.outer_rect.max.x - new_w;
                    ctx.data_mut(|d| d.insert_persisted(id, state));
                    ctx.request_repaint();
                }
            }
        });
}


/// 由 SidePanel 槽位绘制改宽手柄(各 dock 在 workspace 统一 pass 里调用)。
pub fn show_right_dock_resize_grip_for_slot(
    ctx: &egui::Context,
    theme: &Theme,
    area_id: &'static str,
    panel_slot: Option<egui::Rect>,
    panel_id: &str,
    profile: crate::ui::layout_util::SidePanelProfile,
) {
    let screen = ctx.content_rect();
    let dock_inset = theme.spacing_right_dock_screen_inset();
    let Some(slot) = crate::ui::layout_util::right_dock_foreground_slot(
        panel_slot, ctx, panel_id, profile, None, dock_inset,
    ) else {
        return;
    };
    let geom = prepare_right_dock_foreground_geom(slot, screen, theme);
    show_right_dock_resize_grip_layer(ctx, theme, area_id, &geom);
}


pub(crate) fn right_dock_panel_id_for_foreground(area_id: &'static str) -> Option<&'static str> {
    match area_id {
        "mistterm_ai_fg" => Some(crate::ui::layout_util::AI_PANEL_ID),
        "mistterm_monitor_fg" => Some(crate::ui::layout_util::MONITOR_PANEL_ID),
        "mistterm_fragment_fg" => Some(crate::ui::layout_util::FRAGMENT_PANEL_ID),
        "mistterm_sftp_fg" => Some("sftp_browser_panel"),
        "mistterm_port_fwd_fg" => Some("port_forward_panel"),
        "mistterm_credential_fg" => Some("credential_panel"),
        "mistterm_cloud_sync_fg" => Some("cloud_sync_panel"),
        _ => None,
    }
}


/// 右 dock 内「左标签 + 右数值」行(宽度随父级 `available_width`)。
pub fn dock_label_value_row(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    value: impl std::fmt::Display,
) {
    let px = theme.font_size_medium();
    let _ = crate::ui::layout_util::set_width_to_available(ui);
    ui.horizontal(|ui| {
        let row_w = ui.available_width();
        if row_w.is_finite() && row_w > 1.0 {
            ui.set_max_width(row_w);
        }
        crate::ui::icons::icon_label_row(ui, icon, label, px, 6.0, |t| {
            t.size(px).color(theme.text_secondary())
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(value.to_string())
                    .monospace()
                    .size(px)
                    .color(theme.text_primary()),
            );
        });
    });
}


/// 右 dock 大标题 + 左侧图标(与 [`panel_header_title_leading`] 一致)
pub fn dock_title_row(ui: &mut Ui, theme: &Theme, icon: IconId, title: &str) {
    panel_header_title_leading(ui, theme, icon, title);
}


/// 右 dock / 侧栏统一标题行：左侧标题区(可截断)，右侧 RTL 操作区
pub fn dock_panel_title_row(
    ui: &mut Ui,
    theme: &Theme,
    mut draw_title: impl FnMut(&mut Ui),
    _close_tooltip: &str,
    trailing_width: f32,
    mut draw_trailing: impl FnMut(&mut Ui, &Theme) -> bool,
) -> bool {
    let mut closed = false;
    let row_gap = theme.spacing_panel_gap();
    ui.horizontal(|ui| {
        let total_w = ui.available_width();
        ui.scope(|ui| {
            ui.set_max_width((total_w - trailing_width - row_gap).max(0.0));
            draw_title(ui);
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.set_min_width(trailing_width);
            ui.spacing_mut().item_spacing.x = theme.spacing_panel_gap();
            closed = draw_trailing(ui, theme);
        });
    });
    closed
}


pub(crate) fn dock_panel_title_close_trailing(ui: &mut Ui, theme: &Theme, close_tooltip: &str) -> bool {
    dock_close_icon_button(ui, theme, close_tooltip).clicked()
}


/// 右 dock 标题行内容区(固定高度，与终端 Tab 条 [`Theme::size_panel_header_row_h`] 对齐)
pub fn dock_header_horizontal<R>(
    ui: &mut Ui,
    theme: &Theme,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let row_h = theme.size_panel_header_row_h();
    let row_w = ui.available_width().max(1.0);
    ui.allocate_ui_with_layout(
        egui::vec2(row_w, row_h),
        egui::Layout::left_to_right(egui::Align::Center),
        add_contents,
    )
    .inner
}


/// 仅标题 + 关闭 ×(右侧仅一个图标按钮，避免与 dock 工具栏混排重复)
pub fn dock_panel_title_close_only(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    title: &str,
    close_tooltip: &str,
) -> bool {
    let _ = close_tooltip;
    let mut closed = false;
    dock_header_horizontal(ui, theme, |ui| {
        panel_header_title_leading(ui, theme, icon, title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(theme.spacing_dock_panel_trailing_pad());
            if dock_close_icon_button(ui, theme, close_tooltip).clicked() {
                closed = true;
            }
        });
    });
    closed
}


/// 仅标题 + 右侧 ? 帮助 + 关闭 ×：把右 dock 的一次性说明收进悬停提示。
pub fn dock_panel_title_close_with_help(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    title: &str,
    close_tooltip: &str,
    help_tip: &str,
) -> bool {
    let mut closed = false;
    dock_header_horizontal(ui, theme, |ui| {
        panel_header_title_leading(ui, theme, icon, title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(theme.spacing_dock_panel_trailing_pad());
            if dock_close_icon_button(ui, theme, close_tooltip).clicked() {
                closed = true;
            }
            help_icon_button(ui, theme, help_tip);
        });
    });
    closed
}


/// 右 dock 标题行：标题 + 主操作 + 关闭
pub struct DockPanelHeaderActions {
    pub closed: bool,
    pub new_fragment: bool,
}

pub fn dock_panel_title_bar(
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    _title_color: Color32,
    new_tooltip: &str,
    close_tooltip: &str,
) -> DockPanelHeaderActions {
    let mut out = DockPanelHeaderActions {
        closed: false,
        new_fragment: false,
    };
    dock_header_horizontal(ui, theme, |ui| {
        panel_header_title_leading(ui, theme, IconId::Fragment, title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(theme.spacing_dock_panel_trailing_pad());
            ui.spacing_mut().item_spacing.x = theme.spacing_tool_btn_gap();
            if dock_panel_title_close_trailing(ui, theme, close_tooltip) {
                out.closed = true;
            }
            let new_label = crate::i18n::tr(ui.ctx(), "New", "新建");
            if panel_header_new_button_with_label(ui, theme, new_label)
                .on_hover_text(new_tooltip)
                .clicked()
            {
                out.new_fragment = true;
            }
        });
    });
    out
}


/// #350：右 dock 在无 SSH / 本机标签时的可行动空态动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightDockSshGateAction {
    #[default]
    None,
    OpenExistingConnection,
    NewSshSession,
}

/// 右 dock SSH 门闩：`(ready, action)`。`ready == true` 表示可继续绘制面板正文。
pub fn show_right_dock_ssh_gate(
    ui: &mut Ui,
    theme: &Theme,
    ctx: &egui::Context,
    terminal: Option<&crate::ui::terminal::TerminalView>,
    no_session_en: &'static str,
    no_session_zh: &'static str,
) -> (bool, RightDockSshGateAction) {
    use crate::ui::terminal::RightDockSshGate;
    let Some(t) = terminal else {
        let action = paint_right_dock_ssh_needed_empty(
            ui,
            theme,
            ctx,
            no_session_en,
            no_session_zh,
        );
        return (false, action);
    };
    match t.right_dock_ssh_gate() {
        RightDockSshGate::Ready => (true, RightDockSshGateAction::None),
        RightDockSshGate::Connecting => {
            busy_row(
                ui,
                theme,
                crate::i18n::tr(ctx, "Connecting…", "连接建立中…"),
            );
            (false, RightDockSshGateAction::None)
        }
        RightDockSshGate::Disconnected => {
            ui.label(
                RichText::new(crate::i18n::tr(
                    ctx,
                    "Remote session disconnected. Reconnect to use this panel.",
                    "远端会话已断开。请重新连接后再使用此面板。",
                ))
                .color(theme.amber_color()),
            );
            (false, RightDockSshGateAction::None)
        }
        RightDockSshGate::Failed(err) => {
            ui.label(
                RichText::new(format!(
                    "{} {}",
                    crate::i18n::tr(ctx, "Connection failed:", "连接失败："),
                    err
                ))
                .color(theme.red_color()),
            );
            (false, RightDockSshGateAction::None)
        }
        RightDockSshGate::NoSession => {
            let action = paint_right_dock_ssh_needed_empty(
                ui,
                theme,
                ctx,
                no_session_en,
                no_session_zh,
            );
            (false, action)
        }
        RightDockSshGate::LocalShell => {
            // #350：本机标签不假装挂载远端；给出下一步 CTA。
            let action = paint_right_dock_ssh_needed_empty(
                ui,
                theme,
                ctx,
                "This is a local terminal tab. Remote files and port forwarding need a remote SSH session.",
                "当前是本机终端标签。远端文件和端口转发需要先打开一台远端主机会话。",
            );
            (false, action)
        }
    }
}


pub(crate) fn paint_right_dock_ssh_needed_empty(
    ui: &mut Ui,
    theme: &Theme,
    ctx: &egui::Context,
    reason_en: &'static str,
    reason_zh: &'static str,
) -> RightDockSshGateAction {
    ui.label(
        RichText::new(crate::i18n::tr(ctx, reason_en, reason_zh))
            .size(theme.font_size_body())
            .color(theme.text_secondary()),
    );
    ui.add_space(theme.spacing_sm());
    ui.label(
        RichText::new(crate::i18n::tr(
            ctx,
            "Pick a next step:",
            "可以下一步：",
        ))
        .size(theme.font_size_caption())
        .color(theme.text_tertiary()),
    );
    ui.add_space(theme.spacing_xs());
    let mut action = RightDockSshGateAction::None;
    let row_w = ui.available_width().max(120.0);
    if panel_action_primary_button_with_icon_ex_min_w(
        ui,
        theme,
        crate::ui::icons::IconId::Server,
        crate::i18n::tr(ctx, "Open existing SSH…", "打开已有 SSH 连接…"),
        true,
        row_w,
    )
    .clicked()
    {
        action = RightDockSshGateAction::OpenExistingConnection;
    }
    ui.add_space(theme.spacing_xs());
    if panel_action_button_ex(
        ui,
        theme,
        crate::i18n::tr(ctx, "New SSH session…", "新建 SSH 会话…"),
        true,
    )
    .clicked()
    {
        action = RightDockSshGateAction::NewSshSession;
    }
    action
}

