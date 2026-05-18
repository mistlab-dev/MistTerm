//! 弹窗 / 侧栏标题与操作按钮的统一视觉（关闭 ×、侧栏 ◀ 收起、主次按钮）。
//! 颜色与尺寸均来自 [`Theme`]，本模块不硬编码样式。

use eframe::egui::{self, Button, Color32, CursorIcon, Response, RichText, Sense, Ui, Widget};
use crate::ui::theme::Theme;

/// 关闭（弹窗、右 dock、终端搜索条等）
pub const GLYPH_CLOSE: &str = "×";
/// 收起左侧连接栏（勿用「−」，避免与「＋ 新建」形成加减歧义）
pub const GLYPH_SIDEBAR_COLLAPSE: &str = "◀";
/// 新建终端 Tab
pub const GLYPH_TAB_NEW: &str = "+";

/// 方形图标点击区（× / − / ＋ 等）：自绘悬停底，勿用 `Button::fill(TRANSPARENT)`。
fn icon_hit_button(
    ui: &mut Ui,
    theme: &Theme,
    glyph: &str,
    hit_size: f32,
    font_size: f32,
    idle_color: Color32,
    hover_color: Color32,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(hit_size, hit_size), Sense::click());
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    if hovered || pressed {
        ui.ctx().request_repaint();
    }
    if hovered || pressed {
        let fill = if pressed {
            theme.accent_alpha(45)
        } else {
            theme.color_tab_bar_icon_btn_hover_fill()
        };
        ui.painter()
            .rect_filled(rect, theme.radius_list_item(), fill);
    }
    let color = if hovered || pressed {
        hover_color
    } else {
        idle_color
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(font_size),
        color,
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// Tab 栏图标按钮（× 关闭 / ＋ 新建）：固定点击区、悬停底、可读字色。
pub fn tab_bar_icon_button(ui: &mut Ui, theme: &Theme, glyph: &str, tooltip: &str) -> Response {
    icon_hit_button(
        ui,
        theme,
        glyph,
        theme.size_tab_bar_icon_btn(),
        theme.font_size_tab_bar_icon(),
        theme.color_tab_bar_icon(),
        theme.color_tab_bar_icon_hover(),
    )
    .on_hover_text(tooltip)
}

/// 标签栏「新建 Tab」按钮（与 Tab 芯片同高、垂直居中）
pub fn tab_bar_new_tab_button(ui: &mut Ui, theme: &Theme) -> Response {
    let row_h = theme.size_tab_bar_row_h();
    let icon = theme.size_tab_bar_icon_btn();
    ui.allocate_ui_with_layout(
        egui::vec2(icon, row_h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            tab_bar_icon_button(
                ui,
                theme,
                GLYPH_TAB_NEW,
                "新标签：左侧选中连接后点此或 ⌘T；无选中时打开新建会话配置",
            )
        },
    )
    .inner
}

/// 通用图标按钮（可指定 idle 色）
pub fn icon_button(ui: &mut Ui, theme: &Theme, glyph: &str, color: Color32) -> Response {
    icon_hit_button(
        ui,
        theme,
        glyph,
        theme.size_panel_header_control_h(),
        theme.size_icon_glyph(),
        color,
        theme.fg_high_color(),
    )
}

/// 弹窗 / 侧栏标题栏关闭 ×
pub fn close_icon_button(ui: &mut Ui, theme: &Theme) -> Response {
    icon_hit_button(
        ui,
        theme,
        GLYPH_CLOSE,
        theme.size_panel_header_control_h(),
        theme.size_icon_glyph(),
        theme.color_sidebar_header_icon(),
        theme.fg_high_color(),
    )
    .on_hover_text("关闭")
}

/// 侧栏标题行方形图标按钮（与排序下拉同高）。
pub fn sidebar_header_icon_button(ui: &mut Ui, theme: &Theme, glyph: &str, color: Color32) -> Response {
    icon_hit_button(
        ui,
        theme,
        glyph,
        theme.size_sidebar_header_icon(),
        theme.font_size_sidebar_icon_glyph(),
        color,
        theme.fg_high_color(),
    )
}

/// 侧栏「＋」新建会话（浅紫底，与「◀ 收起」区分）
pub fn sidebar_new_session_button(ui: &mut Ui, theme: &Theme) -> Response {
    let size = egui::vec2(
        theme.size_sidebar_header_icon(),
        theme.size_sidebar_header_icon(),
    );
    let rounding = theme.radius_list_item();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    if hovered || pressed {
        ui.ctx().request_repaint();
    }
    let fill = if pressed {
        theme.accent_alpha(64)
    } else if hovered {
        theme.accent_alpha(51)
    } else {
        theme.accent_alpha(38)
    };
    ui.painter().rect(rect, rounding, fill, egui::Stroke::NONE);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "＋",
        egui::FontId::proportional(theme.font_size_sidebar_icon_glyph()),
        theme.accent_color(),
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// 小号文字按钮（替换 `small_button`，带悬停底）
pub fn chrome_small_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    text_hit_button(
        ui,
        theme,
        label,
        theme.font_size_panel_title(),
        theme.color_modal_secondary_text(),
        theme.fg_high_color(),
        egui::vec2(6.0, 3.0),
    )
}

/// 强调色小号文字按钮（如 SSH 导入条「导入」）
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

fn text_hit_button(
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
    if hovered || pressed {
        ui.painter().rect_filled(
            rect,
            theme.radius_list_item(),
            if pressed {
                theme.accent_alpha(51)
            } else {
                theme.color_panel_toolbar_btn_fill()
            },
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
    ui.painter().galley(rect.min + padding, galley);
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// 侧栏排序下拉（固定宽、菜单不换行）
pub fn sidebar_sort_combo(ui: &mut Ui, theme: &Theme, sort_by: &mut crate::core::session_sort::SessionSortBy) {
    use crate::core::session_sort::SessionSortBy;
    let w = theme.size_sidebar_sort_combo_w();
    let h = theme.size_sidebar_header_control_h();
    ui.allocate_ui_with_layout(
        egui::vec2(w, h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            egui::ComboBox::from_id_source("session_sort_by")
                .selected_text(
                    RichText::new(sort_by.short_label())
                        .size(theme.font_size_sidebar_control())
                        .color(theme.fg_medium_color()),
                )
                .width(w)
                .show_ui(ui, |ui| {
                    apply_sidebar_menu_popup_style(ui, theme);
                    ui.set_min_width(w);
                    for mode in SessionSortBy::ALL {
                        let resp = ui.selectable_label(
                            *sort_by == *mode,
                            RichText::new(mode.label())
                                .size(theme.font_size_sidebar_control())
                                .color(if *sort_by == *mode {
                                    theme.fg_high_color()
                                } else {
                                    theme.fg_medium_color()
                                }),
                        );
                        if resp.clicked() {
                            *sort_by = *mode;
                        }
                    }
                });
        },
    );
}

/// 下拉 / 右键 / ComboBox 弹出层共用的控件色（含 `widgets.open`，避免子菜单发黑底）。
pub fn apply_popup_widget_visuals(visuals: &mut egui::Visuals, theme: &Theme) {
    visuals.widgets.inactive.bg_fill = theme.bg_window_color();
    visuals.widgets.hovered.bg_fill = theme.accent_alpha(38);
    visuals.widgets.active.bg_fill = theme.accent_alpha(64);
    visuals.widgets.inactive.fg_stroke.color = theme.fg_medium_color();
    visuals.widgets.hovered.fg_stroke.color = theme.fg_high_color();
    let open = &mut visuals.widgets.open;
    open.weak_bg_fill = theme.accent_alpha(38);
    open.bg_fill = theme.accent_alpha(38);
    open.bg_stroke = egui::Stroke::NONE;
    open.fg_stroke.color = theme.fg_high_color();
    visuals.selection.bg_fill = theme.color_text_selection_bg();
    visuals.selection.stroke.color = theme.color_text_selection_fg();
}

fn apply_sidebar_menu_popup_style(ui: &mut Ui, theme: &Theme) {
    apply_popup_widget_visuals(&mut ui.style_mut().visuals, theme);
    ui.style_mut().spacing.button_padding = egui::vec2(12.0, 6.0);
    ui.style_mut().spacing.item_spacing = egui::vec2(0.0, 2.0);
    ui.style_mut().spacing.indent = 0.0;
}

pub fn modal_window_frame(theme: &Theme) -> egui::Frame {
    theme.frame_modal_window()
}

pub fn modal_content_frame(theme: &Theme) -> egui::Frame {
    theme.frame_modal_content()
}

/// 居中弹窗标题字号（与新建会话等一致）
pub fn modal_title_font_size(theme: &Theme) -> f32 {
    theme.font_size_fragment_dialog_body()
}

/// 标准弹窗 `Window`：无系统标题栏、不可折叠、统一外框（须再 `.open()` / `.show()` / 尺寸）
pub fn modal_window<'a>(window_id: &'a str, theme: &Theme) -> egui::Window<'a> {
    egui::Window::new(window_id)
        .title_bar(false)
        .collapsible(false)
        .frame(modal_window_frame(theme))
}

/// 右侧 dock / 左侧连接栏外框：统一底色与内容区内边距。
pub fn region_panel_frame(theme: &Theme) -> egui::Frame {
    theme.frame_region_panel()
}

/// 左缘略向左扩 2px，盖住 Central `bg_body` 可能压到侧栏左缘的细缝。
pub const RIGHT_DOCK_SHELL_LEFT_BLEED: f32 = 2.0;

/// 右 dock Foreground：先铺满整个槽位（`Frame` 仅包住内容时左侧会透出中央 `bg_body`）。
pub fn paint_right_dock_slot_shell(ui: &mut egui::Ui, slot: egui::Rect, theme: &Theme) {
    let mut fill = slot;
    fill.min.x -= RIGHT_DOCK_SHELL_LEFT_BLEED;
    let rounding = egui::Rounding::same(theme.radius_panel());
    ui.painter()
        .rect_filled(fill, rounding, theme.color_panel_surface());
    ui.painter().rect_stroke(
        slot,
        rounding,
        egui::Stroke::new(1.0, theme.border_color()),
    );
}

/// 槽位扣除 region panel 内边距后的内容矩形（须用 `Margin::shrink_rect`，勿 `shrink2(left+right)`）。
#[inline]
pub fn right_dock_slot_content_rect(slot: egui::Rect, theme: &Theme) -> egui::Rect {
    theme.region_content_margin().shrink_rect(slot)
}

/// Central 之后 Foreground 重绘右 dock 用的 `Area`。
///
/// 必须 `interactable(false)`：默认 `true` 时 egui 会把整块槽位登记为可点层，
/// `layer_id_at` 在槽位矩形内优先于 Background 的侧栏/终端，导致左侧无法操作。
/// 面板内按钮/输入仍由子控件各自响应点击。
pub fn right_dock_foreground_area(id: &'static str) -> egui::Area {
    egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Middle)
        .movable(false)
        .interactable(false)
        .constrain(true)
}

/// 标题栏连接区展示数据（§三）
#[derive(Clone)]
pub struct TitleBarConnection {
    pub server_text: String,
    pub status_label: String,
    pub online: bool,
    pub connecting: bool,
}

fn paint_top_strip(ui: &mut Ui, rect: egui::Rect, theme: &Theme) {
    ui.painter().rect_filled(rect, 0.0, theme.chrome_bar_fill());
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 1.0,
        egui::Stroke::new(1.0, theme.border_divider_color()),
    );
}

