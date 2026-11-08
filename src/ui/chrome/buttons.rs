//! 各类按钮：图标钮、工具条钮、面板动作钮、主次/实心/描边（#398 自 chrome.rs 拆出）。
//! 纯移动，零行为变更。

use crate::ui::icons::{self, IconId};
use crate::ui::theme::Theme;
use eframe::egui::{
    self, Button, Color32, CursorIcon, Painter, Response, RichText, Sense, Stroke, Ui, Widget,
};
use super::*;

pub(crate) fn theme_icon_hit(
    ui: &mut Ui,
    theme: &Theme,
    id: IconId,
    hit: f32,
    icon_px: f32,
    idle: Color32,
    hover: Color32,
) -> Response {
    icons::icon_hit_button(
        ui,
        id,
        hit,
        icon_px,
        idle,
        hover,
        theme.color_tab_bar_icon_btn_hover_fill(),
        theme.accent_alpha(45),
        theme.radius_list_item(),
    )
}


pub(crate) fn theme_icon_hit_revealed(
    ui: &mut Ui,
    theme: &Theme,
    id: IconId,
    hit: f32,
    icon_px: f32,
    idle: Color32,
    hover: Color32,
    revealed: bool,
) -> Response {
    icons::icon_hit_button_revealed(
        ui,
        id,
        hit,
        icon_px,
        idle,
        hover,
        theme.color_tab_bar_icon_btn_hover_fill(),
        theme.accent_alpha(45),
        theme.radius_list_item(),
        revealed,
    )
}


/// 通用图标按钮(可指定 idle 色)
pub fn icon_button(ui: &mut Ui, theme: &Theme, id: IconId, color: Color32) -> Response {
    theme_icon_hit(
        ui,
        theme,
        id,
        theme.size_panel_header_control_h(),
        theme.size_icon_glyph(),
        color,
        theme.text_primary(),
    )
}


/// 标题栏 / 右 dock 关闭 ×(28px 点击区 + 悬停底，与 Tab 栏 × 同级)
pub fn close_icon_button_with_tooltip(ui: &mut Ui, theme: &Theme, tooltip: &str) -> Response {
    theme_icon_hit(
        ui,
        theme,
        IconId::Close,
        theme.size_panel_header_control_h(),
        theme.size_icon_glyph(),
        theme.color_sidebar_header_icon(),
        theme.text_primary(),
    )
    .on_hover_text(tooltip)
}


/// 弹窗 / 侧栏标题栏关闭(默认提示「关闭」)
pub fn close_icon_button(ui: &mut Ui, theme: &Theme) -> Response {
    close_icon_button_with_tooltip(ui, theme, crate::i18n::tr(ui.ctx(), "Close", "关闭"))
}


/// 标题行「?」帮助按钮：与 × 同级点击区，悬停出说明，不占正文版面。
/// 用于把「一次性说明横幅」收进提示里（横幅要占一整块 + 还要「不再显示」「知道了」）。
pub fn help_icon_button(ui: &mut Ui, theme: &Theme, tip: &str) -> Response {
    let hit = theme.size_panel_header_control_h();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(hit, hit), Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        ui.painter().rect_filled(
            rect,
            theme.radius_list_item(),
            theme.color_tab_bar_icon_btn_hover_fill(),
        );
    }
    let color = if hovered {
        theme.text_primary()
    } else {
        theme.color_sidebar_header_icon()
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "?",
        egui::FontId::proportional(theme.size_icon_glyph()),
        color,
    );
    if tip.is_empty() {
        response
    } else {
        response.on_hover_text(tip)
    }
}


/// 侧栏标题行方形图标按钮(与排序下拉同高)。
pub fn sidebar_header_icon_button(
    ui: &mut Ui,
    theme: &Theme,
    id: IconId,
    color: Color32,
) -> Response {
    theme_icon_hit(
        ui,
        theme,
        id,
        theme.size_sidebar_header_icon(),
        theme.font_size_sidebar_icon_glyph(),
        color,
        theme.text_primary(),
    )
}


/// 连接栏标题「收起」：与右 dock 关闭钮同级点击区，避免 18px 弱色图标被挤没/点不着。
pub fn sidebar_collapse_button(ui: &mut Ui, theme: &Theme) -> Response {
    theme_icon_hit(
        ui,
        theme,
        IconId::SidebarCollapse,
        theme.size_panel_header_control_h(),
        theme.size_icon_glyph(),
        theme.color_caption_text(),
        theme.text_primary(),
    )
}


/// 面板标题栏「＋」新建(连接栏 / 命令片段库统一：实心 Primary，与 Tab/Rail 强调色同族)。
pub fn panel_header_new_button(ui: &mut Ui, theme: &Theme) -> Response {
    panel_header_new_button_with_label(ui, theme, "")
}


/// 带可见标签的新建按钮；`label` 为空时仅显示「＋」。
pub fn panel_header_new_button_with_label(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    panel_header_new_button_with_label_h(ui, theme, label, theme.size_control_btn_h())
}


