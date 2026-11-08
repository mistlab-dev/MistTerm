//! 表单控件：输入框、多行、下拉、复选、stepper、进度条、滑杆、搜索框（#398 自 chrome.rs 拆出）。
//! 纯移动，零行为变更。

use crate::ui::icons::{self, IconId};
use crate::ui::theme::Theme;
use eframe::egui::{
    self, Button, Color32, CursorIcon, Painter, Response, RichText, Sense, Stroke, Ui, Widget,
};
use super::*;

/// 面板标题 RichText(modern =flat 主色；其它主题加粗)
pub fn rich_panel_header_title(theme: &Theme, text: &str) -> RichText {
    let mut rt = RichText::new(text).size(theme.font_size_panel_header_title());
    if theme.uses_modern_palette() {
        rt = rt.color(theme.text_primary());
    } else {
        rt = rt.strong().color(theme.color_panel_header_title());
    }
    rt
}


/// 居中弹窗主标题(与 [`rich_panel_header_title`] 一致)
pub fn rich_modal_title(theme: &Theme, text: &str) -> RichText {
    rich_panel_header_title(theme, text)
}


/// 区段标题(与 [`rich_panel_header_title`] 一致；`color` 参数保留兼容)
pub fn rich_section_title(theme: &Theme, text: &str, _color: Color32) -> RichText {
    rich_panel_header_title(theme, text)
}


/// 右 dock 标题(与 [`rich_panel_header_title`] 一致)
pub fn rich_dock_title(theme: &Theme, text: &str) -> RichText {
    rich_panel_header_title(theme, text)
}


/// 表单字段标签 — 12px 加粗，语义色 [`color_form_label`]
pub fn rich_form_label(theme: &Theme, text: &str) -> RichText {
    RichText::new(text)
        .size(theme.font_size_form_label())
        .strong()
        .color(theme.color_form_label())
}


/// 正文 — 12px
pub fn rich_body(theme: &Theme, text: &str) -> RichText {
    RichText::new(text)
        .size(theme.font_size_body())
        .color(theme.text_primary())
}


/// 辅助说明 / 元信息 — 10px
pub fn rich_caption(theme: &Theme, text: &str) -> RichText {
    RichText::new(text)
        .size(theme.font_size_caption())
        .color(theme.color_body_text_muted())
}


pub fn form_field_label(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.label(rich_form_label(theme, text));
}


/// 曾用于卡内子项分隔线；线感始终偏重，已改为无操作，保留调用点以免大面积改布局。
pub fn settings_row_divider(_ui: &mut Ui, _theme: &Theme) {}


/// 设置页抬升底卡片：无描边、圆角对齐面板；子项靠自然间距排列。
pub fn settings_elevated_card(ui: &mut Ui, theme: &Theme, body: impl FnOnce(&mut Ui)) {
    let outer_w = crate::ui::layout_util::set_width_to_available(ui);
    ui.set_min_width(outer_w);
    let pad_x = theme.spacing_search_input_x().max(14.0);
    let pad_y = theme.spacing_sm().max(10.0);
    egui::Frame::NONE
        .fill(theme.color_elevated_card_fill())
        .stroke(egui::Stroke::NONE)
        .corner_radius(theme.radius_panel())
        .inner_margin(egui::Margin::symmetric(pad_x as i8, pad_y as i8))
        .show(ui, |ui| {
            let inner_w = crate::ui::layout_util::set_width_to_available(ui);
            ui.set_min_width(inner_w);
            // 分隔线已拿掉。行距走主题 token，现代 / 经典同一节奏（#130）。
            ui.spacing_mut().item_spacing.y = theme.spacing_card_row();
            body(ui);
        });
    ui.add_space(theme.spacing_md().max(10.0));
}


/// 表单标签的占位宽度，用来把两行步进器的数字框对齐。
pub fn form_label_advance(ui: &Ui, theme: &Theme, text: &str) -> f32 {
    let font = egui::FontId::proportional(theme.font_size_form_label());
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font, theme.color_form_label()));
    galley.size().x.ceil() + 4.0
}


/// 与步进器/数字框同一行时用：占位高度对齐行盒，文字垂直居中。
pub fn form_inline_label(ui: &mut Ui, theme: &Theme, text: &str) {
    let h = form_stepper_row_h(theme);
    let font = egui::FontId::proportional(theme.font_size_form_label());
    let color = theme.color_form_label();
    let galley = ui.ctx().fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font, color));
    let w = galley.size().x.ceil() + 2.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
    ui.painter().galley(
        egui::pos2(rect.left(), rect.center().y - galley.size().y * 0.5),
        galley,
        egui::Color32::WHITE,
    );
}


/// 表单控件行：固定行高 + 垂直居中，避免标签/输入/步进器/复选框基线错乱。
pub fn form_control_row<R>(ui: &mut Ui, theme: &Theme, add: impl FnOnce(&mut Ui) -> R) -> R {
    let row_h = form_stepper_row_h(theme);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), row_h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_height(row_h);
            ui.spacing_mut().item_spacing.x = theme.spacing_sm();
            add(ui)
        },
    )
    .inner
}


/// 左标签(固定宽) + 右侧控件，窄面板里一项一行，基线对齐。
pub fn form_labeled_control_row<R>(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    label_w: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    form_control_row(ui, theme, |ui| {
        let h = form_stepper_row_h(theme);
        let font = egui::FontId::proportional(theme.font_size_form_label());
        let color = theme.color_form_label();
        let galley = ui.fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font, color));
        let w = label_w.max(galley.size().x.ceil() + 4.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
        ui.painter().galley(
            egui::pos2(rect.left(), rect.center().y - galley.size().y * 0.5),
            galley,
            color,
        );
        add(ui)
    })
}