/// 顶栏：仅菜单行（连接信息在 Tab / 底栏，避免与顶栏重复）
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
    ui.allocate_ui_at_rect(rect, |ui| {
        ui.set_clip_rect(rect);
        let content_h = h;
        ui.set_min_height(content_h);
        ui.style_mut().spacing.interact_size.y = content_h;
        egui::menu::bar(ui, |ui| {
            if show_in_window_menu {
                ui.spacing_mut().item_spacing.x = theme.spacing_menu_bar_gap();
                ui.add_space(theme.spacing_menu_bar_left());
                menu_bar_brand(ui, theme);
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

fn ssh_import_chip_actions(
    ui: &mut Ui,
    theme: &Theme,
    pending_ssh_imports: usize,
) -> TitleBarChromeResult {
    let mut out = TitleBarChromeResult::default();
    if close_icon_button(ui, theme)
        .on_hover_text("关闭导入提示")
        .clicked()
    {
        out.dismiss_ssh_import = true;
    }
    ui.add_space(theme.spacing_sm());
    let chip_label = format!("⚡ {} 个待导入", pending_ssh_imports);
    if ui
        .scope(|ui| {
            let w = &mut ui.style_mut().visuals.widgets;
            w.inactive.weak_bg_fill = theme.color_overlay_fill_subtle();
            w.hovered.weak_bg_fill = theme.accent_alpha(25);
            ui.add(
                Button::new(
                    RichText::new(&chip_label)
                        .size(theme.font_size_title_bar_info())
                        .color(theme.amber_color()),
                )
                .rounding(4.0),
            )
        })
        .inner
        .clicked()
    {
        out.open_ssh_import = true;
    }
    out
}

/// 菜单行左侧品牌（macOS 系统标题栏已显示应用名，不再重复）
#[cfg(not(target_os = "macos"))]
pub fn menu_bar_brand(ui: &mut Ui, theme: &Theme) {
    ui.label(
        RichText::new("✦ Mist")
            .size(theme.font_size_menu_item())
            .color(theme.fg_low_color()),
    );
}

#[cfg(target_os = "macos")]
pub fn menu_bar_brand(_ui: &mut Ui, _theme: &Theme) {}

/// 顶栏菜单行上的 SSH 导入 chip 等动作
#[derive(Default)]
pub struct TitleBarChromeResult {
    pub open_ssh_import: bool,
    pub dismiss_ssh_import: bool,
}

/// 终端区会话 Tab：整块底色（圆点 + 标题 + 关闭），对齐 proto `.tab`。
pub struct SessionTabChipResult {
    pub response: Response,
    pub close_clicked: bool,
}

pub fn session_tab_chip(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    active: bool,
    online: bool,
    show_close: bool,
) -> SessionTabChipResult {
    let size = egui::vec2(theme.size_tab_min_w(), theme.size_tab_min_h());
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let hovered = response.hovered();
    let show_close = show_close || active || hovered;
    let fill = if active {
        theme.color_tab_active_fill()
    } else if hovered {
        theme.color_tab_inactive_hover_fill()
    } else {
        egui::Color32::TRANSPARENT
    };
    let rounding = egui::Rounding {
        nw: theme.radius_list_item(),
        ne: theme.radius_list_item(),
        sw: 0.0,
        se: 0.0,
    };
    let stroke = if active {
        egui::Stroke::new(1.0, theme.color_tab_stroke())
    } else {
        egui::Stroke::NONE
    };
    ui.painter().rect(rect, rounding, fill, stroke);
    if active {
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.bottom() - 2.0),
            rect.right_bottom(),
        );
        ui.painter().rect_filled(bar, 0.0, theme.accent_color());
    }
    let mut close_clicked = false;
    let inner = rect.shrink2(egui::vec2(
        theme.spacing_tab_x(),
        theme.spacing_tab_y(),
    ));
    let mut row_ui = ui.child_ui(inner, egui::Layout::left_to_right(egui::Align::Center));
    row_ui.set_width(inner.width());
    row_ui.set_min_width(inner.width());
    row_ui.set_max_width(inner.width());
    row_ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_tab_dot_text();
        let dot_color = if online {
            theme.green_color()
        } else {
            theme.color_tab_offline_dot()
        };
        let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(5.0, 5.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(dot_rect.center(), 2.5, dot_color);
        ui.label(
            RichText::new(label)
                .size(theme.font_size_tab_label())
                .color(if active {
                    theme.fg_high_color()
                } else {
                    theme.fg_low_color()
                }),
        );
        if show_close {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if tab_bar_icon_button(ui, theme, GLYPH_CLOSE, "关闭标签 · ⌘W").clicked() {
                    close_clicked = true;
                }
            });
        }
    });
    SessionTabChipResult {
        response,
        close_clicked,
    }
}