/// 带可见标签的新建按钮，并把行高抬到 `min_h`。
///
/// 按钮标准行高(`size_control_btn_h`，modern 26) 比表单/搜索输入框行高
/// ([`Theme::size_form_field_h`]，28) 矮 2px；两者并排时会一高一低，用 `min_h`
/// 把按钮补到与输入框同高。
pub fn panel_header_new_button_with_label_h(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    min_h: f32,
) -> Response {
    if label.is_empty() {
        let h = theme
            .size_panel_header_control_h()
            .min(theme.size_panel_header_row_h());
        let size = egui::vec2(h, h);
        let rounding = theme.radius_list_item();
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        let hovered = response.hovered();
        let pressed = response.is_pointer_button_down_on();
        if hovered || pressed {
            ui.ctx().request_repaint();
        }
        let (fill, icon_color, _) = primary_control_button_colors(theme, true, hovered, pressed);
        ui.painter()
            .rect(rect, rounding, fill, egui::Stroke::NONE, egui::StrokeKind::Middle);
        icons::paint_icon(
            ui,
            rect,
            IconId::Plus,
            icon_color,
            theme.size_icon_glyph().min(h - 4.0),
        );
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        return response;
    }
    // 标题行主操作：实心 accent，勿用 ToolbarPrimary 的灰底描边(与整体蓝强调脱节)。
    let mut size = control_button_size(
        ui,
        theme,
        label,
        true,
        theme.size_panel_header_btn_min_w(),
    );
    size.y = size.y.max(min_h);
    paint_control_button_sized(
        ui,
        theme,
        label,
        Some(IconId::Plus),
        ControlButtonVariant::Primary,
        size,
        true,
    )
}


/// 「＋新建」按钮的实际占位宽度(供同行排版先算再排)。
///
/// 必须用它而不是 `size_panel_header_btn_min_w()`：后者只是下限(约 2 字宽 + 内边距)，
/// 远小于带标签按钮的真实宽度(图标 + 4 + 文字 + 2×pad_x)。按 min_w 扣减搜索框宽度，
///
/// 会让"搜索框 + 新建"这一行的最小内容宽大于面板宽，行被算宽后侧栏跟着变宽
/// (且每帧用新的 available 重新算，会逐帧继续变宽)。这里返回真实宽度，行刚好装满。
pub fn panel_header_new_button_width(ui: &Ui, theme: &Theme, label: &str) -> f32 {
    control_button_size(
        ui,
        theme,
        label,
        true,
        theme.size_panel_header_btn_min_w(),
    )
    .x
}


/// [`panel_header_new_button`] 别名(侧栏)
#[inline]
pub fn sidebar_new_session_button(ui: &mut Ui, theme: &Theme) -> Response {
    panel_header_new_button(ui, theme)
}


/// 排序芯片预估宽度(与 [`panel_sort_chip`] 一致)
pub fn panel_sort_chip_width(ui: &Ui, theme: &Theme, sort_label: &str) -> f32 {
    let icon_px = theme.size_icon_glyph();
    let pad = theme.spacing_panel_header_btn_pad_x();
    let font = egui::FontId::proportional(theme.font_size_category_label());
    let text_w = ui
        .painter()
        .layout_no_wrap(
            sort_label.to_owned(),
            font,
            theme.color_filter_chip_inactive_text(),
        )
        .size()
        .x;
    (icon_px + 4.0 + text_w + pad * 2.0).max(theme.size_panel_header_btn_min_w())
}


/// 排序芯片(与分类筛选同高；连接栏点开菜单、片段栏点击轮换)
pub fn panel_sort_chip(
    ui: &mut Ui,
    theme: &Theme,
    sort_icon: IconId,
    sort_label: &str,
    hover_text: &str,
) -> Response {
    let chip_h = theme.size_panel_filter_chip_h();
    let icon_px = theme.size_icon_glyph();
    let gap = 4.0;
    let pad_x = theme.spacing_panel_header_btn_pad_x();
    let w = panel_sort_chip_width(ui, theme, sort_label);
    let size = egui::vec2(w, chip_h);
    let rounding = theme.radius_category();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    if hovered || pressed {
        ui.ctx().request_repaint();
    }
    let fill = if theme.uses_modern_palette() {
        if pressed {
            theme.color_widget_active_fill()
        } else if hovered {
            theme.color_widget_hover_fill()
        } else {
            Color32::TRANSPARENT
        }
    } else if pressed {
        theme.accent_alpha(38)
    } else if hovered {
        theme.color_filter_chip_active_fill().gamma_multiply(0.45)
    } else {
        theme.color_overlay_fill_subtle()
    };
    let stroke = if theme.uses_modern_palette() || !(hovered || pressed) {
        egui::Stroke::NONE
    } else {
        egui::Stroke::new(1.0, theme.accent_alpha(51))
    };
    ui.painter().rect(rect, rounding, fill, stroke, egui::StrokeKind::Middle);
    let text_color = if theme.uses_modern_palette() {
        if hovered || pressed {
            theme.text_primary()
        } else {
            theme.text_secondary().gamma_multiply(0.72)
        }
    } else {
        theme.color_filter_chip_inactive_text()
    };
    paint_icon_caption_row_in_rect(
        ui,
        rect,
        sort_icon,
        sort_label,
        icon_px,
        gap,
        theme.font_size_category_label(),
        text_color,
        text_color,
        pad_x,
        false,
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response.on_hover_text(hover_text)
}


/// 小号图标按钮(终端搜索上/下条等)
pub fn chrome_small_icon_button(ui: &mut Ui, theme: &Theme, id: IconId) -> Response {
    theme_icon_hit(
        ui,
        theme,
        id,
        theme.size_panel_header_control_h(),
        theme.size_icon_glyph(),
        theme.color_modal_secondary_text(),
        theme.text_primary(),
    )
}


/// 异步加载行：旋转指示 + 文案(SFTP / 监控 / Vault 等复用)
pub fn busy_row(ui: &mut Ui, theme: &Theme, label: &str) {
    ui.horizontal(|ui| {
        ui.add_space(theme.spacing_sm());
        ui.add(egui::Spinner::new());
        ui.label(
            RichText::new(label)
                .size(theme.font_size_body())
                .color(theme.text_tertiary()),
        );
    });
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(120));
}


