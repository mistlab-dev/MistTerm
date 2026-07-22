use super::*;

/// 顶栏菜单弹出层（§2：圆角、内边距、悬停色）
pub fn apply_menu_popup_style(ui: &mut Ui, theme: &Theme) {
    apply_popup_widget_visuals(&mut ui.style_mut().visuals, theme);
    ui.style_mut().spacing.button_padding = egui::vec2(10.0, 5.0);
    ui.style_mut().spacing.item_spacing = egui::vec2(4.0, 2.0);
}

/// 右键菜单 / 终端 Tab 菜单等（与顶栏菜单同色）
#[inline]
pub fn apply_context_menu_style(ui: &mut Ui, theme: &Theme) {
    apply_menu_popup_style(ui, theme);
}

/// 主题子菜单左侧勾选列（固定宽，与 [`menu_theme_item`] 成对使用）。
pub fn menu_theme_check_slot(ui: &mut Ui, theme: &Theme, selected: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
    if selected {
        icons::paint_icon(
            ui,
            rect,
            IconId::Check,
            theme.accent_color(),
            theme.font_size_menu_item(),
        );
    }
}

/// 视图菜单等开关项（无左侧 18px 勾选列，避免未选中时文字前大块空白）
pub fn menu_toggle_item(ui: &mut Ui, theme: &Theme, selected: bool, name: &str) -> egui::Response {
    ui.selectable_label(
        selected,
        RichText::new(name)
            .size(theme.font_size_menu_item())
            .color(if selected {
                theme.accent_color()
            } else {
                theme.text_secondary()
            }),
    )
}

/// 主题子菜单一行：勾选列 + 可选标签（选中项文字用 accent）。
pub fn menu_theme_item(ui: &mut Ui, theme: &Theme, selected: bool, name: &str) -> egui::Response {
    ui.horizontal(|ui| {
        menu_theme_check_slot(ui, theme, selected);
        let label = egui::RichText::new(name)
            .size(theme.font_size_menu_item())
            .color(if selected {
                theme.accent_color()
            } else {
                theme.text_secondary()
            });
        ui.selectable_label(selected, label)
    })
    .inner
}

/// 顶栏 / 菜单项文字（可选快捷键后缀；仅用于无布局需求的简单标签）
pub fn menu_item_label(theme: &Theme, title: &str, shortcut: Option<&str>) -> RichText {
    let text = if let Some(sc) = shortcut {
        format!("{}  {}", title, sc)
    } else {
        title.to_string()
    };
    RichText::new(text)
        .size(theme.font_size_menu_item())
        .color(theme.text_secondary())
}

fn layout_menu_line(ui: &Ui, text: &str, px: f32, color: Color32) -> std::sync::Arc<egui::Galley> {
    let font_id = egui::FontId::proportional(px);
    ui.fonts(|fonts| fonts.layout_no_wrap(text.to_owned(), font_id, color))
}

/// 测量下拉菜单行内容宽（标题 + 可选快捷键）
pub fn measure_popup_menu_row_width(
    ui: &Ui,
    theme: &Theme,
    title: &str,
    shortcut: Option<&str>,
) -> f32 {
    let px = theme.font_size_menu_item();
    let pad = ui.spacing().button_padding;
    let title_w = layout_menu_line(ui, title, px, Color32::WHITE).size().x;
    let shortcut_w = shortcut
        .map(|s| layout_menu_line(ui, s, px, Color32::WHITE).size().x)
        .unwrap_or(0.0);
    let gap = if shortcut.is_some() {
        theme.spacing_menu_shortcut_gap()
    } else {
        0.0
    };
    pad.x * 2.0 + title_w + gap + shortcut_w
}

/// 统一菜单弹出层宽度，避免无快捷键项悬停背景只铺半行
pub fn prime_menu_popup_width(ui: &mut Ui, min_content_width: f32) {
    if min_content_width.is_finite() && min_content_width > 0.0 {
        ui.set_min_width(min_content_width);
    }
}