/// 会话列表选中行左侧 3px 强调条（§4.4）
pub fn paint_sidebar_selection_accent(
    painter: &egui::Painter,
    row_rect: egui::Rect,
    theme: &Theme,
) {
    let bar = egui::Rect::from_min_max(
        row_rect.left_top(),
        egui::pos2(row_rect.left() + 3.0, row_rect.bottom()),
    );
    painter.rect_filled(bar, 0.0, theme.accent_color());
}

/// 侧栏 / 右 dock 标题行次要工具按钮（浅底 + 描边；宽度按文字测量）。
pub fn panel_toolbar_button_widget<'a>(theme: &'a Theme, text: RichText) -> Button<'a> {
    Button::new(text)
        .fill(theme.color_panel_toolbar_btn_fill())
        .stroke(egui::Stroke::new(1.0, theme.border_divider_color()))
        .rounding(theme.radius_list_item())
}

fn panel_toolbar_button_size(ui: &Ui, theme: &Theme, label: &str) -> egui::Vec2 {
    let h = theme.size_panel_header_control_h();
    let pad_x = theme.spacing_panel_header_btn_pad_x();
    let font = egui::FontId::proportional(theme.font_size_panel_header_control());
    let text_w = ui
        .painter()
        .layout_no_wrap(
            label.to_owned(),
            font,
            theme.color_body_text_muted(),
        )
        .size()
        .x;
    let w = (text_w + 2.0 * pad_x).max(theme.size_panel_header_btn_min_w());
    egui::vec2(w, h)
}