/// 小号文字按钮(替换 `small_button`，带悬停底)
pub fn chrome_small_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    text_hit_button(
        ui,
        theme,
        label,
        theme.font_size_panel_title(),
        theme.color_modal_secondary_text(),
        theme.text_primary(),
        egui::vec2(6.0, 3.0),
    )
}


/// 强调色小号文字按钮(如 SSH 导入条「导入」)
pub fn chrome_small_accent_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    text_hit_button(
        ui,
        theme,
        label,
        theme.font_size_panel_title(),
        theme.accent_color(),
        theme.color_modal_primary_fill_hover(),
        egui::vec2(8.0, 4.0),
    )
}


/// 同行说明链接：固定行高 + 同一 CJK 字体 + 按字形基线对齐(避免中英混排/两段链接错位)。
pub fn panel_caption_link(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    panel_caption_link_with_hover(ui, theme, label, None)
}


/// 同 [`panel_caption_link`]，可附悬停提示(如域名)。
pub fn panel_caption_link_with_hover(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    hover: Option<&str>,
) -> Response {
    let font_size = theme.font_size_caption();
    let row_h = theme.size_panel_filter_chip_h();
    let color = theme.accent_color();
    let font = crate::platform::ui_caption_font_id(font_size);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, color);
    let size = egui::vec2(galley.size().x.max(1.0), row_h);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.ctx().request_repaint();
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    // 两段链接共用「距行底固定间隙」的基线，而不是按 galley 盒高贴底(混排时盒高不同会看起来一高一低)。
    let baseline_in_galley = galley
        .rows
        .first()
        .and_then(|row| row.glyphs.first())
        .map(|g| g.pos.y)
        .unwrap_or(font_size * 0.85);
    let baseline_y = rect.bottom() - 2.0;
    let text_pos = egui::pos2(rect.left(), baseline_y - baseline_in_galley);
    ui.painter().galley(text_pos, galley, egui::Color32::WHITE);
    if hovered {
        ui.painter().hline(
            egui::Rangef::new(rect.left(), rect.right()),
            rect.bottom() - 0.5,
            Stroke::new(1.0, color),
        );
    }
    match hover {
        Some(tip) if !tip.is_empty() => response.on_hover_text(tip),
        _ => response,
    }
}


pub(crate) fn text_hit_button(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    font_size: f32,
    idle_color: Color32,
    hover_color: Color32,
    padding: egui::Vec2,
) -> Response {
    let font = egui::FontId::proportional(font_size);
    let measure = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), idle_color);
    let size = measure.size() + 2.0 * padding;
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    if hovered || pressed {
        ui.ctx().request_repaint();
    }
    // 暗夜常态也画浅底+描边，避免「导入」等像纯文字链接。
    let fill = if pressed {
        theme.accent_alpha(51)
    } else if hovered {
        theme.color_widget_hover_fill()
    } else if theme.uses_modern_palette() {
        theme.color_subtle_inset_fill()
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if theme.uses_modern_palette() {
        theme.color_control_secondary_stroke(true)
    } else {
        egui::Stroke::NONE
    };
    if fill != Color32::TRANSPARENT || stroke != egui::Stroke::NONE {
        ui.painter().rect(
            rect,
            theme.radius_list_item(),
            fill,
            stroke,
            egui::StrokeKind::Middle,
        );
    }
    let text_color = if hovered || pressed {
        hover_color
    } else {
        idle_color
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, text_color);
    ui.painter().galley(rect.min + padding, galley, egui::Color32::WHITE);
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}


