//! 弹窗 / 侧栏标题与操作按钮的统一视觉（关闭 ×、收起 −、主次按钮）。
//! 颜色与尺寸均来自 [`Theme`]，本模块不硬编码样式。

use eframe::egui::{self, Button, Color32, Response, RichText, Ui};
use crate::ui::theme::Theme;

/// 关闭（弹窗、侧栏）
pub const GLYPH_CLOSE: &str = "×";
/// 收起（左侧连接栏、可折叠侧栏）
pub const GLYPH_COLLAPSE: &str = "−";

/// 无底色、无边框的图标按钮（关闭 / 收起）。
pub fn icon_button(ui: &mut Ui, theme: &Theme, glyph: &str, color: Color32) -> Response {
    ui.add(
        Button::new(
            RichText::new(glyph)
                .size(theme.size_icon_glyph())
                .color(color),
        )
        .fill(Color32::TRANSPARENT)
        .stroke(egui::Stroke::NONE)
        .frame(false),
    )
}

pub fn close_icon_button(ui: &mut Ui, theme: &Theme) -> Response {
    icon_button(ui, theme, GLYPH_CLOSE, theme.fg_high_a76()).on_hover_text("关闭")
}

pub fn collapse_icon_button(ui: &mut Ui, theme: &Theme) -> Response {
    icon_button(ui, theme, GLYPH_COLLAPSE, theme.fg_high_a51()).on_hover_text("收起")
}

pub fn modal_window_frame(theme: &Theme) -> egui::Frame {
    theme.frame_modal_window()
}

pub fn modal_content_frame(theme: &Theme) -> egui::Frame {
    theme.frame_modal_content()
}

/// 右侧 dock / 左侧连接栏外框：统一底色与内容区内边距。
pub fn region_panel_frame(theme: &Theme) -> egui::Frame {
    theme.frame_region_panel()
}

/// 弹窗标题行 + 分隔线；返回 `true` 表示点了关闭。
pub fn modal_header(ui: &mut Ui, theme: &Theme, title: &str, title_px: f32) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .size(title_px)
                .strong()
                .color(theme.color_section_title()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if close_icon_button(ui, theme).clicked() {
                close = true;
            }
        });
    });
    ui.add_space(theme.spacing_modal_header_after_title());
    ui.separator();
    ui.add_space(theme.spacing_modal_header_after_sep());
    close
}

/// 右侧 dock 标题行（标题 + 关闭 ×）。
pub fn side_panel_title_row(ui: &mut Ui, theme: &Theme, title: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.heading(RichText::new(title).color(theme.fg_high_color()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if close_icon_button(ui, theme).clicked() {
                close = true;
            }
        });
    });
    close
}

/// 侧栏小标题（如「命令片段」）+ 右侧关闭 ×。
pub fn side_panel_section_title(
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    title_color: Color32,
) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .size(theme.font_size_small())
                .strong()
                .color(title_color),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if close_icon_button(ui, theme).clicked() {
                close = true;
            }
        });
    });
    close
}

pub fn modal_secondary_button_widget<'a>(theme: &'a Theme, label: &'a str) -> Button<'a> {
    Button::new(
        RichText::new(label)
            .size(theme.font_size_normal())
            .color(theme.color_status_online_muted()),
    )
    .min_size(theme.vec2_modal_footer_secondary())
    .fill(Color32::TRANSPARENT)
    .stroke(egui::Stroke::NONE)
    .rounding(theme.radius_list_item())
}

pub fn modal_primary_button_widget<'a>(theme: &'a Theme, label: &'a str) -> Button<'a> {
    Button::new(
        RichText::new(label)
            .size(theme.font_size_normal())
            .color(theme.accent_color()),
    )
    .min_size(theme.vec2_modal_footer_primary())
    .fill(theme.accent_a89())
    .stroke(egui::Stroke::NONE)
    .rounding(theme.radius_list_item())
}

pub fn modal_danger_button_widget<'a>(theme: &'a Theme, label: &'a str) -> Button<'a> {
    Button::new(
        RichText::new(label)
            .size(theme.font_size_normal())
            .color(theme.red_color()),
    )
    .min_size(theme.vec2_modal_footer_secondary())
    .fill(Color32::TRANSPARENT)
    .stroke(egui::Stroke::NONE)
    .rounding(theme.radius_list_item())
}

pub fn modal_secondary_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    ui.add(modal_secondary_button_widget(theme, label))
}

pub fn modal_primary_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    ui.add(modal_primary_button_widget(theme, label))
}

pub fn modal_danger_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    ui.add(modal_danger_button_widget(theme, label))
}

/// 右对齐底栏：先 add 主操作，再 add 次操作（`right_to_left` 布局）。
pub fn modal_footer_actions<F>(ui: &mut Ui, theme: &Theme, add_buttons: F)
where
    F: FnOnce(&mut Ui, &Theme),
{
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            add_buttons(ui, theme);
        });
    });
}