pub fn panel_toolbar_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    let size = panel_toolbar_button_size(ui, theme, label);
    let rounding = theme.radius_list_item();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    if hovered || pressed {
        ui.ctx().request_repaint();
    }
    let fill = if pressed {
        theme.accent_alpha(38)
    } else if hovered {
        theme.color_panel_toolbar_btn_fill().gamma_multiply(1.35)
    } else {
        theme.color_panel_toolbar_btn_fill()
    };
    let stroke = egui::Stroke::new(
        1.0,
        if hovered || pressed {
            theme.accent_alpha(51)
        } else {
            theme.border_divider_color()
        },
    );
    ui.painter().rect(rect, rounding, fill, stroke);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(theme.font_size_panel_header_control()),
        theme.color_body_text_muted(),
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// 右 dock / 侧栏标题行样式
#[derive(Clone, Copy)]
pub enum DockPanelTitleStyle {
    /// 片段面板等：小号加粗 section 色
    Section { color: Color32 },
    /// 凭证库、云端同步等：标准 dock 标题
    DockHeading,
    /// 监控等大号标题
    LargeHeading,
}

fn dock_panel_title_rich_text(theme: &Theme, title: &str, style: DockPanelTitleStyle) -> RichText {
    match style {
        DockPanelTitleStyle::Section { color } => rich_section_title(theme, title, color),
        DockPanelTitleStyle::DockHeading => rich_section_title(theme, title, theme.fg_high_color()),
        DockPanelTitleStyle::LargeHeading => rich_dock_title(theme, title),
    }
}

/// 区段标题（侧栏「连接」、右 dock「命令片段」、凭证库标题等）— 11px 加粗
pub fn rich_section_title(theme: &Theme, text: &str, color: Color32) -> RichText {
    RichText::new(text)
        .size(theme.font_size_section_title())
        .strong()
        .color(color)
}

/// 右 dock 大标题（系统监控、Git 同步）— 14px
pub fn rich_dock_title(theme: &Theme, text: &str) -> RichText {
    RichText::new(text)
        .size(theme.font_size_dock_title())
        .strong()
        .color(theme.fg_high_color())
}

/// 表单字段标签 — 11px
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
        .color(theme.fg_high_color())
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

/// 标题行右侧操作区宽度（工具按钮 + 关闭 ×；RTL 顺序为 close, …tools）
pub fn panel_header_trailing_width(ui: &Ui, theme: &Theme, tool_labels: &[&str]) -> f32 {
    let close_w = theme.size_panel_header_control_h();
    let gap = theme.spacing_panel_gap();
    let pad = theme.spacing_panel_title_pad_x() * 0.5;
    if tool_labels.is_empty() {
        return close_w + pad;
    }
    let tools_w: f32 = tool_labels
        .iter()
        .map(|l| panel_toolbar_button_size(ui, theme, l).x)
        .sum();
    tools_w + gap * tool_labels.len() as f32 + close_w + pad
}

/// 右 dock / 侧栏统一标题行：左侧标题区（可截断），右侧 RTL 操作区
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
        ui.with_layout(
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.set_min_width(trailing_width);
                ui.spacing_mut().item_spacing.x = theme.spacing_panel_gap();
                closed = draw_trailing(ui, theme);
            },
        );
    });
    closed
}

fn dock_panel_title_close_trailing(
    ui: &mut Ui,
    theme: &Theme,
    close_tooltip: &str,
) -> bool {
    close_icon_button(ui, theme)
        .on_hover_text(close_tooltip)
        .clicked()
}