/// 会话列表区排序：筛选行右侧图标，点开选排序方式
pub fn sidebar_list_sort_button(
    ui: &mut Ui,
    theme: &Theme,
    sort_by: &mut crate::core::session_sort::SessionSortBy,
) {
    use crate::core::session_sort::SessionSortBy;
    let ctx = ui.ctx();
    let popup_id = ui.auto_id_with("session_list_sort");
    let icon = session_sort_icon(*sort_by);
    let row_lbl = crate::i18n::session_sort_popup_row(ctx, *sort_by);
    let hover = format!(
        "{}{}{}",
        crate::i18n::tr(ctx, "Sort: ", "排序："),
        row_lbl,
        crate::i18n::tr(ctx, " (click to pick)", "（点击选择）"),
    );
    let short = crate::i18n::session_sort_chip_short(ctx, *sort_by);
    let response = panel_sort_chip(ui, theme, icon, short, &hover);
    // egui 0.37：Popup::from_toggle_button_response 自带点击开合；Below 对应 RectAlign 底部展开。
    let _ = popup_id;
    egui::Popup::from_toggle_button_response(&response)
        .align(egui::RectAlign::BOTTOM_START)
        .show(|ui| {
            apply_sidebar_menu_popup_style(ui, theme);
            ui.set_min_width(theme.size_sidebar_sort_combo_w());
            for mode in SessionSortBy::ALL {
                if ui
                    .selectable_label(
                        *sort_by == *mode,
                        RichText::new(crate::i18n::session_sort_popup_row(ui.ctx(), *mode))
                            .size(theme.font_size_sidebar_control()),
                    )
                    .clicked()
                {
                    *sort_by = *mode;
                }
            }
        });
}
// #206 S1: egui 0.37 Popup 自带 close_behavior(点击外部/选中后自动关)，不再手动 close_popup。


/// 主 / 次按钮视觉(弹窗底栏、标题栏工具、面板内操作共用)
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ControlButtonVariant {
    Primary,
    Secondary,
    Danger,
}

pub(crate) fn paint_caption_in_rect_center(
    ui: &mut Ui,
    rect: egui::Rect,
    label: &str,
    font_size: f32,
    color: Color32,
) {
    // #363：壳层按钮文案走 ui_font_id（CJK Name 族），与 TextStyle::Body/Button 一致；
    // 勿用 FontId::proportional（Geist 优先），避免中英回退段切分带来的字距观感问题。
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        crate::platform::ui_font_id(font_size),
        color,
    );
    let pos = rect.center() - galley.size() * 0.5;
    ui.painter().galley(pos, galley, egui::Color32::WHITE);
}


/// `center_row`: 工具按钮等在槽内居中；`false` 时自左 `pad_x` 起排(排序芯片、状态栏)。
pub(crate) fn paint_icon_caption_row_in_rect(
    ui: &mut Ui,
    rect: egui::Rect,
    icon: IconId,
    label: &str,
    icon_px: f32,
    gap: f32,
    font_size: f32,
    text_color: Color32,
    icon_color: Color32,
    pad_x: f32,
    center_row: bool,
) {
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(
        label.to_owned(),
        crate::platform::ui_font_id(font_size),
        text_color,
    );
    let text_w = galley.size().x;
    let text_h = galley.size().y;
    let icon_cy = rect.center().y;
    let (text_x, icon_cx) = if center_row {
        let total_w = icon_px + gap + text_w;
        let start_x = rect.center().x - total_w * 0.5;
        (start_x + icon_px + gap, start_x + icon_px * 0.5)
    } else {
        let start_x = rect.left() + pad_x;
        (start_x + icon_px + gap, start_x + icon_px * 0.5)
    };
    icons::paint_icon(
        ui,
        egui::Rect::from_center_size(egui::pos2(icon_cx, icon_cy), egui::vec2(icon_px, icon_px)),
        icon,
        icon_color,
        icon_px,
    );
    painter.galley(egui::pos2(text_x, icon_cy - text_h * 0.5), galley, egui::Color32::WHITE);
}


pub(crate) fn control_button_size(
    ui: &Ui,
    theme: &Theme,
    label: &str,
    with_icon: bool,
    min_w: f32,
) -> egui::Vec2 {
    let h = theme.size_control_btn_h();
    let pad_x = theme.spacing_control_btn_pad_x();
    let font = crate::platform::ui_font_id(theme.font_size_control_btn());
    let text_w = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, theme.text_primary())
        .size()
        .x;
    let icon_extra = if with_icon {
        theme.size_icon_glyph() + 4.0
    } else {
        0.0
    };
    let w = (text_w + icon_extra + 2.0 * pad_x).max(min_w);
    egui::vec2(w, h)
}