/// 下拉菜单行：标题居左、快捷键居右（整行可点）
pub fn popup_menu_button_shortcut_enabled(
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    shortcut: Option<&str>,
    enabled: bool,
) -> Response {
    let px = theme.font_size_menu_item();
    let pad = ui.spacing().button_padding;
    let title_color = if enabled {
        theme.text_secondary()
    } else {
        theme.text_tertiary()
    };
    let title_g = layout_menu_line(ui, title, px, title_color);
    let shortcut_w = shortcut
        .map(|s| layout_menu_line(ui, s, px, theme.text_tertiary()).size().x)
        .unwrap_or(0.0);
    let gap = if shortcut.is_some() {
        theme.spacing_menu_shortcut_gap()
    } else {
        0.0
    };
    let content_w = pad.x * 2.0 + title_g.size().x + gap + shortcut_w;
    let row_w = ui.available_width().max(content_w);
    let content_h = title_g.size().y;
    let row_h = (content_h + pad.y * 2.0).max(ui.spacing().interact_size.y);

    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(row_w, row_h), sense);

    if ui.is_enabled() {
        let visuals = ui.style().interact_selectable(&response, enabled);
        if enabled && (response.hovered() || response.has_focus()) {
            ui.painter().rect_filled(rect, 0.0, visuals.bg_fill);
        }
        let title_pos = egui::pos2(rect.min.x + pad.x, rect.center().y - title_g.size().y * 0.5);
        ui.painter().galley(title_pos, title_g);
        if let Some(sc) = shortcut {
            let sc_color = if enabled {
                theme.text_tertiary()
            } else {
                theme.color_form_hint()
            };
            let sg = layout_menu_line(ui, sc, px, sc_color);
            let shortcut_pos = egui::pos2(
                rect.max.x - pad.x - sg.size().x,
                rect.center().y - sg.size().y * 0.5,
            );
            ui.painter().galley(shortcut_pos, sg);
        }
    }

    if response.hovered() && enabled && ui.is_enabled() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    response
}

/// 下拉菜单行（默认可点）
pub fn popup_menu_button_shortcut(
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    shortcut: Option<&str>,
) -> Response {
    popup_menu_button_shortcut_enabled(ui, theme, title, shortcut, true)
}

/// 菜单项 + 当前平台主修饰键快捷键（`⌘ + n` / `Ctrl + n`）。
pub fn popup_menu_button_accel(ui: &mut Ui, theme: &Theme, title: &str, key: &str) -> Response {
    let shortcut = crate::platform::accel(key);
    popup_menu_button_shortcut(ui, theme, title, Some(&shortcut))
}

/// 菜单项 + `⌘ + Shift + j` / `Ctrl + Shift + j`。
pub fn popup_menu_button_accel_shift(
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    key: &str,
) -> Response {
    let shortcut = crate::platform::accel_shift(key);
    popup_menu_button_shortcut(ui, theme, title, Some(&shortcut))
}

/// 菜单项 + 当前平台主修饰键快捷键（`⌘ + n` / `Ctrl + n`）— 仅文案，供旧调用。
pub fn menu_item_label_accel(theme: &Theme, title: &str, key: &str) -> RichText {
    let shortcut = crate::platform::accel(key);
    menu_item_label(theme, title, Some(&shortcut))
}

/// 菜单项 + `⌘ + Shift + j` / `Ctrl + Shift + j` — 仅文案，供旧调用。
pub fn menu_item_label_accel_shift(theme: &Theme, title: &str, key: &str) -> RichText {
    let shortcut = crate::platform::accel_shift(key);
    menu_item_label(theme, title, Some(&shortcut))
}

/// 弹出菜单 / 右键 / Tab 菜单项（与顶栏菜单同字号，非面板灰钮）
pub fn popup_menu_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    popup_menu_button_shortcut(ui, theme, label, None)
}

/// 带启用态的弹出菜单项
pub fn popup_menu_button_enabled(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    enabled: bool,
) -> Response {
    popup_menu_button_shortcut_enabled(ui, theme, label, None, enabled)
}
