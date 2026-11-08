//! 弹窗框架与标题行：modal window/frame、标题带、? 帮助（#398 自 chrome.rs 拆出）。
//! 纯移动，零行为变更。

use crate::ui::icons::{self, IconId};
use crate::ui::theme::Theme;
use eframe::egui::{
    self, Button, Color32, CursorIcon, Painter, Response, RichText, Sense, Stroke, Ui, Widget,
};
use super::*;

/// 弹窗标题行(标题 + 右侧 ? 帮助 + ×)。说明文字走悬停提示，正文不再铺横幅。
pub fn modal_header_with_help(
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    help_tip: &str,
) -> bool {
    let mx = theme.spacing_modal_content_x();
    let my = theme.spacing_modal_content_y();
    let mut closed = false;
    let band = theme
        .frame_modal_title_band()
        .outer_margin(egui::Margin {
            left: -mx as i8,
            right: -mx as i8,
            top: -my as i8,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(rich_modal_title(theme, title));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if close_icon_button(ui, theme).clicked() {
                        closed = true;
                    }
                    help_icon_button(ui, theme, help_tip);
                });
            });
        });
    paint_modal_header_bottom_divider(ui, theme, band.response.rect);
    ui.add_space(theme.spacing_modal_header_after_sep());
    closed
}


pub fn modal_window_frame(theme: &Theme, ctx: &egui::Context) -> egui::Frame {
    theme.frame_modal_window(ctx)
}


pub fn modal_content_frame(theme: &Theme) -> egui::Frame {
    theme.frame_modal_content()
}


/// 面板标题字号(侧栏 / dock / 弹窗统一)
pub fn modal_title_font_size(theme: &Theme) -> f32 {
    theme.font_size_panel_header_title()
}


/// 标准弹窗 `Window`：无系统标题栏、不可折叠、统一外框(须再 `.open()` / `.show()` / 尺寸)。
/// 外框描边来自 [`Theme::modal_stroke`]：暗夜面板线宽为 0，弹窗用 1 物理像素发丝线，避免叠在同色窗口上没有边。
/// 统一弹窗入口：去掉默认 title_bar / collapse，应用 [`modal_window_frame`]，
/// 并把约束放宽到整屏(`ctx.content_rect()`)，否则默认 `constrain(true)` 会把弹窗夹在
/// `ctx.available_rect()` 内，右 dock 打开后无法把弹窗拖到 dock 上方。
///
/// 首次居中请用 [`layout_util::modal_center_pos`] + `.default_pos(...)`，勿 `.anchor(...)`(拖拽会弹回)。
///
/// 尺寸策略：本函数**不**设置 `resizable`，由各调用点显式决定。
/// - 固定尺寸弹窗：一律 `.resizable(false).fixed_size(sz)`，不得开 resize，否则内容会把宽/高
///   钉回原值，出现「看起来能拖但拖不动」的假手柄。
/// - 允许拖动缩放的弹窗：内容必须尺寸弹性(不要 `set_min_width`/`set_min_size` 钉死，
///   滚动区不要用 `available_height` 反向撑满窗口)。
pub fn modal_window<'a>(
    window_id: &'a str,
    theme: &Theme,
    ctx: &egui::Context,
) -> egui::Window<'a> {
    egui::Window::new(window_id)
        .title_bar(false)
        .collapsible(false)
        .frame(modal_window_frame(theme, ctx))
        // egui 0.27 系列方法名是 `constraint_to`(拼写问题，但 API 就是这样)。
        .constrain_to(ctx.content_rect())
}


/// 将刚绘制的弹窗提到最前，避免被右 dock Foreground 盖住或误点底层关闭钮。
pub fn raise_window_response(ctx: &egui::Context, response: &egui::Response) {
    ctx.move_to_top(response.layer_id);
}


/// 弹窗标题行(标题 + 右上角 ×)。与 [`modal_header`] 相同；保留此名以免旧调用点遗漏。
pub fn modal_header_title_only(ui: &mut Ui, theme: &Theme, title: &str, title_px: f32) -> bool {
    modal_header(ui, theme, title, title_px)
}


/// 弹窗标题行(标题文字 + 右侧 ×)。#216：去掉左侧无语义「+」装饰，不换成其它凑数图标。
pub fn modal_header(ui: &mut Ui, theme: &Theme, title: &str, title_px: f32) -> bool {
    let _ = title_px;
    let mx = theme.spacing_modal_content_x();
    let my = theme.spacing_modal_content_y();
    let mut closed = false;
    let band = theme
        .frame_modal_title_band()
        .outer_margin(egui::Margin {
            left: -mx as i8,
            right: -mx as i8,
            top: -my as i8,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // 标题文字可点选区域留给 egui Window 拖移；不要再放无语义图标。
                ui.label(rich_modal_title(theme, title));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if close_icon_button(ui, theme).clicked() {
                        closed = true;
                    }
                });
            });
        });
    paint_modal_header_bottom_divider(ui, theme, band.response.rect);
    ui.add_space(theme.spacing_modal_header_after_sep());
    closed
}


/// 弹窗标题行(标题 + 右侧 ×，无前置图标)。
/// 用于「帮助」等无需通用加号图标、避免标题前出现误导读致的弹窗。
pub fn modal_header_plain(ui: &mut Ui, theme: &Theme, title: &str) -> bool {
    let mx = theme.spacing_modal_content_x();
    let my = theme.spacing_modal_content_y();
    let mut closed = false;
    let band = theme
        .frame_modal_title_band()
        .outer_margin(egui::Margin {
            left: -mx as i8,
            right: -mx as i8,
            top: -my as i8,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(rich_panel_header_title(theme, title));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if close_icon_button(ui, theme).clicked() {
                        closed = true;
                    }
                });
            });
        });
    paint_modal_header_bottom_divider(ui, theme, band.response.rect);
    ui.add_space(theme.spacing_modal_header_after_sep());
    closed
}


pub(crate) fn paint_modal_header_bottom_divider(ui: &mut Ui, theme: &Theme, band: egui::Rect) {
    let hairline = theme.hairline_width(ui.ctx());
    let y = theme.snap_y_to_pixel(ui.ctx(), band.bottom() - hairline * 0.5);
    ui.painter().hline(
        band.x_range(),
        y,
        egui::Stroke::new(hairline, theme.color_modal_header_divider()),
    );
}