/// 仅标题 + 关闭 ×
pub fn dock_panel_title_close_only(
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    style: DockPanelTitleStyle,
    close_tooltip: &str,
) -> bool {
    let trailing_w = panel_header_trailing_width(ui, theme, &[]);
    let title = title.to_string();
    dock_panel_title_row(
        ui,
        theme,
        |ui| {
            ui.label(dock_panel_title_rich_text(theme, &title, style));
        },
        close_tooltip,
        trailing_w,
        |ui, theme| dock_panel_title_close_trailing(ui, theme, close_tooltip),
    )
}

/// 命令片段等：标题 + 工具按钮 + 关闭
pub struct DockPanelHeaderActions {
    pub closed: bool,
    pub new_fragment: bool,
    pub cycle_sort: bool,
}

pub fn dock_panel_title_bar(
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    title_color: Color32,
    sort_label: &str,
    new_label: &str,
    close_tooltip: &str,
) -> DockPanelHeaderActions {
    let mut out = DockPanelHeaderActions {
        closed: false,
        new_fragment: false,
        cycle_sort: false,
    };
    let trailing_w = panel_header_trailing_width(ui, theme, &[new_label, sort_label]);
    let title = title.to_string();
    let closed = dock_panel_title_row(
        ui,
        theme,
        |ui| {
            ui.label(dock_panel_title_rich_text(
                theme,
                &title,
                DockPanelTitleStyle::Section { color: title_color },
            ));
        },
        close_tooltip,
        trailing_w,
        |ui, theme| {
            let mut closed = false;
            if dock_panel_title_close_trailing(ui, theme, close_tooltip) {
                closed = true;
            }
            if panel_toolbar_button(ui, theme, new_label)
                .on_hover_text("打开片段库：自建命令、占位符与变量")
                .clicked()
            {
                out.new_fragment = true;
            }
            if panel_toolbar_button(ui, theme, sort_label).clicked() {
                out.cycle_sort = true;
            }
            closed
        },
    );
    out.closed = closed;
    out
}

/// 命令片段侧栏列表行入参
pub struct FragmentListRow<'a> {
    pub title: &'a str,
    pub command: &'a str,
    pub stats_line: &'a str,
    pub tag_label: &'a str,
}

/// 命令片段列表行交互结果
pub struct FragmentListRowResponse {
    pub row: Response,
    pub title: Response,
}

/// 按面板可用宽度与标签文字测量，分配主栏/标签列宽（无裸像素魔法数）。
fn fragment_list_row_column_widths(
    ui: &Ui,
    theme: &Theme,
    tag_label: &str,
    content_w: f32,
) -> (f32, f32) {
    let gap = theme.spacing_fragment_row_tag_gap();
    let main_min = theme.size_fragment_list_main_min_w();
    let tag_pad = theme.spacing_fragment_tag_inner_x();
    let tag_font = egui::FontId::proportional(theme.font_size_tag());
    let tag_color = theme.color_fragment_tag_text();
    let tag_text_w = ui
        .painter()
        .layout_no_wrap(tag_label.to_owned(), tag_font, tag_color)
        .size()
        .x;
    let tag_w_desired = tag_text_w + 2.0 * tag_pad;
    let tag_cap = content_w * theme.fragment_list_tag_max_width_frac();
    let tag_budget = (content_w - gap - main_min).max(0.0);
    let tag_w = tag_w_desired.min(tag_cap).min(tag_budget);
    let main_w = (content_w - gap - tag_w).max(0.0);
    (main_w, tag_w)
}