/// 步进器方钮 / 数字框 / 步进器自身行高（20px）。
///
/// 与 12px 表单标签对齐，取 #130 目标区间 20–22 的下沿，比标注截图里约 26px 的 ± 和数字框明显矮一档。
/// 不是通用表单行高。表单行盒仍约 26px（与通用按钮同高），方钮在行内垂直居中。
/// `form_control_row` 用 `set_height` 锁死行高；若把 26px 按钮塞进 20px 行盒会被裁切。
pub fn form_stepper_control_h(theme: &Theme) -> f32 {
    stepper_control_h_from_btn(theme.size_control_btn_h())
}


/// 表单行盒高度（约 26px），与通用按钮同高。
///
/// 步进器方钮和数字框用 [`form_stepper_control_h`]（20px），在这个行盒里垂直居中。
pub(crate) fn form_stepper_row_h(theme: &Theme) -> f32 {
    24.0_f32.max(theme.size_control_btn_h().min(26.0))
}


pub(crate) fn form_stepper_btn_side(theme: &Theme) -> f32 {
    form_stepper_control_h(theme)
}


/// 控件行内的单行输入：高度与表单行盒一致，文字垂直居中。
///
/// 始终走表单实心底+描边（与 [`form_singleline_field_boxed`] 同套），不走 modern 下划线，
/// 以便端口转发等同排控件与下拉/按钮描边成套。
pub fn form_inline_singleline(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    text: &mut String,
    hint: &str,
    desired_width: f32,
    password: bool,
) -> Response {
    let row_h = form_stepper_row_h(theme);
    let focused = ui.memory(|m| m.has_focus(id));
    let width = desired_width.max(48.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, row_h), Sense::hover());
    let stroke = if focused {
        egui::Stroke::new(1.0, theme.accent_color())
    } else {
        egui::Stroke::new(1.0, theme.color_form_input_stroke())
    };
    ui.painter().rect(
        rect,
        theme.radius_search_input(),
        theme.color_form_input_fill(),
        stroke,
        egui::StrokeKind::Middle,
    );
    let inner = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 2.0, rect.top()),
        egui::vec2((width - 4.0).max(8.0), row_h),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.set_min_size(inner.size());
        ui.set_max_size(inner.size());
        with_underline_field_visuals(ui, theme, |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let prev_override = ui.style_mut().visuals.override_text_color;
                ui.style_mut().visuals.override_text_color = Some(theme.color_form_hint());
                let mut edit = egui::TextEdit::singleline(text)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .desired_width((width - 8.0).max(40.0))
                    .margin(egui::vec2(0.0, 0.0))
                    .align(egui::Align2::CENTER_CENTER)
                    .text_color(theme.color_text_input_text())
                    .font(egui::FontId::proportional(theme.font_size_control_input()));
                if !hint.is_empty() {
                    edit = edit.hint_text(hint_rich(theme, hint, theme.font_size_control_input()));
                }
                if password {
                    edit = edit.password(true);
                }
                let response = ui.add(edit);
                ui.style_mut().visuals.override_text_color = prev_override;
                response
            })
            .inner
        })
    })
    .inner
}


/// 控件行内复选框：占满行高并垂直居中。
pub fn form_inline_checkbox(
    ui: &mut Ui,
    theme: &Theme,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut bool,
    text: &str,
) -> Response {
    let row_h = form_stepper_row_h(theme);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width().min(160.0), row_h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_max_height(row_h);
            form_checkbox_with_id(ui, theme, id, value, text)
        },
    )
    .inner
}


/// 统一复选框：未选中时浅底 + 描边始终可见；勾选为 accent 底 + 浅色勾。
/// 全局 `inactive.bg_fill = TRANSPARENT` 下裸 `ui.checkbox` 往往只有悬停才看得出方框。
/// 产品 UI（偏好 / 规则 / 导入 / 面板表单等）禁止裸 `ui.checkbox` / `Checkbox::new`，请走本 helper；
/// 禁用占位也用 `ui.add_enabled_ui(false, … form_checkbox*)`，保持同一套 off 态描边。
pub fn form_checkbox(ui: &mut Ui, theme: &Theme, value: &mut bool, text: &str) -> Response {
    form_checkbox_with_id(ui, theme, text, value, text)
}


pub fn form_checkbox_with_id(
    ui: &mut Ui,
    theme: &Theme,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut bool,
    text: &str,
) -> Response {
    ui.push_id(egui::Id::unique(id), |ui| {
        let rounding = egui::CornerRadius::same(theme.radius_checkbox() as u8);
        let off_border = theme.color_checkbox_off_stroke_color();
        let w = &mut ui.style_mut().visuals.widgets;
        w.inactive.bg_fill = theme.color_checkbox_off_fill();
        w.inactive.bg_stroke = egui::Stroke::new(1.0, off_border);
        w.inactive.corner_radius = rounding;
        w.hovered.bg_fill = theme.color_checkbox_hover_fill();
        w.hovered.bg_stroke = egui::Stroke::new(1.0, theme.accent_dim_color());
        w.hovered.corner_radius = rounding;
        w.active.bg_fill = theme.accent_color();
        w.active.bg_stroke = egui::Stroke::new(1.0, theme.accent_color());
        w.active.corner_radius = rounding;
        w.active.fg_stroke = egui::Stroke::new(1.8, theme.color_checkbox_checkmark());
        ui.checkbox(value, text)
    })
    .inner
}