pub(crate) fn secondary_control_button_colors(
    theme: &Theme,
    can_activate: bool,
    hovered: bool,
    pressed: bool,
) -> (Color32, Color32, Color32) {
    if !can_activate {
        return (
            theme.color_control_secondary_fill_disabled(),
            theme.color_control_disabled_text(),
            theme.color_control_disabled_text(),
        );
    }
    if pressed {
        let c = theme.color_control_secondary_active_text();
        return (theme.color_control_secondary_fill_pressed(), c, c);
    }
    if hovered {
        let c = theme.color_control_secondary_active_text();
        return (theme.color_control_secondary_fill_hover(), c, c);
    }
    (
        theme.color_control_secondary_fill_idle(),
        theme.color_control_secondary_idle_text(),
        theme.color_control_secondary_idle_icon(),
    )
}


pub(crate) fn primary_control_button_colors(
    theme: &Theme,
    can_activate: bool,
    hovered: bool,
    pressed: bool,
) -> (Color32, Color32, Color32) {
    if !can_activate {
        if hovered {
            return (
                theme
                    .color_control_primary_disabled_fill()
                    .gamma_multiply(1.12),
                theme.color_control_disabled_text(),
                theme.color_control_disabled_text(),
            );
        }
        return (
            theme.color_control_primary_disabled_fill(),
            theme.color_control_disabled_text(),
            theme.color_control_disabled_text(),
        );
    }
    if pressed {
        let c = theme.color_modal_primary_text();
        return (theme.accent_dim_color(), c, c);
    }
    if hovered {
        let c = theme.color_modal_primary_text();
        return (theme.color_modal_primary_fill_hover(), c, c);
    }
    let c = theme.color_modal_primary_text();
    (theme.color_modal_primary_fill(), c, c)
}


pub(crate) fn paint_control_button(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    icon: Option<IconId>,
    variant: ControlButtonVariant,
    min_w: f32,
    can_activate: bool,
) -> Response {
    let size = control_button_size(ui, theme, label, icon.is_some(), min_w);
    paint_control_button_sized(ui, theme, label, icon, variant, size, can_activate)
}


/// 与 [`paint_control_button`] 相同，但尺寸由调用方给定(用于需要与同一行输入框对齐高度的场景)。
pub(crate) fn paint_control_button_sized(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    icon: Option<IconId>,
    variant: ControlButtonVariant,
    size: egui::Vec2,
    can_activate: bool,
) -> Response {
    let rounding = theme.radius_card();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    if hovered || pressed {
        ui.ctx().request_repaint();
    }

    let stroke = match variant {
        ControlButtonVariant::Primary => egui::Stroke::NONE,
        ControlButtonVariant::Secondary => theme.color_control_secondary_stroke(can_activate),
        ControlButtonVariant::Danger => egui::Stroke::new(1.0, theme.color_text_input_stroke()),
    };
    let (fill, text_color, icon_color) = match variant {
        ControlButtonVariant::Danger => {
            unreachable!("danger buttons use paint_icon_only_button")
        }
        ControlButtonVariant::Primary => {
            primary_control_button_colors(theme, can_activate, hovered, pressed)
        }
        ControlButtonVariant::Secondary => {
            secondary_control_button_colors(theme, can_activate, hovered, pressed)
        }
    };

    ui.painter().rect(rect, rounding, fill, stroke, egui::StrokeKind::Middle);
    let pad_x = theme.spacing_control_btn_pad_x();
    if let Some(id) = icon {
        let icon_px = theme.size_icon_glyph();
        // 通栏按钮：内容居中；内容宽按钮：按 pad 左起，避免字贴描边。
        let center = size.x > (control_button_size(ui, theme, label, true, 0.0).x + 8.0);
        paint_icon_caption_row_in_rect(
            ui,
            rect,
            id,
            label,
            icon_px,
            4.0,
            theme.font_size_control_btn(),
            text_color,
            icon_color,
            pad_x,
            center,
        );
    } else {
        let inner = rect.shrink2(egui::vec2(pad_x, 0.0));
        paint_caption_in_rect_center(ui, inner, label, theme.font_size_control_btn(), text_color);
    }
    if hovered {
        ui.ctx().set_cursor_icon(if can_activate {
            CursorIcon::PointingHand
        } else {
            CursorIcon::NotAllowed
        });
    }
    response
}


pub(crate) fn icon_only_button_size(theme: &Theme, min_w: f32) -> egui::Vec2 {
    let h = theme.size_control_btn_h();
    let pad_x = theme.spacing_panel_header_btn_pad_x();
    let icon_px = theme.size_icon_glyph();
    let side = (icon_px + 2.0 * pad_x).max(min_w).max(h);
    egui::vec2(side, h)
}


/// 面板标题行纯图标边长(对齐 Tab ×；须低于 [`Theme::size_panel_header_row_h`] 以免裁顶)。
pub(crate) fn header_toolbar_icon_button_size(theme: &Theme) -> egui::Vec2 {
    let side = theme.size_panel_header_control_h();
    egui::vec2(side, side)
}


/// 仅图标(方形容器)，悬停显示 `tooltip`。
pub(crate) fn paint_icon_only_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    variant: ControlButtonVariant,
    min_w: f32,
    can_activate: bool,
) -> Response {
    paint_icon_only_button_at(
        ui,
        theme,
        icon,
        variant,
        icon_only_button_size(theme, min_w),
        can_activate,
    )
}