/// 命令片段侧栏单行（标题 + 命令 + 统计 + 右侧分类/标签，宽度自适应）。
pub fn fragment_list_row(ui: &mut Ui, theme: &Theme, row: FragmentListRow<'_>) -> FragmentListRowResponse {
    let pad_x = theme.spacing_fragment_row_pad_x();
    let pad_y = theme.spacing_fragment_row_pad_y();
    let gap = theme.spacing_fragment_row_tag_gap();
    let line_gap = theme.spacing_fragment_row_line_gap();

    let row_w = ui.available_width();
    let content_w = (row_w - 2.0 * pad_x).max(0.0);
    let (main_w, tag_w) = fragment_list_row_column_widths(ui, theme, row.tag_label, content_w);
    let row_h = theme.size_fragment_list_row_min_h();

    let (row_rect, row_response) =
        ui.allocate_at_least(egui::vec2(row_w, row_h), egui::Sense::click());
    let bg = if row_response.hovered() {
        theme.list_row_hover_bg()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(row_rect, theme.radius_card(), bg);

    let inner = egui::Margin::symmetric(pad_x, pad_y).shrink_rect(row_rect);
    let mut row_ui = ui.child_ui(inner, egui::Layout::left_to_right(egui::Align::TOP));
    row_ui.set_max_width(content_w);
    let title_resp = row_ui.horizontal(|ui| {
        ui.set_max_width(content_w);
        ui.spacing_mut().item_spacing.x = gap;
        let title = ui.vertical(|ui| {
            ui.set_max_width(main_w);
            ui.set_min_width(0.0);
            ui.spacing_mut().item_spacing.y = line_gap;

            let title = ui
                .add(
                    egui::Label::new(
                        RichText::new(row.title)
                            .size(theme.font_size_fragment_title())
                            .color(theme.accent_color()),
                    )
                    .sense(egui::Sense::click()),
                )
                .on_hover_text(row.command);

            let cmd_trim = row.command.trim();
            ui.add(
                egui::Label::new(
                    RichText::new(cmd_trim)
                        .size(theme.font_size_fragment_cmd())
                        .monospace()
                        .color(theme.color_status_bar_conn()),
                )
                .truncate(true),
            )
            .on_hover_text(cmd_trim);

            ui.add(
                egui::Label::new(
                    RichText::new(row.stats_line)
                        .size(theme.font_size_fragment_stats())
                        .color(theme.color_caption_text()),
                )
                .truncate(true),
            );
            title
        });
        ui.allocate_ui_with_layout(
            egui::vec2(tag_w.max(0.0), inner.height()),
            egui::Layout::top_down(egui::Align::RIGHT),
            |ui| {
                ui.set_max_width(tag_w.max(0.0));
                ui.add(
                    egui::Label::new(
                        RichText::new(row.tag_label)
                            .size(theme.font_size_tag())
                            .color(theme.color_fragment_tag_text()),
                    )
                    .truncate(true),
                )
                .on_hover_text(row.tag_label);
            },
        );
        title.inner
    })
    .inner;

    FragmentListRowResponse {
        row: row_response,
        title: title_resp,
    }
}

/// 均分宽度的筛选芯片行（常用/Docker、全部/在线/离线等）
pub fn filter_chip_row(
    ui: &mut Ui,
    theme: &Theme,
    labels: &[&str],
    active: &str,
    panel_w: f32,
) -> Option<String> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.set_max_width(panel_w);
        let chip_h = theme.size_panel_filter_chip_h();
        let chip_gap = theme.spacing_panel_gap();
        ui.spacing_mut().item_spacing = egui::vec2(chip_gap, 0.0);
        let n = labels.len().max(1) as f32;
        let item_w = ((ui.available_width() - chip_gap * (n - 1.0)) / n)
            .max(theme.size_panel_header_btn_min_w());
        for label in labels {
            let is_active = active == *label;
            if filter_chip_button(ui, theme, label, is_active, egui::vec2(item_w, chip_h)).clicked()
            {
                picked = Some((*label).to_string());
            }
        }
    });
    picked
}

/// 分类筛选芯片（全部/在线/离线、常用/Docker 等）
pub fn filter_chip_button(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    active: bool,
    min_size: egui::Vec2,
) -> Response {
    let text_color = if active {
        theme.color_filter_chip_active_text()
    } else {
        theme.color_filter_chip_inactive_text()
    };
    let fill = if active {
        theme.color_filter_chip_active_fill()
    } else {
        Color32::TRANSPARENT
    };
    ui.add(
        Button::new(
            RichText::new(label)
                .size(theme.font_size_category_label())
                .color(text_color),
        )
        .fill(fill)
        .stroke(egui::Stroke::NONE)
        .rounding(theme.radius_category())
        .min_size(min_size),
    )
}

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
        ui.painter().text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            "✓",
            egui::FontId::proportional(theme.font_size_menu_item()),
            theme.accent_color(),
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
                theme.fg_medium_color()
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
                theme.fg_medium_color()
            });
        ui.selectable_label(selected, label)
    })
    .inner
}

/// 菜单项快捷键后缀（弱色）
pub fn menu_item_label(theme: &Theme, title: &str, shortcut: Option<&str>) -> RichText {
    let text = if let Some(sc) = shortcut {
        format!("{}  {}", title, sc)
    } else {
        title.to_string()
    };
    RichText::new(text)
        .size(theme.font_size_menu_item())
        .color(theme.fg_medium_color())
}

/// 输入框占位符 RichText（各主题统一用 `color_form_hint`）
pub fn hint_rich(theme: &Theme, text: &str, font_size: f32) -> RichText {
    RichText::new(text)
        .size(font_size)
        .color(theme.color_form_hint())
}

/// 单行输入框（带底+描边，各主题可读）
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
    theme.frame_form_text_input(focused).show(ui, |ui| {
        let mut edit = egui::TextEdit::singleline(text)
            .id(id)
            .frame(false)
            .desired_width((desired_width - 16.0).max(48.0))
            .text_color(theme.color_text_input_text())
            .font(egui::FontId::proportional(theme.font_size_body()));
        if !hint.is_empty() {
            edit = edit.hint_text(hint_rich(theme, hint, theme.font_size_body()));
        }
        if password {
            edit = edit.password(true);
        }
        ui.add(edit)
    }).inner
}

/// 多行输入框（带底+描边）
pub fn form_multiline_field(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    text: &mut String,
    desired_width: f32,
    rows: usize,
    password: bool,
) -> Response {
    let focused = ui.memory(|m| m.has_focus(id));
    theme.frame_form_text_input(focused).show(ui, |ui| {
        let mut edit = egui::TextEdit::multiline(text)
            .id(id)
            .frame(false)
            .desired_width((desired_width - 16.0).max(48.0))
            .desired_rows(rows)
            .text_color(theme.color_text_input_text())
            .font(egui::FontId::proportional(theme.font_size_body()));
        if password {
            edit = edit.password(true);
        }
        ui.add(edit)
    }).inner
}