/// 地址栏内嵌路径输入(无下划线、透明底，随剩余宽度伸缩)。
pub fn form_singleline_field_sftp_embedded(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    text: &mut String,
    hint: &str,
) -> Response {
    with_underline_field_visuals(ui, theme, |ui| {
        let w = ui.available_width().max(48.0);
        let prev_override = ui.style_mut().visuals.override_text_color;
        ui.style_mut().visuals.override_text_color = Some(theme.color_form_hint());
        let mut edit = egui::TextEdit::singleline(text)
            .id(id)
            .frame(egui::Frame::NONE)
            .desired_width(w)
            .text_color(theme.text_primary())
            .font(egui::FontId::proportional(theme.font_size_control_input()));
        if !hint.is_empty() {
            edit = edit.hint_text(hint_rich(theme, hint, theme.font_size_control_input()));
        }
        let response = ui.add(edit);
        ui.style_mut().visuals.override_text_color = prev_override;
        response
    })
}


/// 偏好 / 设置区小节标题(与表单标签区分层级)
pub fn form_section_heading(theme: &Theme, text: &str) -> RichText {
    RichText::new(text)
        .size(theme.font_size_panel_title())
        .strong()
        .color(theme.color_form_label())
}


/// 表单输入区临时视觉(modern 下划线：透明 TextEdit 底)
pub(crate) fn with_underline_field_visuals<R>(
    ui: &mut Ui,
    theme: &Theme,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    if !theme.uses_underline_inputs() {
        return add(ui);
    }
    let prev_extreme = ui.visuals().extreme_bg_color;
    let prev_inactive = ui.style().visuals.widgets.inactive.bg_fill;
    let prev_weak = ui.style().visuals.widgets.inactive.weak_bg_fill;
    ui.visuals_mut().extreme_bg_color = Color32::TRANSPARENT;
    ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
    ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    let out = add(ui);
    ui.visuals_mut().extreme_bg_color = prev_extreme;
    ui.style_mut().visuals.widgets.inactive.bg_fill = prev_inactive;
    ui.style_mut().visuals.widgets.inactive.weak_bg_fill = prev_weak;
    out
}


pub(crate) fn paint_form_field_underline(ui: &Ui, theme: &Theme, rect: egui::Rect, focused: bool) {
    let ctx = ui.ctx();
    let w = theme.hairline_width(ctx);
    let line_y = theme.snap_y_to_pixel(ctx, rect.bottom() - w * 0.5);
    let line_color = if focused {
        theme.color_input_underline_focus()
    } else {
        theme.color_input_underline_idle()
    };
    ui.painter()
        .hline(rect.x_range(), line_y, Stroke::new(w, line_color));
}


/// 输入框占位符 RichText(斜体 + 弱色，与输入正文区分)
pub fn hint_rich(theme: &Theme, text: &str, font_size: f32) -> RichText {
    RichText::new(text)
        .size(font_size)
        .italics()
        .color(theme.color_form_hint())
}


/// 单行输入框(modern：透明底 + 底边线；其它主题：带底+描边)
pub fn form_singleline_field(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    text: &mut String,
    hint: &str,
    desired_width: f32,
    password: bool,
) -> Response {
    let focused = ui.memory(|m| m.has_focus(id));
    let underline = theme.uses_underline_inputs();
    let inner_w = if underline {
        desired_width.max(48.0)
    } else {
        (desired_width - theme.spacing_search_input_x() * 2.0 - 4.0).max(48.0)
    };
    let shown = theme.frame_form_text_input(focused).show(ui, |ui| {
        with_underline_field_visuals(ui, theme, |ui| {
            let prev_override = ui.style_mut().visuals.override_text_color;
            ui.style_mut().visuals.override_text_color = Some(theme.color_form_hint());
            let mut edit = egui::TextEdit::singleline(text)
                .id(id)
                .frame(egui::Frame::NONE)
                .desired_width(inner_w)
                .text_color(theme.color_text_input_text())
                .font(egui::FontId::proportional(theme.font_size_control_input()));
            if !hint.is_empty() {
                edit = edit.hint_text(hint_rich(theme, hint, theme.font_size_control_input()));
            }
            if password {
                edit = edit.password(true);
            }
            let response = ui.add(edit);
            ui.style_mut().visuals.override_text_color = prev_override;
            response
        })
    });
    if underline {
        paint_form_field_underline(ui, theme, shown.response.rect, focused);
    }
    shown.inner
}


/// 表单实心单行输入框(凭证库 / 表单弹窗用)：暗夜 #1e1e1e 底 + 1px #3c3c3c 描边，
/// 不走 [`form_singleline_field`] 的下划线样式，便于和同高的按钮并排对齐。
pub fn form_singleline_field_boxed(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    text: &mut String,
    hint: &str,
    desired_width: f32,
    password: bool,
) -> Response {
    let focused = ui.memory(|m| m.has_focus(id));
    let row_h = theme.size_control_btn_h().max(28.0);
    let width = desired_width.max(96.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, row_h), Sense::hover());
    let stroke = if focused {
        egui::Stroke::new(1.0, theme.accent_color())
    } else {
        egui::Stroke::new(1.0, theme.color_form_input_stroke())
    };
    ui.painter().rect(
        rect,
        theme.radius_search_input(),
        theme.color_form_input_fill(),
        stroke,
        egui::StrokeKind::Inside,
    );
    let pad_x = theme.spacing_search_input_x();
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + pad_x, rect.top()),
        egui::pos2(rect.right() - pad_x, rect.bottom()),
    );
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(inner),
        |ui| {
            ui.set_min_size(inner.size());
            ui.set_max_size(inner.size());
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let prev_override = ui.style_mut().visuals.override_text_color;
                ui.style_mut().visuals.override_text_color = Some(theme.color_form_hint());
                let mut edit = egui::TextEdit::singleline(text)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .desired_width(inner.width().max(40.0))
                    .align(egui::Align2::CENTER_CENTER)
                    .text_color(theme.color_text_input_text())
                    .font(egui::FontId::proportional(theme.font_size_control_input()));
                if !hint.is_empty() {
                    edit = edit.hint_text(hint_rich(theme, hint, theme.font_size_control_input()));
                }
                if password {
                    edit = edit.password(true);
                }
                let response = ui.add(edit);
                ui.style_mut().visuals.override_text_color = prev_override;
                response
            })
            .inner
        },
    )
    .inner
}