pub(crate) fn paint_icon_only_button_at(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    variant: ControlButtonVariant,
    size: egui::Vec2,
    can_activate: bool,
) -> Response {
    let rounding = theme.radius_card();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    if hovered || pressed {
        ui.ctx().request_repaint();
    }

    let stroke = match variant {
        ControlButtonVariant::Primary => egui::Stroke::NONE,
        ControlButtonVariant::Secondary => theme.color_control_secondary_stroke(can_activate),
        ControlButtonVariant::Danger => egui::Stroke::new(1.0, theme.color_text_input_stroke()),
    };
    let (fill, icon_color) = match variant {
        ControlButtonVariant::Primary => {
            let (fill, text, icon) =
                primary_control_button_colors(theme, can_activate, hovered, pressed);
            let _ = text;
            (fill, icon)
        }
        ControlButtonVariant::Secondary => {
            let (fill, text, icon) =
                secondary_control_button_colors(theme, can_activate, hovered, pressed);
            let _ = text;
            (fill, icon)
        }
        ControlButtonVariant::Danger => {
            if hovered || pressed {
                (
                    theme
                        .red_color()
                        .gamma_multiply(if pressed { 0.22 } else { 0.14 }),
                    theme.red_color(),
                )
            } else {
                (
                    theme.color_panel_toolbar_btn_fill(),
                    theme.red_color().gamma_multiply(0.85),
                )
            }
        }
    };

    ui.painter().rect(rect, rounding, fill, stroke, egui::StrokeKind::Middle);
    let icon_px = theme
        .size_icon_glyph()
        .min((size.y - 6.0).max(12.0));
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(icon_px, icon_px));
    icons::paint_icon(ui, icon_rect, icon, icon_color, icon_px);
    if hovered {
        ui.ctx().set_cursor_icon(if can_activate {
            CursorIcon::PointingHand
        } else {
            CursorIcon::NotAllowed
        });
    }
    response
}


/// 侧栏 / 右 dock 标题行次要工具按钮(宽度按文字测量)。
pub fn panel_toolbar_button_widget<'a>(theme: &'a Theme, text: RichText) -> Button<'a> {
    Button::new(text)
        .fill(theme.color_control_secondary_fill_idle())
        .stroke(theme.color_control_secondary_stroke(true))
        .corner_radius(theme.radius_list_item())
}


pub(crate) fn panel_toolbar_button_size(ui: &Ui, theme: &Theme, label: &str, with_icon: bool) -> egui::Vec2 {
    control_button_size(
        ui,
        theme,
        label,
        with_icon,
        theme.size_panel_header_btn_min_w(),
    )
}


pub fn panel_toolbar_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    paint_control_button(
        ui,
        theme,
        label,
        None,
        ControlButtonVariant::Secondary,
        theme.size_panel_header_btn_min_w(),
        true,
    )
}


/// 无边框文字链式操作(AI 消息复制/重新生成等；`emphasis` 0~1 控制可见度)。
pub fn panel_ghost_action_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    emphasis: f32,
) -> Response {
    let emphasis = emphasis.clamp(0.18, 1.0);
    let color = theme.color_form_hint().gamma_multiply(emphasis);
    let icon_px = theme.font_size_small();
    let pad_x = theme.spacing_xs();
    let galley = ui.painter().layout(
        label.to_owned(),
        egui::FontId::proportional(theme.font_size_small()),
        color,
        f32::INFINITY,
    );
    let size = egui::vec2(
        pad_x * 2.0 + icon_px + theme.spacing_xs() + galley.size().x,
        theme.size_control_btn_h().min(22.0),
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.hovered() || emphasis >= 0.99 {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.min.x + pad_x + icon_px * 0.5, rect.center().y),
        egui::vec2(icon_px, icon_px),
    );
    crate::ui::icons::paint_icon(ui, icon_rect, icon, color, icon_px);
    ui.painter().galley(
        egui::pos2(
            icon_rect.max.x + theme.spacing_xs(),
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        egui::Color32::WHITE,
    );
    response.on_hover_text(label)
}


/// 标题行 / 工具栏纯图标按钮(悬停文案见 `tooltip`)。
/// 使用标题行控件边长，避免高于 dock 标题行被裁切顶部描边。
pub fn panel_toolbar_icon_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
) -> Response {
    paint_icon_only_button_at(
        ui,
        theme,
        icon,
        ControlButtonVariant::Secondary,
        header_toolbar_icon_button_size(theme),
        true,
    )
    .on_hover_text(tooltip)
}


/// AI 面板底部操作栏纯图标按钮(28×28，悬停文案见 `tooltip`)。
pub fn panel_ai_action_icon_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
) -> Response {
    let side = theme.size_ai_action_btn();
    paint_icon_only_button_at(
        ui,
        theme,
        icon,
        ControlButtonVariant::Secondary,
        egui::vec2(side, side),
        true,
    )
    .on_hover_text(tooltip)
}