/// 侧栏搜索框（左侧 🔍，focus 时紫色描边）
pub fn sidebar_search_field(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    query: &mut String,
    hint: &str,
    desired_width: f32,
) -> Response {
    let focused = ui.memory(|m| m.has_focus(id));
    let stroke = if focused {
        egui::Stroke::new(1.0, theme.accent_alpha(51))
    } else {
        egui::Stroke::new(1.0, theme.border_divider_color())
    };
    let font = theme.font_size_sidebar_control();
    egui::Frame::none()
        .fill(theme.color_subtle_inset_fill())
        .stroke(stroke)
        .rounding(theme.radius_search_input())
        .inner_margin(theme.margin_sidebar_search_field())
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(
                    RichText::new("🔍")
                        .size(font)
                        .color(theme.fg_low_color()),
                );
                ui.add(
                    egui::TextEdit::singleline(query)
                        .id(id)
                        .frame(false)
                        .hint_text(hint_rich(theme, hint, font))
                        .text_color(theme.color_text_input_text())
                        .font(egui::FontId::proportional(font))
                        .desired_width((desired_width - 22.0).max(80.0)),
                );
            });
        })
        .response
}

/// 左栏顶部 SSH 配置导入提示条（§4.2，约 34px，弱提示）
pub struct SshImportBannerAction {
    pub import: bool,
    pub dismiss: bool,
}

pub fn ssh_import_sidebar_banner(
    ui: &mut Ui,
    theme: &Theme,
    pending_count: usize,
) -> Option<SshImportBannerAction> {
    if pending_count == 0 {
        return None;
    }
    let mut action = SshImportBannerAction {
        import: false,
        dismiss: false,
    };
    const BAR_H: f32 = 34.0;
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, BAR_H), egui::Sense::click());
    let painter = ui.painter();
    let top = theme.bg_window_color();
    let bottom = theme.bg_body_color();
    const GRAD_STEPS: usize = 6;
    let step_h = rect.height() / GRAD_STEPS as f32;
    for i in 0..GRAD_STEPS {
        let t = (i as f32 + 0.5) / GRAD_STEPS as f32;
        let band = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.top() + step_h * i as f32),
            egui::pos2(rect.right(), rect.top() + step_h * (i as f32 + 1.0)),
        );
        painter.rect_filled(
            band,
            0.0,
            Color32::from_rgba_unmultiplied(
                lerp_u8(top.r(), bottom.r(), t),
                lerp_u8(top.g(), bottom.g(), t),
                lerp_u8(top.b(), bottom.b(), t),
                255,
            ),
        );
    }
    painter.hline(
        rect.x_range(),
        rect.bottom() - 1.0,
        egui::Stroke::new(1.0, theme.border_divider_color()),
    );

    let msg = format!("检测到 {} 个未导入的 SSH 配置", pending_count);
    let inner = rect.shrink2(egui::vec2(10.0, 0.0));
    ui.allocate_ui_at_rect(inner, |ui| {
        ui.set_height(BAR_H);
        ui.horizontal_centered(|ui| {
            ui.label(
                RichText::new("⚡")
                    .size(theme.font_size_title_bar_info())
                    .color(theme.amber_color()),
            );
            ui.add_space(theme.spacing_tool_btn_gap() + 1.0);
            let label = ui.label(
                RichText::new(&msg)
                    .size(theme.font_size_title_bar_info())
                    .color(theme.fg_low_color()),
            );
            label.on_hover_text(&msg);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, theme, GLYPH_CLOSE, theme.color_caption_text())
                    .on_hover_text("关闭提示")
                    .clicked()
                {
                    action.dismiss = true;
                }
                ui.add_space(theme.spacing_region_gap());
                if chrome_small_accent_button(ui, theme, "导入")
                    .on_hover_text("打开 SSH 配置导入")
                    .clicked()
                {
                    action.import = true;
                }
            });
        });
    });
    if resp.clicked() && !action.import && !action.dismiss {
        action.import = true;
    }
    Some(action)
}

#[inline]
fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    ((a as f32) * (1.0 - t) + (b as f32) * t).round() as u8
}

/// 标题栏 macOS 风格红绿灯（装饰；真实关/最小化/最大化由系统窗口按钮处理）
pub fn title_bar_traffic_lights(ui: &mut Ui, theme: &Theme) {
    let r = theme.radius_traffic_light();
    let gap = 7.0;
    let slot_w = r * 2.0 * 3.0 + gap * 2.0;
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(slot_w, r * 2.0), egui::Sense::hover());
    let cy = rect.center().y;
    let mut x = rect.left() + r;
    for color in [
        Color32::from_rgb(255, 95, 86),
        Color32::from_rgb(255, 189, 46),
        Color32::from_rgb(39, 201, 63),
    ] {
        ui.painter()
            .circle_filled(egui::pos2(x, cy), r, color);
        x += r * 2.0 + gap;
    }
}

/// 状态栏工具图标：默认 ≈8% 白，hover ≈25%（无独立着色）
pub fn status_tool_glyph(ui: &mut Ui, theme: &Theme, glyph: &str) -> Response {
    let h = theme.chrome_bar_content_height(theme.status_bar_height());
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(22.0, h), egui::Sense::click());
    let color = if resp.hovered() {
        theme.color_toolbar_glyph_hover()
    } else {
        theme.color_toolbar_glyph_idle()
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(theme.font_size_tool_btn()),
        color,
    );
    resp
}