/// 多行输入框：始终用有底边框(不下划线)，避免「上面光标、下面一根线」的假高行。
pub fn form_multiline_field(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    text: &mut String,
    desired_width: f32,
    rows: usize,
    password: bool,
) -> Response {
    form_multiline_field_with_hint(ui, theme, id, text, "", desired_width, rows, password)
}


/// 带占位提示的多行输入。
///
/// 底/描边/圆角与 [`form_singleline_field_boxed`] / [`form_combo`] 同套（`color_form_input_*`）。
pub fn form_multiline_field_with_hint(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    text: &mut String,
    hint: &str,
    desired_width: f32,
    rows: usize,
    password: bool,
) -> Response {
    let focused = ui.memory(|m| m.has_focus(id));
    let rows = rows.max(1).min(8);
    let pad_y = 4.0;
    let inner_w = (desired_width - theme.spacing_search_input_x() * 2.0 - 4.0).max(48.0);
    egui::Frame::NONE
        .fill(theme.color_form_input_fill())
        .stroke(if focused {
            egui::Stroke::new(1.0, theme.accent_color())
        } else {
            egui::Stroke::new(1.0, theme.color_form_input_stroke())
        })
        .corner_radius(theme.radius_search_input())
        .inner_margin(egui::Margin::symmetric(theme.spacing_search_input_x() as i8, pad_y as i8))
        .show(ui, |ui| {
            let mut edit = egui::TextEdit::multiline(text)
                .id(id)
                .frame(egui::Frame::NONE)
                .desired_width(inner_w)
                .desired_rows(rows)
                .text_color(theme.color_text_input_text())
                .font(egui::FontId::proportional(theme.font_size_control_input()));
            if !hint.is_empty() {
                edit = edit.hint_text(hint_rich(theme, hint, theme.font_size_control_input()));
            }
            if password {
                edit = edit.password(true);
            }
            ui.add(edit)
        })
        .inner
}


/// 给 ComboBox 按钮套上与表单输入相同的底+描边（modern 全局 inactive 透明时裸下拉几乎看不见框）。
pub(crate) fn apply_form_combo_button_visuals(visuals: &mut egui::Visuals, theme: &Theme) {
    let fill = theme.color_form_input_fill();
    let stroke = egui::Stroke::new(1.0, theme.color_form_input_stroke());
    let focus_stroke = egui::Stroke::new(1.0, theme.accent_color());
    let radius = egui::CornerRadius::same(theme.radius_search_input() as u8);
    let fg = egui::Stroke::new(1.0, theme.color_text_input_text());
    let hover_fill = Color32::from_rgb(
        fill.r().saturating_add(14),
        fill.g().saturating_add(14),
        fill.b().saturating_add(14),
    );

    let idle = &mut visuals.widgets.inactive;
    idle.bg_fill = fill;
    idle.weak_bg_fill = fill;
    idle.bg_stroke = stroke;
    idle.corner_radius = radius;
    idle.fg_stroke = fg;
    idle.expansion = 0.0;

    let hovered = &mut visuals.widgets.hovered;
    hovered.bg_fill = hover_fill;
    hovered.weak_bg_fill = hover_fill;
    hovered.bg_stroke = stroke;
    hovered.corner_radius = radius;
    hovered.fg_stroke = fg;
    hovered.expansion = 0.0;

    let active = &mut visuals.widgets.active;
    active.bg_fill = hover_fill;
    active.weak_bg_fill = hover_fill;
    active.bg_stroke = focus_stroke;
    active.corner_radius = radius;
    active.fg_stroke = fg;
    active.expansion = 0.0;

    let open = &mut visuals.widgets.open;
    open.bg_fill = hover_fill;
    open.weak_bg_fill = hover_fill;
    open.bg_stroke = focus_stroke;
    open.corner_radius = radius;
    open.fg_stroke = fg;
    open.expansion = 0.0;
}


/// 统一下拉：与 [`form_singleline_field_boxed`] / [`form_multiline_field`] 同套底+描边+圆角，高度贴近控件按钮。
/// 弹出层仍走 [`apply_menu_popup_style`]。产品表单禁止裸 `ComboBox`（modern 下无框）。
pub fn form_combo<R>(
    ui: &mut Ui,
    theme: &Theme,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    selected_text: impl Into<egui::WidgetText>,
    width: f32,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let font_size = theme.font_size_control_input();
    let font = egui::FontId::proportional(font_size);
    ui.style_mut()
        .text_styles
        .insert(egui::TextStyle::Button, font.clone());
    ui.style_mut()
        .text_styles
        .insert(egui::TextStyle::Body, font);

    let prev_inactive = ui.visuals().widgets.inactive;
    let prev_hovered = ui.visuals().widgets.hovered;
    let prev_active = ui.visuals().widgets.active;
    let prev_open = ui.visuals().widgets.open;
    let prev_pad = ui.spacing().button_padding;

    apply_form_combo_button_visuals(ui.visuals_mut(), theme);
    // 字号 + 上下 padding ≈ size_control_btn_h，与同行主/次按钮齐高。
    let pad_y = ((theme.size_control_btn_h() - font_size) * 0.5)
        .max(4.0)
        .min(theme.spacing_search_input_y().max(4.0));
    ui.spacing_mut().button_padding = egui::vec2(theme.spacing_search_input_x(), pad_y);

    let response = egui::ComboBox::from_id_salt(id_salt)
        .selected_text(selected_text)
        .width(width.max(48.0))
        .show_ui(ui, |ui| {
            apply_menu_popup_style(ui, theme);
            add_contents(ui)
        });

    ui.visuals_mut().widgets.inactive = prev_inactive;
    ui.visuals_mut().widgets.hovered = prev_hovered;
    ui.visuals_mut().widgets.active = prev_active;
    ui.visuals_mut().widgets.open = prev_open;
    ui.spacing_mut().button_padding = prev_pad;
    response
}