/// 标题行 / 工具栏：图标 + 可见文字。
pub fn panel_toolbar_button_with_icon(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
) -> Response {
    paint_control_button(
        ui,
        theme,
        label,
        Some(icon),
        ControlButtonVariant::Secondary,
        theme.size_panel_header_btn_min_w(),
        true,
    )
}


/// 工具栏图标按钮或采集中态(带可见标签)。
pub fn panel_toolbar_button_with_icon_or_busy(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    busy_label: &str,
    busy: bool,
) -> Response {
    if !busy {
        return panel_toolbar_button_with_icon(ui, theme, icon, label);
    }
    let size = control_button_size(
        ui,
        theme,
        busy_label,
        true,
        theme.size_panel_header_btn_min_w(),
    );
    let rounding = theme.radius_list_item();
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect(
        rect,
        rounding,
        theme.color_panel_toolbar_btn_fill(),
        theme.divider_stroke(),
        egui::StrokeKind::Middle,
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.add_space(6.0);
    child.add(egui::Spinner::new());
    child.add_space(4.0);
    child.label(
        RichText::new(busy_label)
            .size(theme.font_size_control_btn())
            .color(theme.text_tertiary()),
    );
    response.on_hover_text(busy_label)
}


/// 标题行主操作(实心 accent，纯图标)。
pub fn panel_toolbar_primary_icon_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
) -> Response {
    paint_control_button(
        ui,
        theme,
        tooltip,
        Some(icon),
        ControlButtonVariant::Primary,
        theme.size_panel_header_btn_min_w(),
        true,
    )
}


/// 工具栏图标按钮或采集中态：槽位尺寸与 [`panel_toolbar_icon_button`] 一致，避免刷新时行高跳动。
pub fn panel_toolbar_icon_button_or_busy(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
    busy: bool,
) -> Response {
    if !busy {
        return panel_toolbar_icon_button(ui, theme, icon, tooltip);
    }
    let size = icon_only_button_size(theme, theme.size_panel_header_btn_min_w());
    let rounding = theme.radius_list_item();
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect(
        rect,
        rounding,
        theme.color_panel_toolbar_btn_fill(),
        theme.divider_stroke(),
        egui::StrokeKind::Middle,
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(
        rect).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)),
    );
    child.add(egui::Spinner::new());
    response.on_hover_text(crate::i18n::tr(ui.ctx(), "Collecting metrics…", "采集中…"))
}


/// 面板 / dock 内行内次要按钮(与排序芯片、弹窗「取消」同族)
pub fn panel_action_icon_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
) -> Response {
    panel_action_button_with_icon_ex(ui, theme, icon, tooltip, true)
}


pub fn panel_action_icon_button_ex(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
    enabled: bool,
) -> Response {
    panel_action_button_with_icon_ex(ui, theme, icon, tooltip, enabled)
}


pub fn panel_action_primary_icon_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
) -> Response {
    panel_action_primary_button_with_icon_ex(ui, theme, icon, tooltip, true)
}


pub fn panel_action_primary_icon_button_ex(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
    enabled: bool,
) -> Response {
    panel_action_primary_button_with_icon_ex(ui, theme, icon, tooltip, enabled)
}


/// 面板 / dock 内行内次要按钮(与排序芯片、弹窗「取消」同族)
pub fn panel_action_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    panel_action_button_ex(ui, theme, label, true)
}


/// [`panel_action_button`] 的实际尺寸(供同一行内先量再排，例如空态三钮居中)。
pub fn panel_action_button_size(ui: &Ui, theme: &Theme, label: &str) -> egui::Vec2 {
    control_button_size(
        ui,
        theme,
        label,
        false,
        theme.size_control_btn_min_w(),
    )
}


/// 带启用态的面板次要按钮
pub fn panel_action_button_ex(ui: &mut Ui, theme: &Theme, label: &str, enabled: bool) -> Response {
    panel_action_button_ex_min_w(ui, theme, label, enabled, theme.size_control_btn_min_w())
}


/// 把已绘制的按钮标记为「禁用」并挂上「为什么点不动」的悬停说明。
/// 自绘按钮用 `Sense::click()` 分配、悬停仍可达，`Response::enabled` 默认是 `true`；
/// 不显式置 false 的话 [`egui::Response::on_disabled_hover_text`] 永远不会触发。
/// 只在 `enabled == false` 时挂提示，避免无差别给所有按钮加 tooltip。
pub fn disabled_action_with_reason(mut response: Response, enabled: bool, reason: &str) -> Response {
    if !enabled {
        // 0.37 Response 不再暴露 enabled 字段/setter：自绘按钮用 Sense::hover()
        // 即可同时达成「不响应点击 + 悬停 tooltip 可达」，与原语义一致。
        response = response.interact(egui::Sense::hover());
        response = response.on_disabled_hover_text(reason);
    }
    response
}


/// 常显的灰字「为什么禁用」提示(命令历史 / 粘贴历史底栏等需要一眼可见的场合)。
pub fn disabled_reason_hint(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_small())
            .color(theme.text_tertiary()),
    );
}