/// 面板折叠后状态栏「▸ 名称 · N」复原按钮（须占真实宽度，勿用 0 宽 painter 叠字）
pub fn status_restore_chip(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    ui.add(
        Button::new(
            RichText::new(label)
                .size(theme.font_size_restore_btn())
                .color(theme.accent_alpha(89)),
        )
        .fill(Color32::TRANSPARENT)
        .stroke(egui::Stroke::NONE)
        .frame(false)
        .min_size(egui::vec2(
            0.0,
            theme.chrome_bar_content_height(theme.status_bar_height()),
        )),
    )
}

/// 弹窗标题行 + 分隔线；返回 `true` 表示点了关闭。
pub fn modal_header(ui: &mut Ui, theme: &Theme, title: &str, title_px: f32) -> bool {
    let trailing_w = panel_header_trailing_width(ui, theme, &[]);
    let title = title.to_string();
    let close = dock_panel_title_row(
        ui,
        theme,
        |ui| {
            ui.label(
                RichText::new(&title)
                    .size(title_px)
                    .strong()
                    .color(theme.color_section_title()),
            );
        },
        "关闭",
        trailing_w,
        |ui, theme| dock_panel_title_close_trailing(ui, theme, "关闭"),
    );
    ui.add_space(theme.spacing_modal_header_after_title());
    ui.separator();
    ui.add_space(theme.spacing_modal_header_after_sep());
    close
}

/// 右侧 dock 标题行（标题 + 关闭 ×）。
#[inline]
pub fn side_panel_title_row(ui: &mut Ui, theme: &Theme, title: &str) -> bool {
    dock_panel_title_close_only(
        ui,
        theme,
        title,
        DockPanelTitleStyle::DockHeading,
        "关闭",
    )
}

/// 侧栏小标题 + 右侧关闭 ×（与 [`dock_panel_title_close_only`] 相同布局）。
#[inline]
pub fn side_panel_section_title(
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    title_color: Color32,
) -> bool {
    dock_panel_title_close_only(
        ui,
        theme,
        title,
        DockPanelTitleStyle::Section { color: title_color },
        "关闭",
    )
}

/// 弹窗主按钮（自绘三态；勿 `add_enabled` 灰化，否则悬停不可见）
pub struct ModalPrimaryButton<'a> {
    theme: &'a Theme,
    label: &'a str,
    /// `false` 时仍可悬停高亮，点击由调用方忽略
    can_activate: bool,
}

impl ModalPrimaryButton<'_> {
    pub fn can_activate(mut self, can: bool) -> Self {
        self.can_activate = can;
        self
    }
}

impl Widget for ModalPrimaryButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        paint_modal_primary_button(ui, self.theme, self.label, self.can_activate)
    }
}

pub fn modal_primary_button_widget<'a>(theme: &'a Theme, label: &'a str) -> ModalPrimaryButton<'a> {
    ModalPrimaryButton {
        theme,
        label,
        can_activate: true,
    }
}

fn paint_modal_primary_button(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    can_activate: bool,
) -> Response {
    let size = theme.vec2_modal_footer_primary();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let rounding = theme.radius_list_item();
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    if hovered || pressed {
        ui.ctx().request_repaint();
    }

    let (fill, text_color) = if can_activate {
        if pressed {
            (
                theme.accent_dim_color(),
                theme.color_modal_primary_text(),
            )
        } else if hovered {
            (
                theme.color_modal_primary_fill_hover(),
                theme.color_modal_primary_text(),
            )
        } else {
            (
                theme.color_modal_primary_fill(),
                theme.color_modal_primary_text(),
            )
        }
    } else if hovered {
        (
            theme.color_modal_primary_fill_hover().gamma_multiply(0.75),
            theme.fg_high_color(),
        )
    } else {
        (
            theme.accent_alpha(89),
            theme.color_modal_secondary_text(),
        )
    };

    ui.painter().rect_filled(rect, rounding, fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(theme.font_size_normal()),
        text_color,
    );
    if hovered {
        ui.ctx().set_cursor_icon(if can_activate {
            CursorIcon::PointingHand
        } else {
            CursorIcon::NotAllowed
        });
    }
    response
}

fn paint_modal_secondary_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    let size = theme.vec2_modal_footer_secondary();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let rounding = theme.radius_list_item();
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    if hovered || pressed {
        ui.ctx().request_repaint();
    }

    if hovered || pressed {
        let fill = if pressed {
            theme.accent_alpha(51)
        } else {
            theme.color_panel_toolbar_btn_fill()
        };
        ui.painter().rect_filled(rect, rounding, fill);
    }
    let text_color = if hovered || pressed {
        theme.fg_high_color()
    } else {
        theme.color_modal_secondary_text()
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(theme.font_size_normal()),
        text_color,
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

fn paint_modal_danger_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    let size = theme.vec2_modal_footer_secondary();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let rounding = theme.radius_list_item();
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    if hovered || pressed {
        ui.ctx().request_repaint();
    }
    if hovered || pressed {
        ui.painter().rect_filled(
            rect,
            rounding,
            theme.red_color().gamma_multiply(if pressed { 0.22 } else { 0.14 }),
        );
    }
    let text_color = if hovered || pressed {
        theme.red_color()
    } else {
        theme.red_color().gamma_multiply(0.85)
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(theme.font_size_normal()),
        text_color,
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

pub fn modal_secondary_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    paint_modal_secondary_button(ui, theme, label)
}

pub fn modal_primary_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    paint_modal_primary_button(ui, theme, label, true)
}

pub fn modal_danger_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    paint_modal_danger_button(ui, theme, label)
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