/// 主题进度条，条内显示百分比。
///
/// 裸 `egui::ProgressBar` 不设 `fill` 时用 `selection.bg_fill`（文本拖选底）。
/// 暗夜那是半透明白，百分比又走 `override_text_color`（白字），于是「32%」落在浅灰填充上。
/// 这里强制填充、轨道和标签色，不改 `selection.bg_fill`。
pub fn progress_bar(ui: &mut Ui, theme: &Theme, fraction: f32) -> Response {
    let fraction = fraction.clamp(0.0, 1.0);
    let pct = (fraction * 100.0) as usize;
    progress_bar_labeled(ui, theme, fraction, &format!("{pct}%"), None)
}


/// 主题进度条，条内使用调用方文案（SFTP 传输、rz/sz）。
///
/// `desired_width` 为 `None` 时占满当前行。高度取 [`Theme::progress_bar_height`] 与正文行高的较大值：
/// egui 把标签裁在条内，8px 的监控细条会把百分比切掉。
pub fn progress_bar_labeled(
    ui: &mut Ui,
    theme: &Theme,
    fraction: f32,
    label: &str,
    desired_width: Option<f32>,
) -> Response {
    let fraction = fraction.clamp(0.0, 1.0);
    let prev_extreme = ui.visuals().extreme_bg_color;
    // 轨道没有单独的 setter，egui 用 extreme_bg_color 画未填充部分。只改这一笔，画完还原。
    ui.visuals_mut().extreme_bg_color = theme.color_progress_track();
    let height = theme
        .progress_bar_height()
        .max(theme.font_size_body() + 6.0);
    let text = egui::RichText::new(label)
        .color(theme.color_progress_label())
        .size(theme.font_size_body());
    let mut bar = egui::ProgressBar::new(fraction)
        .desired_height(height)
        .fill(theme.color_progress_fill())
        .text(text);
    if let Some(width) = desired_width {
        bar = bar.desired_width(width);
    }
    let response = ui.add(bar);
    ui.visuals_mut().extreme_bg_color = prev_extreme;
    response
}


/// 只读多行文本：支持鼠标拖选与 Ctrl/Cmd+C(`&str` 缓冲不可编辑)。
/// 带可见滑轨的水平滑块(全局 `inactive.bg_fill` 为透明时仍绘制轨道)。
pub fn labeled_slider_f32(
    ui: &mut Ui,
    theme: &Theme,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    label: &str,
    suffix: &str,
) -> Response {
    let prev_inactive = ui.visuals().widgets.inactive.bg_fill;
    ui.visuals_mut().widgets.inactive.bg_fill = theme.color_slider_rail_fill();
    let resp = ui.add(
        egui::Slider::new(value, range)
            .text(label)
            .suffix(suffix)
            .trailing_fill(true),
    );
    ui.visuals_mut().widgets.inactive.bg_fill = prev_inactive;
    resp
}