/// 带禁用原因的次要按钮；`reason_when_disabled` 仅在 `enabled == false` 时显示。
pub fn panel_action_button_ex_with_reason(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    enabled: bool,
    reason_when_disabled: &str,
) -> Response {
    disabled_action_with_reason(
        panel_action_button_ex(ui, theme, label, enabled),
        enabled,
        reason_when_disabled,
    )
}


/// 指定最小宽度的次要按钮(通栏 / 等分列用)。
pub fn panel_action_button_ex_min_w(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    enabled: bool,
    min_w: f32,
) -> Response {
    paint_control_button(
        ui,
        theme,
        label,
        None,
        ControlButtonVariant::Secondary,
        min_w,
        enabled,
    )
}


/// 面板内行内主按钮(保存、克隆、确认、重试等)— 实心 accent，与标题「新建」同族。
pub fn panel_action_primary_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    panel_action_primary_button_ex(ui, theme, label, true)
}


pub fn panel_action_primary_button_ex(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    enabled: bool,
) -> Response {
    paint_control_button(
        ui,
        theme,
        label,
        None,
        ControlButtonVariant::Primary,
        theme.size_control_btn_min_w(),
        enabled,
    )
}


/// 图标 + 文字的次要按钮(侧栏 SFTP / 资源面板等比纯图标更易识别的工具按钮)。
pub fn panel_action_button_with_icon_ex(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    enabled: bool,
) -> Response {
    panel_action_button_with_icon_ex_min_w(
        ui,
        theme,
        icon,
        label,
        enabled,
        theme.size_control_btn_min_w(),
    )
}


pub fn panel_action_button_with_icon_ex_min_w(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    enabled: bool,
    min_w: f32,
) -> Response {
    paint_control_button(
        ui,
        theme,
        label,
        Some(icon),
        ControlButtonVariant::Secondary,
        min_w,
        enabled,
    )
}


/// 图标 + 文字的主按钮(最显眼的「上传」等正向操作)— 实心 accent。
pub fn panel_action_primary_button_with_icon_ex(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    enabled: bool,
) -> Response {
    panel_action_primary_button_with_icon_ex_min_w(
        ui,
        theme,
        icon,
        label,
        enabled,
        theme.size_control_btn_min_w(),
    )
}


pub fn panel_action_primary_button_with_icon_ex_min_w(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    enabled: bool,
    min_w: f32,
) -> Response {
    paint_control_button(
        ui,
        theme,
        label,
        Some(icon),
        ControlButtonVariant::Primary,
        min_w,
        enabled,
    )
}


/// 实心主按钮(与 [`panel_action_primary_button_with_icon_ex`] 同族)。
pub fn panel_solid_primary_button_with_icon_ex(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    enabled: bool,
) -> Response {
    panel_action_primary_button_with_icon_ex(ui, theme, icon, label, enabled)
}


/// 有底+描边的工具按钮(与次要按钮同族，暗夜统一浅底+描边)。
pub fn panel_outlined_toolbar_button_with_icon_ex(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    enabled: bool,
) -> Response {
    panel_action_button_with_icon_ex(ui, theme, icon, label, enabled)
}


/// 有底+描边的纯图标按钮(与次要按钮同族)。
pub fn panel_outlined_icon_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
    enabled: bool,
) -> Response {
    panel_action_icon_button_ex(ui, theme, icon, tooltip, enabled)
}


/// 矮轮廓快捷 chip(空态提问；固定行高 + 统一字号，同行文字基线对齐)。
pub fn panel_quick_prompt_chip(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    let font = egui::FontId::proportional(theme.font_size_ui_control());
    let text_color = theme.text_primary();
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, text_color);
    let pad_x = 10.0;
    // 高度不跟单条文案的 galley 走(全角「?」等会抬高行盒)，同行必须同高。
    let chip_h = theme.size_panel_filter_chip_h().max(theme.size_control_btn_h() - 2.0);
    let size = egui::vec2(
        (galley.size().x + pad_x * 2.0).max(theme.size_panel_header_btn_min_w()),
        chip_h,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    let fill = if pressed {
        theme.color_widget_active_fill()
    } else if hovered {
        theme.color_widget_hover_fill()
    } else {
        theme.color_subtle_inset_fill()
    };
    let stroke = egui::Stroke::new(
        theme.hairline_width(ui.ctx()),
        if hovered || pressed {
            theme.accent_alpha(160)
        } else {
            theme.color_text_input_stroke()
        },
    );
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(theme.radius_category() as u8),
        fill,
        stroke,
        egui::StrokeKind::Middle,
    );
    // 垂直居中到固定 chip 高，同行多枚文字落在同一条中线。
    let text_pos = egui::pos2(
        rect.center().x - galley.size().x * 0.5,
        rect.center().y - galley.size().y * 0.5,
    );
    ui.painter().galley(text_pos, galley, egui::Color32::WHITE);
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