/// 铺满可用宽度的滑块(不显示数值，数值由调用方在上一行展示)。
///
/// 窄面板(右 dock ~250px)里 [`labeled_slider_f32`] 把滑轨/数值/文本挤在同一行，
/// 滑轨只剩几十像素；这一支让滑轨独占一行铺满正文宽。
pub fn slider_full_width_f32(
    ui: &mut Ui,
    theme: &Theme,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> Response {
    let prev_inactive = ui.visuals().widgets.inactive.bg_fill;
    let prev_slider_w = ui.spacing().slider_width;
    ui.visuals_mut().widgets.inactive.bg_fill = theme.color_slider_rail_fill();
    ui.spacing_mut().slider_width = (ui.available_width() - 6.0).max(60.0);
    let resp = ui.add(
        egui::Slider::new(value, range)
            .show_value(false)
            .trailing_fill(true),
    );
    ui.visuals_mut().widgets.inactive.bg_fill = prev_inactive;
    ui.spacing_mut().slider_width = prev_slider_w;
    resp
}


/// 带可见滑轨的水平滑块(`f64` 版本，如刷新间隔秒数)。
pub fn labeled_slider_f64(
    ui: &mut Ui,
    theme: &Theme,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    label: &str,
) -> Response {
    let prev_inactive = ui.visuals().widgets.inactive.bg_fill;
    ui.visuals_mut().widgets.inactive.bg_fill = theme.color_slider_rail_fill();
    let resp = ui.add(
        egui::Slider::new(value, range)
            .text(label)
            .trailing_fill(true),
    );
    ui.visuals_mut().widgets.inactive.bg_fill = prev_inactive;
    resp
}


pub fn selectable_readonly_monospace(
    ui: &mut Ui,
    theme: &Theme,
    text: &str,
    font_size: f32,
    desired_width: f32,
) -> Response {
    let mut text_ref = text;
    ui.add(
        egui::TextEdit::multiline(&mut text_ref)
            .font(egui::FontId::monospace(font_size))
            .text_color(theme.text_secondary())
            .frame(egui::Frame::NONE)
            .margin(egui::vec2(0.0, 0.0))
            .desired_width(desired_width.max(1.0))
            .code_editor(),
    )
}


/// 搜索框内容行(🔍 + 单行输入)，两种外框共用。
pub(crate) fn search_field_contents(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    query: &mut String,
    hint: &str,
    font: f32,
    text_w: f32,
    max_w: f32,
) -> Response {
    ui.horizontal(|ui| {
        ui.set_max_width(max_w);
        ui.spacing_mut().item_spacing.x = theme.spacing_sm();
        let (r, _) = ui.allocate_exact_size(egui::vec2(font, font), egui::Sense::hover());
        icons::paint_icon(ui, r, IconId::Search, theme.text_tertiary(), font);
        let prev_override = ui.style_mut().visuals.override_text_color;
        ui.style_mut().visuals.override_text_color = Some(theme.color_form_hint());
        let response = ui.add(
            egui::TextEdit::singleline(query)
                .id(id)
                .frame(egui::Frame::NONE)
                .hint_text(hint_rich(theme, hint, font))
                .text_color(theme.color_text_input_text())
                .font(egui::FontId::proportional(font))
                .desired_width(text_w),
        );
        ui.style_mut().visuals.override_text_color = prev_override;
        response
    })
    .inner
}


/// 面板/对话框内搜索框(下划线式，跟随主题输入样式)。
pub fn search_field(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    query: &mut String,
    hint: &str,
    desired_width: f32,
) -> Response {
    let focused = ui.memory(|m| m.has_focus(id));
    // modern 主题是"透明底 + 底边线"：底边线由 form_singleline_field 补画，
    // 这里若不补，搜索框在色板上会完全没有边框也没有下划线，看着不像输入框。
    let underline = theme.uses_underline_inputs();
    let font = theme.font_size_control_input();
    let pad_y = theme.spacing_search_input_y();
    let pad_x = theme.spacing_search_input_x();
    let stroke = theme.stroke_width_panel();
    let row_h = font + pad_y * 2.0 + stroke * 2.0;
    let mut outer_w = desired_width;
    if ui.max_rect().width().is_finite() {
        outer_w = outer_w.min(ui.max_rect().width());
    }
    outer_w = outer_w.min(ui.available_width()).max(72.0);
    let text_w = (outer_w - font - theme.spacing_sm() - pad_x * 2.0 - stroke * 2.0).max(48.0);

    let out = ui.allocate_ui_with_layout(
        egui::vec2(outer_w, row_h),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            ui.set_width(outer_w);
            theme
                .frame_form_text_input(focused)
                .show(ui, |ui| {
                    ui.set_width(outer_w);
                    with_underline_field_visuals(ui, theme, |ui| {
                        search_field_contents(ui, theme, id, query, hint, font, text_w, outer_w)
                    })
                })
                .inner
        },
    );
    if underline {
        paint_form_field_underline(ui, theme, out.response.rect, focused);
    }
    out.inner
}


/// 面板内搜索框(实心底 + 描边，与「偏好设置」等表单输入框同一套样式)。
///
/// 尺寸/配色刻意与 [`form_singleline_field_boxed`] 对齐：底 `color_form_input_fill()`
/// (`#1e1e1e`)、1px 描边 `color_form_input_stroke()`(`#3c3c3c`)、圆角 `radius_search_input()`、
/// 行高 `size_control_btn_h().max(28.0)`；聚焦描边换强调色。区别只在左侧多了搜索图标、文字左对齐。
///
/// 描边宽度必须写死 1.0：`theme.stroke_width_panel()` 在 modern 调色板下是 0.0，
/// 用它画等于没描边，整个框会消失。填充也不要自制更亮的色，那样会和其它输入框不是一套。
pub fn search_field_boxed(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    query: &mut String,
    hint: &str,
    desired_width: f32,
) -> Response {
    let focused = ui.memory(|m| m.has_focus(id));
    let font = theme.font_size_control_input();
    let pad_y = theme.spacing_search_input_y();
    let pad_x = theme.spacing_search_input_x();
    let stroke = 1.0_f32;
    let row_h = theme.size_form_field_h();
    let mut outer_w = desired_width;
    if ui.max_rect().width().is_finite() {
        outer_w = outer_w.min(ui.max_rect().width());
    }
    outer_w = outer_w.min(ui.available_width()).max(72.0);
    let text_w = (outer_w - font - theme.spacing_sm() - pad_x * 2.0 - stroke * 2.0).max(40.0);

    let (rect, _) = ui.allocate_exact_size(egui::vec2(outer_w, row_h), egui::Sense::hover());
    let line = if focused {
        theme.accent_color()
    } else {
        theme.color_form_input_stroke()
    };
    ui.painter().rect(
        rect,
        theme.radius_search_input(),
        theme.color_form_input_fill(),
        egui::Stroke::new(stroke, line),
        egui::StrokeKind::Inside,
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + pad_x, rect.top() + pad_y),
        egui::pos2(rect.right() - pad_x, rect.bottom() - pad_y),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.set_min_size(inner.size());
        ui.set_max_size(inner.size());
        with_underline_field_visuals(ui, theme, |ui| {
            search_field_contents(ui, theme, id, query, hint, font, text_w, inner.width())
        })
    })
    .inner
}


/// 面板内搜索行：左右留白与侧栏一致；`content_w` 为面板正文宽(右 dock 须传入，避免 outer_margin 撑出裁切)。
pub fn panel_search_row(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    query: &mut String,
    hint: &str,
    content_w: Option<f32>,
) -> Response {
    let margin = if content_w.is_some() {
        egui::Margin {
            left: 0,
            right: 0,
            top: 4,
            bottom: 6,
        }
    } else {
        theme.spacing_sidebar_search_outer()
    };
    let inset_x = margin.left as f32 + margin.right as f32;
    let stroke_pad = theme.stroke_width_panel() * 2.0 + 1.0;
    let cap = content_w.unwrap_or_else(|| crate::ui::layout_util::set_width_to_available(ui));
    let search_w = (cap - inset_x - stroke_pad).max(72.0);
    egui::Frame::NONE
        .outer_margin(margin)
        .show(ui, |ui| search_field(ui, theme, id, query, hint, search_w))
        .inner
}


/// 数字框包进与单行输入相同的底+描边，字号与表单输入一致。
/// 若内层仍是 `DragValue`，会盖掉其 ↔ 拖动手势光标(应优先改用步进器)。
pub fn form_drag_value_field(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    add_field: impl FnOnce(&mut Ui) -> Response,
) -> Response {
    let focused = ui.memory(|m| m.has_focus(id));
    let underline = theme.uses_underline_inputs();
    let shown = theme
        .frame_form_text_input(focused)
        .show(ui, |ui| {
            with_underline_field_visuals(ui, theme, |ui| {
                let font = egui::FontId::proportional(theme.font_size_control_input());
                ui.style_mut()
                    .text_styles
                    .insert(egui::TextStyle::Body, font.clone());
                ui.style_mut()
                    .text_styles
                    .insert(egui::TextStyle::Button, font);
                add_field(ui)
            })
        });
    if underline {
        paint_form_field_underline(ui, theme, shown.response.rect, focused);
    }
    // DragValue 默认悬停为 ResizeHorizontal；偏好类数字框不应露出可拖暗示。
    shown.inner.on_hover_cursor(CursorIcon::Text)
}


/// 步进器中间的数字框：固定宽高，不随父布局被拉高。
pub(crate) fn form_stepper_value_field(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    width: f32,
    add_field: impl FnOnce(&mut Ui) -> Response,
) -> Response {
    let focused = ui.memory(|m| m.has_focus(id));
    let row_h = form_stepper_control_h(theme);
    // 与表单输入同套 token，避免步进数字框和正则/下拉各画一套描边。
    let stroke = if focused {
        egui::Stroke::new(1.0, theme.accent_color())
    } else {
        egui::Stroke::new(1.0, theme.color_form_input_stroke())
    };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, row_h), Sense::hover());
    ui.painter().rect(
        rect,
        theme.radius_card(),
        theme.color_form_input_fill(),
        stroke,
        egui::StrokeKind::Middle,
    );
    let inner = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 4.0, rect.top()),
        egui::vec2((width - 8.0).max(8.0), row_h),
    );
    let mut edit = ui
        .scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.set_min_size(inner.size());
            ui.set_max_size(inner.size());
            // 数字与 ±、表单标签同一档（12px），不要用 13px 正文把矮框撑满。
            let font = egui::FontId::proportional(theme.font_size_ui_control());
            ui.style_mut()
                .text_styles
                .insert(egui::TextStyle::Body, font.clone());
            ui.style_mut()
                .text_styles
                .insert(egui::TextStyle::Button, font);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                add_field(ui)
            })
            .inner
        })
        .inner;
    if edit.hovered() || ui.rect_contains_pointer(rect) {
        ui.ctx().set_cursor_icon(CursorIcon::Text);
    }
    edit = edit.on_hover_cursor(CursorIcon::Text);
    edit
}


pub(crate) fn form_stepper_btn(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    let side = form_stepper_btn_side(theme);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::click());
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    let (fill, text_c, _) = secondary_control_button_colors(theme, true, hovered, pressed);
    // 与 panel_action 次要按钮同用 radius_card；勿用 list_item 圆角（20px 方钮上会像圆形）。
    ui.painter().rect(
        rect,
        theme.radius_card(),
        fill,
        theme.color_control_secondary_stroke(true),
        egui::StrokeKind::Middle,
    );
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(theme.font_size_ui_control()),
        text_c,
    );
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        egui::Color32::WHITE,
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}


pub(crate) fn form_stepper_row<R>(
    ui: &mut Ui,
    theme: &Theme,
    field_w: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let row_h = form_stepper_control_h(theme);
    let btn = form_stepper_btn_side(theme);
    let gap = theme.spacing_xs();
    let total_w = btn + gap + field_w + gap + btn;
    ui.allocate_ui_with_layout(
        egui::vec2(total_w, row_h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_height(row_h);
            ui.spacing_mut().item_spacing.x = gap;
            add(ui)
        },
    )
    .inner
}


/// 整数步进器：`−` 数字框 `＋`。固定行高，点加减或框内键入。
pub fn form_u32_stepper(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    step: u32,
) -> Response {
    let min_v = *range.start();
    let max_v = *range.end();
    let step = step.max(1);
    let mut changed = false;
    let edit_buf_id = id.with("stepper_buf");
    let field_w = 48.0;

    let mut response = form_stepper_row(ui, theme, field_w, |ui| {
        if form_stepper_btn(ui, theme, "−").clicked() {
            let next = value.saturating_sub(step).max(min_v);
            if next != *value {
                *value = next;
                changed = true;
                ui.ctx().data_mut(|d| d.remove::<String>(edit_buf_id));
            }
        }

        let focused = ui.memory(|m| m.has_focus(id));
        let mut text = if focused {
            ui.ctx()
                .data(|d| d.get_temp::<String>(edit_buf_id))
                .unwrap_or_else(|| value.to_string())
        } else {
            value.to_string()
        };
        let edit = form_stepper_value_field(ui, theme, id, field_w, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut text)
                    .id(id)
                    .desired_width(field_w - 8.0)
                    .align(egui::Align2::CENTER_CENTER)
                    .align(egui::Align2::CENTER_CENTER)
                    .margin(egui::vec2(0.0, 0.0))
                    .font(egui::FontId::proportional(theme.font_size_ui_control()))
                    .frame(egui::Frame::NONE),
            )
        });
        if edit.has_focus() {
            ui.ctx()
                .data_mut(|d| d.insert_temp(edit_buf_id, text.clone()));
        }
        if edit.changed() {
            if let Ok(n) = text.trim().parse::<u32>() {
                let n = n.clamp(min_v, max_v);
                if n != *value {
                    *value = n;
                    changed = true;
                }
            }
        }
        if edit.lost_focus() {
            ui.ctx().data_mut(|d| d.remove::<String>(edit_buf_id));
            if let Ok(n) = text.trim().parse::<u32>() {
                let n = n.clamp(min_v, max_v);
                if n != *value {
                    *value = n;
                    changed = true;
                }
            }
        }

        if form_stepper_btn(ui, theme, "+").clicked() {
            let next = (*value).saturating_add(step).min(max_v);
            if next != *value {
                *value = next;
                changed = true;
                ui.ctx().data_mut(|d| d.remove::<String>(edit_buf_id));
            }
        }
        edit
    });

    if changed {
        response.mark_changed();
    }
    response
}


/// `u8` 步进器(如 KeepAlive 超时次数)。
pub fn form_u8_stepper(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    value: &mut u8,
    range: std::ops::RangeInclusive<u8>,
    step: u8,
) -> Response {
    let mut v = u32::from(*value);
    let r = form_u32_stepper(
        ui,
        theme,
        id,
        &mut v,
        u32::from(*range.start())..=u32::from(*range.end()),
        u32::from(step.max(1)),
    );
    *value = v as u8;
    r
}


/// `u16` 步进器(如端口)。
pub fn form_u16_stepper(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    value: &mut u16,
    range: std::ops::RangeInclusive<u16>,
    step: u16,
) -> Response {
    let mut v = u32::from(*value);
    let r = form_u32_stepper(
        ui,
        theme,
        id,
        &mut v,
        u32::from(*range.start())..=u32::from(*range.end()),
        u32::from(step.max(1)),
    );
    *value = v as u16;
    r
}


/// `f32` 步进器(如字号)；逻辑同 [`form_u32_stepper`]。
pub fn form_f32_stepper(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    suffix: &str,
) -> Response {
    let min_v = *range.start();
    let max_v = *range.end();
    let step = step.abs().max(0.01);
    let mut changed = false;
    let edit_buf_id = id.with("stepper_buf");
    let field_w = 64.0;

    let mut response = form_stepper_row(ui, theme, field_w, |ui| {
        if form_stepper_btn(ui, theme, "−").clicked() {
            let next = (*value - step).clamp(min_v, max_v);
            if (next - *value).abs() > f32::EPSILON {
                *value = next;
                changed = true;
                ui.ctx().data_mut(|d| d.remove::<String>(edit_buf_id));
            }
        }

        let focused = ui.memory(|m| m.has_focus(id));
        let mut text = if focused {
            ui.ctx()
                .data(|d| d.get_temp::<String>(edit_buf_id))
                .unwrap_or_else(|| format_stepper_f32(*value, suffix))
        } else {
            format_stepper_f32(*value, suffix)
        };
        let edit = form_stepper_value_field(ui, theme, id, field_w, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut text)
                    .id(id)
                    .desired_width(field_w - 8.0)
                    .align(egui::Align2::CENTER_CENTER)
                    .align(egui::Align2::CENTER_CENTER)
                    .margin(egui::vec2(0.0, 0.0))
                    .font(egui::FontId::proportional(theme.font_size_ui_control()))
                    .frame(egui::Frame::NONE),
            )
        });
        if edit.has_focus() {
            ui.ctx()
                .data_mut(|d| d.insert_temp(edit_buf_id, text.clone()));
        }
        if edit.changed() {
            let raw = text.trim().trim_end_matches(suffix).trim();
            if let Ok(n) = raw.parse::<f32>() {
                let n = n.clamp(min_v, max_v);
                if (n - *value).abs() > f32::EPSILON {
                    *value = n;
                    changed = true;
                }
            }
        }
        if edit.lost_focus() {
            ui.ctx().data_mut(|d| d.remove::<String>(edit_buf_id));
            let raw = text.trim().trim_end_matches(suffix).trim();
            if let Ok(n) = raw.parse::<f32>() {
                let n = n.clamp(min_v, max_v);
                if (n - *value).abs() > f32::EPSILON {
                    *value = n;
                    changed = true;
                }
            }
        }

        if form_stepper_btn(ui, theme, "+").clicked() {
            let next = (*value + step).clamp(min_v, max_v);
            if (next - *value).abs() > f32::EPSILON {
                *value = next;
                changed = true;
                ui.ctx().data_mut(|d| d.remove::<String>(edit_buf_id));
            }
        }
        edit
    });

    if changed {
        response.mark_changed();
    }
    response
}


/// 与 12px 标签齐的步进器边长。高于此的通用按钮收到 20px；更矮的按钮不再被步进器撑高。
pub(crate) fn stepper_control_h_from_btn(control_btn_h: f32) -> f32 {
    FORM_STEPPER_CONTROL_H.min(control_btn_h)
}

/// #130：步进器方钮 / 数字框目标高度（规格 20–22px 的下沿）。
const FORM_STEPPER_CONTROL_H: f32 = 20.0;

pub(crate) fn format_stepper_f32(v: f32, suffix: &str) -> String {
    let num = if (v - v.round()).abs() < 0.05 {
        format!("{}", v.round() as i32)
    } else {
        format!("{v:.1}")
    };
    if suffix.is_empty() {
        num
    } else {
        format!("{num}{suffix}")
    }
}

