//! 弹窗 / 侧栏标题与操作按钮的统一视觉（关闭 ×、侧栏 ◀ 收起、主次按钮）。
//! 颜色与尺寸均来自 [`Theme`]，本模块不硬编码样式。

use eframe::egui::{self, Button, Color32, CursorIcon, Painter, Response, RichText, Sense, Stroke, Ui, Widget};
use crate::ui::icons::{self, IconId};
use crate::ui::theme::Theme;

fn theme_icon_hit(
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

fn theme_icon_hit_revealed(
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

/// Tab 栏图标按钮（关闭 / 新建）：固定点击区、悬停底。
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

/// 标签栏「新建 Tab」按钮（与 Tab 芯片同高、垂直居中）
pub fn tab_bar_new_tab_button(ui: &mut Ui, theme: &Theme) -> Response {
    let row_h = theme.size_tab_bar_row_h();
    let icon = theme.size_tab_bar_icon_btn();
    let accel = crate::platform::accel("T");
    let tooltip = match crate::i18n::language(ui.ctx()) {
        crate::i18n::UiLanguage::En => format!(
            "New tab: select a session on the left, then click here or {accel}; opens new session dialog if none selected.",
        ),
        crate::i18n::UiLanguage::Zh => format!(
            "新标签：左侧选中连接后点此或 {accel}；无选中时打开新建会话配置",
        ),
    };
    ui.allocate_ui_with_layout(
        egui::vec2(icon, row_h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            tab_bar_icon_button(
                ui,
                theme,
                IconId::Plus,
                tooltip.as_str(),
            )
        },
    )
    .inner
}

/// 通用图标按钮（可指定 idle 色）
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

/// 标题栏 / 右 dock 关闭 ×（28px 点击区 + 悬停底，与 Tab 栏 × 同级）
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

/// 弹窗 / 侧栏标题栏关闭（默认提示「关闭」）
pub fn close_icon_button(ui: &mut Ui, theme: &Theme) -> Response {
    close_icon_button_with_tooltip(ui, theme, crate::i18n::tr(ui.ctx(), "Close", "关闭"))
}

/// 右 dock 标题栏关闭（与 [`close_icon_button_with_tooltip`] 相同尺寸；`tooltip` 仅设置一次，避免叠两条提示）
pub fn dock_close_icon_button(ui: &mut Ui, theme: &Theme, tooltip: &str) -> Response {
    close_icon_button_with_tooltip(ui, theme, tooltip)
}

/// 侧栏标题行方形图标按钮（与排序下拉同高）。
pub fn sidebar_header_icon_button(ui: &mut Ui, theme: &Theme, id: IconId, color: Color32) -> Response {
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

/// 面板标题栏「＋」新建（连接栏 / 命令片段库统一：小方钮 + 浅紫底）
pub fn panel_header_new_button(ui: &mut Ui, theme: &Theme) -> Response {
    panel_header_new_button_with_label(ui, theme, "")
}

/// 带可见标签的新建按钮；`label` 为空时仅显示「＋」。
pub fn panel_header_new_button_with_label(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    if label.is_empty() {
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
        let fill = if theme.uses_modern_palette() {
            if pressed {
                theme.color_widget_active_fill()
            } else if hovered {
                theme.color_widget_hover_fill()
            } else {
                Color32::TRANSPARENT
            }
        } else if pressed {
            theme.accent_alpha(64)
        } else if hovered {
            theme.accent_alpha(51)
        } else {
            theme.accent_alpha(38)
        };
        let icon_color = if theme.uses_modern_palette() {
            if hovered || pressed {
                theme.text_primary()
            } else {
                theme.text_secondary().gamma_multiply(0.72)
            }
        } else {
            theme.accent_color()
        };
        ui.painter().rect(rect, rounding, fill, egui::Stroke::NONE);
        icons::paint_icon(
            ui,
            rect,
            IconId::Plus,
            icon_color,
            theme.font_size_sidebar_icon_glyph(),
        );
        if hovered {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        return response;
    }
    paint_control_button(
        ui,
        theme,
        label,
        Some(IconId::Plus),
        ControlButtonVariant::ToolbarPrimary,
        theme.size_panel_header_btn_min_w(),
        true,
    )
}

/// [`panel_header_new_button`] 别名（侧栏）
#[inline]
pub fn sidebar_new_session_button(ui: &mut Ui, theme: &Theme) -> Response {
    panel_header_new_button(ui, theme)
}

/// 排序芯片预估宽度（与 [`panel_sort_chip`] 一致）
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

/// 排序芯片（与分类筛选同高；连接栏点开菜单、片段栏点击轮换）
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
    ui.painter().rect(rect, rounding, fill, stroke);
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

/// 小号图标按钮（终端搜索上/下条等）
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

/// 异步加载行：旋转指示 + 文案（SFTP / 监控 / Vault 等复用）
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

/// 小号文字按钮（替换 `small_button`，带悬停底）
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

fn session_sort_icon(sort: crate::core::session_sort::SessionSortBy) -> IconId {
    use crate::core::session_sort::SessionSortBy;
    match sort {
        SessionSortBy::Name | SessionSortBy::NameDesc => IconId::SortName,
        SessionSortBy::LastConnected => IconId::SortRecent,
        SessionSortBy::CreatedAt => IconId::SortUsage,
    }
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
    if response.clicked() {
        ui.memory_mut(|mem| mem.toggle_popup(popup_id));
    }
    egui::popup::popup_above_or_below_widget(
        ui,
        popup_id,
        &response,
        egui::AboveOrBelow::Below,
        |ui| {
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
                    ui.memory_mut(|mem| mem.close_popup());
                }
            }
        },
    );
}

/// 下拉 / 右键 / ComboBox 弹出层共用的控件色（含 `widgets.open`，避免子菜单发黑底）。
pub fn apply_popup_widget_visuals(visuals: &mut egui::Visuals, theme: &Theme) {
    if theme.uses_modern_palette() {
        let menu_bg = theme.color_menu_popup_fill();
        visuals.window_fill = menu_bg;
        visuals.widgets.inactive.bg_fill = menu_bg;
        visuals.widgets.inactive.weak_bg_fill = menu_bg;
        visuals.widgets.hovered.bg_fill = theme.color_control_secondary_fill_hover();
        visuals.widgets.active.bg_fill = theme.color_control_secondary_fill_pressed();
        visuals.widgets.inactive.fg_stroke.color = theme.text_secondary();
        visuals.widgets.hovered.fg_stroke.color = theme.text_primary();
        visuals.widgets.active.fg_stroke.color = theme.text_primary();
        let open = &mut visuals.widgets.open;
        open.weak_bg_fill = theme.color_control_secondary_fill_pressed();
        open.bg_fill = theme.color_control_secondary_fill_pressed();
        open.bg_stroke = egui::Stroke::NONE;
        open.fg_stroke.color = theme.text_primary();
    } else {
        visuals.widgets.inactive.bg_fill = theme.bg_window_color();
        visuals.widgets.hovered.bg_fill = theme.accent_alpha(38);
        visuals.widgets.active.bg_fill = theme.accent_alpha(64);
        visuals.widgets.inactive.fg_stroke.color = theme.text_secondary();
        visuals.widgets.hovered.fg_stroke.color = theme.text_primary();
        let open = &mut visuals.widgets.open;
        open.weak_bg_fill = theme.accent_alpha(38);
        open.bg_fill = theme.accent_alpha(38);
        open.bg_stroke = egui::Stroke::NONE;
        open.fg_stroke.color = theme.text_primary();
    }
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

/// 面板标题字号（侧栏 / dock / 弹窗统一）
pub fn modal_title_font_size(theme: &Theme) -> f32 {
    theme.font_size_panel_header_title()
}

/// 面板标题 RichText（modern =flat 主色；其它主题加粗）
pub fn rich_panel_header_title(theme: &Theme, text: &str) -> RichText {
    let mut rt = RichText::new(text).size(theme.font_size_panel_header_title());
    if theme.uses_modern_palette() {
        rt = rt.color(theme.text_primary());
    } else {
        rt = rt.strong().color(theme.color_panel_header_title());
    }
    rt
}

/// 居中弹窗主标题（与 [`rich_panel_header_title`] 一致）
pub fn rich_modal_title(theme: &Theme, text: &str) -> RichText {
    rich_panel_header_title(theme, text)
}

/// 区域外框：左、上、右（不画底边，避免与底栏顶部分隔线叠成双行）
pub fn paint_rect_border_ltr(painter: &Painter, rect: egui::Rect, stroke: Stroke) {
    if rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }
    painter.vline(rect.min.x, rect.y_range(), stroke);
    painter.hline(rect.x_range(), rect.min.y, stroke);
    painter.vline(rect.max.x - 0.5, rect.y_range(), stroke);
}

/// 侧栏 / 右 dock 壳层描边：左、上、右 + 底部分隔线（底线用 divider，避免与状态栏叠粗线）。
pub fn paint_region_panel_shell_border(
    painter: &Painter,
    rect: egui::Rect,
    theme: &Theme,
    flush_bottom: bool,
) {
    if rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }
    let stroke = theme.panel_stroke();
    paint_rect_border_ltr(painter, rect, stroke);
    if flush_bottom {
        painter.hline(
            rect.x_range(),
            rect.max.y - 0.5,
            theme.divider_stroke(),
        );
    }
}

/// 区域外框：仅左右（顶线由 Tab 条底部分隔线承担，避免与 PTY 顶行叠线）
pub fn paint_rect_border_lr(painter: &Painter, rect: egui::Rect, stroke: Stroke) {
    if rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }
    painter.vline(rect.min.x, rect.y_range(), stroke);
    painter.vline(rect.max.x - 0.5, rect.y_range(), stroke);
}

/// 标题行与正文之间的横线
pub fn panel_header_divider(ui: &mut Ui, theme: &Theme) {
    let w = ui.available_width().max(1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(1.0, theme.color_panel_header_divider()),
    );
}

/// 右 dock 标题行与正文之间的分隔（modern：留白 + 极淡发丝线）
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
        egui::Stroke::new(theme.hairline_width(ui.ctx()), theme.color_dock_header_divider()),
    );
}

/// 标准弹窗 `Window`：无系统标题栏、不可折叠、统一外框（须再 `.open()` / `.show()` / 尺寸）
/// 统一弹窗入口：去掉默认 title_bar / collapse，应用 [`modal_window_frame`]，
/// 并把约束放宽到整屏（`ctx.screen_rect()`），否则默认 `constrain(true)` 会把弹窗夹在
/// `ctx.available_rect()` 内，右 dock 打开后无法把弹窗拖到 dock 上方。
///
/// 首次居中请用 [`layout_util::modal_center_pos`] + `.default_pos(...)`，勿 `.anchor(...)`（拖拽会弹回）。
pub fn modal_window<'a>(window_id: &'a str, theme: &Theme, ctx: &egui::Context) -> egui::Window<'a> {
    egui::Window::new(window_id)
        .title_bar(false)
        .collapsible(false)
        .frame(modal_window_frame(theme))
        // egui 0.27 系列方法名是 `constraint_to`（拼写问题，但 API 就是这样）。
        .constraint_to(ctx.screen_rect())
}

/// 将刚绘制的弹窗提到最前，避免被右 dock Foreground 盖住或误点底层关闭钮。
pub fn raise_window_response(ctx: &egui::Context, response: &egui::Response) {
    ctx.move_to_top(response.layer_id);
}

/// 右侧 dock / 左侧连接栏外框：统一底色与内容区内边距。
pub fn region_panel_frame(theme: &Theme) -> egui::Frame {
    theme.frame_region_panel()
}

/// 左连接栏外框（底缘贴状态栏顶线，底角不圆；描边由 [`paint_region_panel_shell_border`] 统一绘制）
pub fn sidebar_panel_frame(theme: &Theme) -> egui::Frame {
    theme
        .frame_region_panel_flush_bottom()
        .stroke(egui::Stroke::NONE)
        .inner_margin(theme.right_dock_content_margin())
}

/// 右 dock 左侧让出的 `bg_body` 缝（单 dock 与终端之间、多 dock 之间都看得见）。
fn right_dock_outer_margin(theme: &Theme) -> egui::Margin {
    let mut m = theme.margin_right_dock_screen_outer();
    m.left = theme.spacing_dock_gap();
    m
}

/// 右 `SidePanel` 占位槽（透明，屏右缘留 `bg_body` 缝）。
pub fn right_dock_placeholder_frame(theme: &Theme) -> egui::Frame {
    egui::Frame::none().outer_margin(right_dock_outer_margin(theme))
}

/// 右 `SidePanel` 可见外框（SFTP / 凭证等直绘 dock）。
pub fn right_dock_panel_frame(theme: &Theme) -> egui::Frame {
    theme
        .frame_region_panel_flush_bottom()
        .outer_margin(right_dock_outer_margin(theme))
}

/// 在右 dock 槽位（含左侧 `spacing_dock_gap` 缝）铺 `bg_body`；须用 [`side_panel_place_slot`] 后的矩形。
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
        egui::Id::new("mistterm_right_dock_slot_bg"),
    );
    let painter = ctx.layer_painter(layer_id);
    painter.rect_filled(bg, 0.0, theme.bg_body_color());
    painter.vline(bg.min.x + 0.5, bg.y_range(), theme.divider_stroke());
}

/// SidePanel 回调内（旧路径）：勿再使用；在 `.show` 之后调 [`paint_right_dock_slot_gap`].
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
        egui::Id::new("mistterm_right_dock_slot_bg"),
    );
    let painter = ui.ctx().layer_painter(layer_id);
    painter.rect_filled(bg, 0.0, theme.bg_body_color());
    // 缝左侧 1px 分隔线（终端/相邻 dock 与当前 dock 之间）
    painter.vline(bg.min.x + 0.5, bg.y_range(), theme.divider_stroke());
}

/// 右 dock `outer_margin` 与窗口右缘之间的竖条（须铺 `bg_body`，否则会露系统/窗口黑底）。
pub fn paint_right_dock_screen_gutter(
    ctx: &egui::Context,
    theme: &Theme,
    top_chrome_height: f32,
) {
    let inset = theme.spacing_right_dock_screen_inset();
    if inset < 0.5 || !inset.is_finite() {
        return;
    }
    let screen = ctx.screen_rect();
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
    let layer_id =
        egui::LayerId::new(egui::Order::Background, egui::Id::new("mistterm_right_dock_gutter"));
    ctx.layer_painter(layer_id)
        .rect_filled(gutter, 0.0, theme.bg_body_color());
}

/// 左缘略向左扩 2px，盖住 Central `bg_body` 可能压到侧栏左缘的细缝。
pub const RIGHT_DOCK_SHELL_LEFT_BLEED: f32 = 0.0;

/// 右 dock Foreground：先铺满整个槽位（`Frame` 仅包住内容时左侧会透出中央 `bg_body`）。
pub fn paint_right_dock_slot_shell(ui: &mut egui::Ui, slot: egui::Rect, theme: &Theme) {
    paint_right_dock_slot_shell_with_painter(ui.painter(), slot, theme);
}

/// 与 [`paint_right_dock_slot_shell`] 相同，用于在 `Area` 外先铺底色（避免可点层盖住整块槽位）。
/// 右 dock 壳层圆角：顶角不圆（与终端 Tab 条齐平）；贴底栏时底角也为 0。
pub fn right_dock_shell_rounding(theme: &Theme, flush_bottom: bool) -> egui::Rounding {
    if flush_bottom {
        egui::Rounding::ZERO
    } else {
        let r = theme.radius_panel();
        egui::Rounding {
            nw: 0.0,
            ne: 0.0,
            sw: r,
            se: r,
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

/// 槽位扣除 region panel 内边距后的内容矩形（须用 `Margin::shrink_rect`，勿 `shrink2(left+right)`）。
#[inline]
pub fn right_dock_slot_content_rect(slot: egui::Rect, theme: &Theme) -> egui::Rect {
    theme.right_dock_content_margin().shrink_rect(slot)
}

/// Central 之后 Foreground 重绘右 dock 用的图层（仅绘制壳层，勿在此注册可点 `Area`）。
#[inline]
pub fn right_dock_foreground_layer_id(id: &'static str) -> egui::LayerId {
    egui::LayerId::new(egui::Order::Middle, egui::Id::new(id))
}

/// 右 dock Foreground `Area`（可点层）；正文仍在 `inner` 子区域布局。
pub fn right_dock_foreground_body_area(id: &'static str) -> egui::Area {
    egui::Area::new(egui::Id::new(id))
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

/// 由 SidePanel 槽位计算 Foreground 绘制区（与 [`right_dock_slot_content_rect`] 一致）。
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

/// 铺 Foreground 壳层与右边框（在 `Area` 之前用 `Painter` 调用）。
pub fn paint_right_dock_foreground_shell(
    ctx: &egui::Context,
    layer_id: egui::LayerId,
    paint: egui::Rect,
    theme: &Theme,
) {
    let painter = egui::Painter::new(ctx.clone(), layer_id, paint);
    paint_right_dock_slot_shell_with_painter_ex(&painter, paint, theme, true);
}

/// 标准 Foreground 正文宿主：`Area` 覆盖 `paint`，正文在 `inner`（与 LAYOUT.md §8.3 一致）。
pub fn show_right_dock_foreground_body<R>(
    area_id: &'static str,
    ctx: &egui::Context,
    _theme: &Theme,
    geom: &RightDockForegroundGeom,
    _profile: crate::ui::layout_util::SidePanelProfile,
    add_body: impl FnOnce(&mut Ui, f32) -> R,
) -> egui::InnerResponse<R> {
    let paint_size = geom.paint.size();
    let body_w = geom.inner.width().max(48.0);
    right_dock_foreground_body_area(area_id)
        .constrain_to(geom.paint)
        .fixed_pos(geom.paint.min)
        .show(ctx, |ui| {
            ui.set_min_size(paint_size);
            ui.set_max_size(paint_size);
            ui.set_clip_rect(geom.paint);
            ui.allocate_ui_at_rect(geom.inner, |ui| {
                ui.set_clip_rect(geom.inner);
                let cap = ui.available_width().max(48.0).min(body_w);
                let w = crate::ui::layout_util::constrain_ui_to_right_dock_body(ui, cap);
                add_body(ui, w)
            })
            .inner
        })
}

/// 改宽手柄：独立 `Foreground` 层，须在**全部** dock 正文绘完后、按屏上左→右顺序调用（右邻正文会盖住左缝）。
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
        egui::pos2((geom.paint.min.x + 4.0).min(geom.paint.max.x), geom.paint.max.y),
    );
    if !gutter.is_positive() {
        return;
    }
    egui::Area::new(egui::Id::new((area_id, "resize_grip_layer")))
        .order(egui::Order::Foreground)
        .movable(false)
        .interactable(true)
        .fixed_pos(gutter.min)
        .show(ctx, |ui| {
            ui.set_min_size(gutter.size());
            ui.set_max_size(gutter.size());
            let response = ui.interact(gutter, ui.id(), egui::Sense::drag());
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
                let id = egui::Id::new(panel_id);
                if let Some(mut state) = egui::containers::panel::PanelState::load(ctx, id) {
                    let dx = ui.input(|i| i.pointer.delta().x);
                    let current_w = state.rect.width();
                    let (_, min_w, max_w) =
                        crate::ui::layout_util::right_dock_resize_bounds(current_w);
                    let new_w = (current_w - dx).clamp(min_w, max_w);
                    state.rect.min.x = state.rect.max.x - new_w;
                    ctx.data_mut(|d| d.insert_persisted(id, state));
                    ctx.request_repaint();
                }
            }
        });
}

/// 由 SidePanel 槽位绘制改宽手柄（各 dock 在 workspace 统一 pass 里调用）。
pub fn show_right_dock_resize_grip_for_slot(
    ctx: &egui::Context,
    theme: &Theme,
    area_id: &'static str,
    panel_slot: Option<egui::Rect>,
    panel_id: &str,
    profile: crate::ui::layout_util::SidePanelProfile,
) {
    let screen = ctx.screen_rect();
    let dock_inset = theme.spacing_right_dock_screen_inset();
    let Some(slot) = crate::ui::layout_util::right_dock_foreground_slot(
        panel_slot,
        ctx,
        panel_id,
        profile,
        None,
        dock_inset,
    ) else {
        return;
    };
    let geom = prepare_right_dock_foreground_geom(slot, screen, theme);
    show_right_dock_resize_grip_layer(ctx, theme, area_id, &geom);
}

fn right_dock_panel_id_for_foreground(area_id: &'static str) -> Option<&'static str> {
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

/// 右 dock 内「左标签 + 右数值」行（宽度随父级 `available_width`）。
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
    let ht_dismiss = crate::i18n::tr(ui.ctx(), "Dismiss SSH import banner", "关闭导入提示");
    if close_icon_button_with_tooltip(ui, theme, ht_dismiss).clicked()
    {
        out.dismiss_ssh_import = true;
    }
    ui.add_space(theme.spacing_sm());
    let chip_clicked = ui
        .scope(|ui| {
            let w = &mut ui.style_mut().visuals.widgets;
            w.inactive.weak_bg_fill = theme.color_overlay_fill_subtle();
            w.hovered.weak_bg_fill = theme.accent_alpha(25);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (r, _) =
                    ui.allocate_exact_size(
                        egui::vec2(theme.size_icon_glyph(), theme.size_icon_glyph()),
                        egui::Sense::hover(),
                    );
                icons::paint_icon(ui, r, IconId::Alert, theme.amber_color(), theme.size_icon_glyph());
                let label = match crate::i18n::language(ui.ctx()) {
                    crate::i18n::UiLanguage::En => {
                        format!("{pending_ssh_imports} pending imports")
                    }
                    crate::i18n::UiLanguage::Zh => {
                        format!("{pending_ssh_imports} 个待导入")
                    }
                };
                ui.add(
                    Button::new(
                        RichText::new(label)
                            .size(theme.font_size_title_bar_info())
                            .color(theme.amber_color()),
                    )
                    .rounding(4.0),
                )
                .clicked()
            })
            .inner
        })
        .inner;
    if chip_clicked {
        out.open_ssh_import = true;
    }
    out
}

/// 顶栏菜单行上的 SSH 导入 chip 等动作
#[derive(Default)]
pub struct TitleBarChromeResult {
    pub open_ssh_import: bool,
    pub dismiss_ssh_import: bool,
}

/// VS Code 风格 Tab 底栏指示线（2 物理像素 accent）
fn paint_vscode_tab_bottom_indicator(
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
    painter.rect_filled(bar, 0.0, theme.accent_color());
}

/// 终端区会话 Tab：整块底色（圆点 + 标题 + 关闭），对齐 proto `.tab`。
pub struct SessionTabChipResult {
    pub response: Response,
    pub close_clicked: bool,
}

/// 标签右侧关闭槽位（与 [`session_tab_chip`] 内关闭按钮对齐）。
fn pointer_hovers_tab_close_slot(ctx: &egui::Context, inner: egui::Rect, close_slot: f32) -> bool {
    let close_rect = egui::Rect::from_min_size(
        egui::pos2(inner.max.x - close_slot, inner.center().y - close_slot * 0.5),
        egui::vec2(close_slot, close_slot),
    );
    ctx.pointer_hover_pos()
        .is_some_and(|p| close_rect.contains(p))
}

pub fn session_tab_chip(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    active: bool,
    online: bool,
    show_close: bool,
    env_color: Option<egui::Color32>,
) -> SessionTabChipResult {
    let size = egui::vec2(theme.size_tab_min_w(), theme.size_tab_min_h());
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let inner = rect.shrink2(egui::vec2(
        theme.spacing_tab_x(),
        theme.spacing_tab_y(),
    ));
    let close_slot = theme.size_tab_bar_icon_btn();
    let close_slot_hot = pointer_hovers_tab_close_slot(ui.ctx(), inner, close_slot);
    // 子控件（×）会抢走外层 hover；用关闭槽位命中避免 × 显隐来回切换。
    let tab_hot = response.hovered() || close_slot_hot;
    let modern = theme.uses_modern_palette();
    let fill = if modern {
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
    let rounding = if modern {
        egui::Rounding::ZERO
    } else {
        egui::Rounding::same(theme.radius_category())
    };
    let stroke = if modern || active {
        egui::Stroke::NONE
    } else {
        egui::Stroke::new(1.0, theme.color_tab_inactive_stroke())
    };
    ui.painter().rect(rect, rounding, fill, stroke);
    if active {
        paint_vscode_tab_bottom_indicator(ui.painter(), ui.ctx(), rect, theme);
    }
    let mut close_clicked = false;
    let mut row_ui = ui.child_ui(inner, egui::Layout::left_to_right(egui::Align::Center));
    row_ui.set_width(inner.width());
    row_ui.set_min_width(inner.width());
    row_ui.set_max_width(inner.width());
    row_ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_tab_dot_text();
        let status_color = if online {
            theme.green_color()
        } else {
            theme.color_tab_offline_dot()
        };
        let dot_color = env_color
            .map(|c| c.gamma_multiply(0.78))
            .unwrap_or(status_color);
        let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(5.0, 5.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(dot_rect.center(), 2.5, dot_color);
        ui.label(
            RichText::new(label)
                .size(theme.font_size_tab_label())
                .color(if active {
                    theme.text_primary()
                } else if modern {
                    theme.text_secondary().gamma_multiply(0.72)
                } else {
                    theme.text_tertiary()
                }),
        );
        // 关闭按钮始终占位；仅切换绘制，避免控件树显隐导致 × 上 hover 闪烁。
        let close_visible = show_close || active || tab_hot;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close_tooltip = format!(
                "{} · {}",
                crate::i18n::tr(ui.ctx(), "Close tab", "关闭标签"),
                crate::platform::accel("W")
            );
            let close_resp = theme_icon_hit_revealed(
                ui,
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
        });
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

/// 主 / 次按钮视觉（弹窗底栏、标题栏工具、面板内操作共用）
#[derive(Clone, Copy, PartialEq)]
enum ControlButtonVariant {
    Primary,
    ToolbarPrimary,
    Secondary,
    Danger,
}

fn paint_caption_in_rect_center(
    ui: &mut Ui,
    rect: egui::Rect,
    label: &str,
    font_size: f32,
    color: Color32,
) {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(font_size),
        color,
    );
    let pos = rect.center() - galley.size() * 0.5;
    ui.painter().galley(pos, galley);
}

/// `center_row`: 工具按钮等在槽内居中；`false` 时自左 `pad_x` 起排（排序芯片、状态栏）。
fn paint_icon_caption_row_in_rect(
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
        egui::FontId::proportional(font_size),
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
    painter.galley(egui::pos2(text_x, icon_cy - text_h * 0.5), galley);
}

fn control_button_size(ui: &Ui, theme: &Theme, label: &str, with_icon: bool, min_w: f32) -> egui::Vec2 {
    let h = theme.size_control_btn_h();
    let pad_x = theme.spacing_panel_header_btn_pad_x();
    let font = egui::FontId::proportional(theme.font_size_control_btn());
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

fn secondary_control_button_colors(
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
        return (
            theme.color_control_secondary_fill_pressed(),
            c,
            c,
        );
    }
    if hovered {
        let c = theme.color_control_secondary_active_text();
        return (
            theme.color_control_secondary_fill_hover(),
            c,
            c,
        );
    }
    (
        theme.color_control_secondary_fill_idle(),
        theme.color_control_secondary_idle_text(),
        theme.color_control_secondary_idle_icon(),
    )
}

fn primary_control_button_colors(
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
        return (
            theme.accent_dim_color(),
            c,
            c,
        );
    }
    if hovered {
        let c = theme.color_modal_primary_text();
        return (
            theme.color_modal_primary_fill_hover(),
            c,
            c,
        );
    }
    let c = theme.color_modal_primary_text();
    (
        theme.color_modal_primary_fill(),
        c,
        c,
    )
}

/// 面板命令栏主按钮（暗夜：浅底 ghost；彩色主题：与弹窗主按钮同族）
fn toolbar_primary_control_button_colors(
    theme: &Theme,
    can_activate: bool,
    hovered: bool,
    pressed: bool,
) -> (Color32, Color32, Color32) {
    if theme.uses_modern_palette() {
        if !can_activate {
            return (
                theme.color_control_secondary_fill_disabled(),
                theme.color_control_disabled_text(),
                theme.color_control_disabled_text(),
            );
        }
        if pressed {
            let c = theme.text_primary();
            return (
                theme.color_control_secondary_fill_pressed(),
                c,
                c,
            );
        }
        if hovered {
            let c = theme.text_primary();
            return (theme.color_widget_hover_fill(), c, c);
        }
        let muted = theme.text_secondary().gamma_multiply(0.72);
        return (Color32::TRANSPARENT, muted, muted);
    }
    primary_control_button_colors(theme, can_activate, hovered, pressed)
}

fn paint_control_button(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    icon: Option<IconId>,
    variant: ControlButtonVariant,
    min_w: f32,
    can_activate: bool,
) -> Response {
    let size = control_button_size(ui, theme, label, icon.is_some(), min_w);
    let rounding = theme.radius_list_item();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    if hovered || pressed {
        ui.ctx().request_repaint();
    }

    let stroke = match variant {
        ControlButtonVariant::Primary | ControlButtonVariant::ToolbarPrimary => egui::Stroke::NONE,
        ControlButtonVariant::Secondary => theme.color_control_secondary_stroke(can_activate),
        ControlButtonVariant::Danger => {
            egui::Stroke::new(1.0, theme.color_text_input_stroke())
        }
    };
    let (fill, text_color, icon_color) = match variant {
        ControlButtonVariant::Danger => {
            unreachable!("danger buttons use paint_icon_only_button")
        }
        ControlButtonVariant::Primary => {
            primary_control_button_colors(theme, can_activate, hovered, pressed)
        }
        ControlButtonVariant::ToolbarPrimary => {
            toolbar_primary_control_button_colors(theme, can_activate, hovered, pressed)
        }
        ControlButtonVariant::Secondary => {
            secondary_control_button_colors(theme, can_activate, hovered, pressed)
        }
    };

    ui.painter().rect(rect, rounding, fill, stroke);
    if let Some(id) = icon {
        let icon_px = theme.size_icon_glyph();
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
            0.0,
            true,
        );
    } else {
        paint_caption_in_rect_center(
            ui,
            rect,
            label,
            theme.font_size_control_btn(),
            text_color,
        );
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

fn icon_only_button_size(theme: &Theme, min_w: f32) -> egui::Vec2 {
    let h = theme.size_control_btn_h();
    let pad_x = theme.spacing_panel_header_btn_pad_x();
    let icon_px = theme.size_icon_glyph();
    let side = (icon_px + 2.0 * pad_x).max(min_w).max(h);
    egui::vec2(side, h)
}

/// 仅图标（方形容器），悬停显示 `tooltip`。
fn paint_icon_only_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    variant: ControlButtonVariant,
    min_w: f32,
    can_activate: bool,
) -> Response {
    let size = icon_only_button_size(theme, min_w);
    let rounding = theme.radius_list_item();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let pressed = response.is_pointer_button_down_on();
    if hovered || pressed {
        ui.ctx().request_repaint();
    }

    let stroke = match variant {
        ControlButtonVariant::Primary | ControlButtonVariant::ToolbarPrimary => egui::Stroke::NONE,
        ControlButtonVariant::Secondary => theme.color_control_secondary_stroke(can_activate),
        ControlButtonVariant::Danger => {
            egui::Stroke::new(1.0, theme.color_text_input_stroke())
        }
    };
    let (fill, icon_color) = match variant {
        ControlButtonVariant::Primary => {
            let (fill, text, icon) =
                primary_control_button_colors(theme, can_activate, hovered, pressed);
            let _ = text;
            (fill, icon)
        }
        ControlButtonVariant::ToolbarPrimary => {
            let (fill, text, icon) =
                toolbar_primary_control_button_colors(theme, can_activate, hovered, pressed);
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
                    theme.red_color().gamma_multiply(if pressed { 0.22 } else { 0.14 }),
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

    ui.painter().rect(rect, rounding, fill, stroke);
    let icon_px = theme.size_icon_glyph();
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

/// 侧栏 / 右 dock 标题行次要工具按钮（宽度按文字测量）。
pub fn panel_toolbar_button_widget<'a>(theme: &'a Theme, text: RichText) -> Button<'a> {
    if theme.uses_modern_palette() {
        Button::new(text)
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .rounding(theme.radius_list_item())
    } else {
        Button::new(text)
            .fill(theme.color_panel_toolbar_btn_fill())
            .stroke(theme.divider_stroke())
            .rounding(theme.radius_list_item())
    }
}

fn panel_toolbar_button_size(ui: &Ui, theme: &Theme, label: &str, with_icon: bool) -> egui::Vec2 {
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

/// 无边框文字链式操作（AI 消息复制/重新生成等；`emphasis` 0~1 控制可见度）。
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
        egui::pos2(icon_rect.max.x + theme.spacing_xs(), rect.center().y - galley.size().y * 0.5),
        galley,
    );
    response.on_hover_text(label)
}

/// 标题行 / 工具栏纯图标按钮（悬停文案见 `tooltip`）。
pub fn panel_toolbar_icon_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
) -> Response {
    paint_icon_only_button(
        ui,
        theme,
        icon,
        ControlButtonVariant::Secondary,
        theme.size_panel_header_btn_min_w(),
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

/// 工具栏图标按钮或采集中态（带可见标签）。
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
    );
    let mut child = ui.child_ui(
        rect,
        egui::Layout::left_to_right(egui::Align::Center),
    );
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

/// 标题行主操作（accent 底，纯图标）。
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
        ControlButtonVariant::ToolbarPrimary,
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
    );
    let mut child = ui.child_ui(
        rect,
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
    );
    child.add(egui::Spinner::new());
    response.on_hover_text(crate::i18n::tr(
        ui.ctx(),
        "Collecting metrics…",
        "采集中…",
    ))
}

/// 面板标题行左侧：图标 + 文案（侧栏 / dock / 弹窗统一）
pub fn panel_header_title_leading(ui: &mut Ui, theme: &Theme, icon: IconId, title: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm();
        let px = theme.size_icon_glyph();
        let (r, _) = ui.allocate_exact_size(egui::vec2(px, px), egui::Sense::hover());
        let icon_color = if theme.uses_modern_palette() {
            theme.text_secondary()
        } else {
            theme.color_panel_header_title()
        };
        icons::paint_icon(ui, r, icon, icon_color, px);
        ui.label(rich_panel_header_title(theme, title));
    });
}

/// 右 dock 大标题 + 左侧图标（与 [`panel_header_title_leading`] 一致）
pub fn dock_title_row(ui: &mut Ui, theme: &Theme, icon: IconId, title: &str) {
    panel_header_title_leading(ui, theme, icon, title);
}

/// 区段标题 + 左侧图标
pub fn section_title_row(ui: &mut Ui, theme: &Theme, icon: IconId, title: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let px = theme.font_size_section_title();
        let (r, _) = ui.allocate_exact_size(egui::vec2(px, px), egui::Sense::hover());
        icons::paint_icon(ui, r, icon, color, px);
        ui.label(rich_section_title(theme, title, color));
    });
}

/// 区段标题（与 [`rich_panel_header_title`] 一致；`color` 参数保留兼容）
pub fn rich_section_title(theme: &Theme, text: &str, _color: Color32) -> RichText {
    rich_panel_header_title(theme, text)
}

/// 右 dock 标题（与 [`rich_panel_header_title`] 一致）
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

/// 统一复选框：未选中时浅底 + 描边始终可见；勾选为 accent 底 + 浅色勾。
/// 全局 `inactive.bg_fill = TRANSPARENT` 下裸 `ui.checkbox` 往往只有悬停才看得出方框。
pub fn form_checkbox(ui: &mut Ui, theme: &Theme, value: &mut bool, text: &str) -> Response {
    form_checkbox_with_id(ui, theme, text, value, text)
}

pub fn form_checkbox_with_id(
    ui: &mut Ui,
    theme: &Theme,
    id: impl std::hash::Hash,
    value: &mut bool,
    text: &str,
) -> Response {
    ui.push_id(id, |ui| {
        let rounding = egui::Rounding::same(theme.radius_checkbox());
        let off_border = theme.color_checkbox_off_stroke_color();
        let w = &mut ui.style_mut().visuals.widgets;
        w.inactive.bg_fill = theme.color_checkbox_off_fill();
        w.inactive.bg_stroke = egui::Stroke::new(1.0, off_border);
        w.inactive.rounding = rounding;
        w.hovered.bg_fill = theme.color_checkbox_hover_fill();
        w.hovered.bg_stroke = egui::Stroke::new(1.0, theme.accent_dim_color());
        w.hovered.rounding = rounding;
        w.active.bg_fill = theme.accent_color();
        w.active.bg_stroke = egui::Stroke::new(1.0, theme.accent_color());
        w.active.rounding = rounding;
        w.active.fg_stroke = egui::Stroke::new(1.8, theme.color_checkbox_checkmark());
        ui.checkbox(value, text)
    })
    .inner
}

/// 标题行右侧操作区宽度（工具按钮 + 关闭 ×；RTL 顺序为 close, …tools）
/// 标题行右侧工具按钮描述（用于预留宽度）
pub struct PanelToolbarSpec<'a> {
    pub icon: Option<IconId>,
    pub label: &'a str,
}

pub fn panel_header_trailing_width(ui: &Ui, theme: &Theme, tool_labels: &[&str]) -> f32 {
    let specs: Vec<PanelToolbarSpec> = tool_labels
        .iter()
        .map(|l| PanelToolbarSpec {
            icon: None,
            label: l,
        })
        .collect();
    panel_header_trailing_width_tools(ui, theme, &specs)
}

pub fn panel_header_trailing_width_tools(
    ui: &Ui,
    theme: &Theme,
    tools: &[PanelToolbarSpec<'_>],
) -> f32 {
    let close_w = theme.size_panel_header_control_h();
    let gap = theme.spacing_panel_gap();
    let pad = theme.spacing_panel_title_pad_x() * 0.5;
    if tools.is_empty() {
        return close_w + pad;
    }
    let tools_w: f32 = tools
        .iter()
        .map(|t| panel_toolbar_button_size(ui, theme, t.label, t.icon.is_some()).x)
        .sum();
    tools_w + gap * tools.len() as f32 + close_w + pad
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
    dock_close_icon_button(ui, theme, close_tooltip).clicked()
}

/// 右 dock 标题行内容区（固定高度，与终端 Tab 条 [`Theme::size_panel_header_row_h`] 对齐）
pub fn dock_header_horizontal<R>(ui: &mut Ui, theme: &Theme, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let row_h = theme.size_panel_header_row_h();
    let row_w = ui.available_width().max(1.0);
    ui.allocate_ui_with_layout(
        egui::vec2(row_w, row_h),
        egui::Layout::left_to_right(egui::Align::Center),
        add_contents,
    )
    .inner
}

/// 仅标题 + 关闭 ×（右侧仅一个图标按钮，避免与 dock 工具栏混排重复）
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
        ui.with_layout(
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.add_space(theme.spacing_dock_panel_trailing_pad());
                if dock_close_icon_button(ui, theme, close_tooltip).clicked() {
                    closed = true;
                }
            },
        );
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

/// 筛选芯片行 + 右侧排序芯片（同一行，不占额外表头行）
pub struct FilterChipRowWithSortResult {
    pub picked: Option<String>,
    pub cycle_sort: bool,
}

pub fn filter_chip_row_with_sort(
    ui: &mut Ui,
    theme: &Theme,
    chips: &[(&str, &str)],
    active_value: &str,
    sort_icon: IconId,
    sort_chip_display: &str,
    sort_hover_tooltip: &str,
) -> FilterChipRowWithSortResult {
    let mut out = FilterChipRowWithSortResult {
        picked: None,
        cycle_sort: false,
    };
    let chip_h = theme.size_panel_filter_chip_h();
    let chip_gap = theme.spacing_panel_gap();
    let sort_gap = theme.spacing_filter_sort_gap();

    egui::Frame::none()
        .outer_margin(egui::Margin {
            left: 0.0,
            right: 0.0,
            top: 2.0,
            bottom: 4.0,
        })
        .show(ui, |ui| {
            let row_w = ui.available_width().max(96.0);
            if theme.uses_modern_palette() {
                let sort_w = panel_sort_chip_width(ui, theme, sort_chip_display);
                ui.horizontal(|ui| {
                    ui.set_max_width(row_w);
                    ui.spacing_mut().item_spacing = egui::vec2(chip_gap, 0.0);
                    let seg_w = (ui.available_width() - sort_w - sort_gap).max(96.0);
                    ui.scope(|ui| {
                        ui.set_max_width(seg_w);
                        if let Some(picked) = segmented_control_row(
                            ui,
                            theme,
                            chips,
                            active_value,
                            Some(ui.available_width().max(96.0)),
                        ) {
                            out.picked = Some(picked);
                        }
                    });
                    ui.add_space(sort_gap);
                    ui.allocate_ui_with_layout(
                        egui::vec2(sort_w, chip_h),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if panel_sort_chip(
                                ui,
                                theme,
                                sort_icon,
                                sort_chip_display,
                                sort_hover_tooltip,
                            )
                            .clicked()
                            {
                                out.cycle_sort = true;
                            }
                        },
                    );
                });
            } else {
                let sort_w = panel_sort_chip_width(ui, theme, sort_chip_display);
                ui.horizontal(|ui| {
                    ui.set_max_width(row_w);
                    ui.spacing_mut().item_spacing = egui::vec2(chip_gap, 0.0);
                    let chips_w = (ui.available_width() - sort_w - sort_gap).max(96.0);
                    ui.scope(|ui| {
                        ui.set_max_width(chips_w);
                        let n = chips.len().max(1) as f32;
                        let max_w = theme.size_panel_filter_chip_max_w();
                        let even_w = ((chips_w - chip_gap * (n - 1.0)) / n)
                            .max(theme.size_panel_header_btn_min_w());
                        let item_w = even_w.min(max_w);
                        for (value, chip_label) in chips {
                            let is_active = active_value == *value;
                            if filter_chip_button(
                                ui,
                                theme,
                                chip_label,
                                is_active,
                                egui::vec2(item_w, chip_h),
                            )
                            .clicked()
                            {
                                out.picked = Some((*value).to_string());
                            }
                        }
                    });
                    ui.add_space(sort_gap);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if panel_sort_chip(
                            ui,
                            theme,
                            sort_icon,
                            sort_chip_display,
                            sort_hover_tooltip,
                        )
                        .clicked()
                        {
                            out.cycle_sort = true;
                        }
                    });
                });
            }
        });
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

/// 片段列表标题行：标签列宽（随文案测量，带上限）。
fn fragment_list_tag_column_width(
    ui: &Ui,
    theme: &Theme,
    tag_label: &str,
    content_w: f32,
) -> f32 {
    let tag_pad = theme.spacing_fragment_tag_inner_x();
    let tag_font = egui::FontId::proportional(theme.font_size_fragment_tag());
    let tag_color = theme.color_fragment_tag_text();
    let tag_text_w = ui
        .painter()
        .layout_no_wrap(tag_label.to_owned(), tag_font, tag_color)
        .size()
        .x;
    let tag_w_desired = tag_text_w + 2.0 * tag_pad;
    let tag_cap = content_w * theme.fragment_list_tag_max_width_frac();
    tag_w_desired.min(tag_cap).min(content_w)
}

/// 命令片段侧栏单行：首行「标题 + 右对齐标签」，下接命令与统计。
pub fn fragment_list_row(ui: &mut Ui, theme: &Theme, row: FragmentListRow<'_>) -> FragmentListRowResponse {
    let pad_x = theme.spacing_fragment_row_pad_x();
    let pad_y = theme.spacing_fragment_row_pad_y();
    let gap = theme.spacing_fragment_row_tag_gap();
    let line_gap = theme.spacing_fragment_row_line_gap();
    let title_px = theme.font_size_fragment_title();
    let tag_px = theme.font_size_fragment_tag();
    let title_line_h =
        title_px.max(tag_px) + theme.spacing_fragment_title_line_pad();

    let row_w = crate::ui::layout_util::side_panel_row_width(ui);
    let content_w = (row_w - 2.0 * pad_x).max(0.0);
    let tag_col_w = fragment_list_tag_column_width(ui, theme, row.tag_label, content_w);
    let title_col_w = (content_w - gap - tag_col_w).max(0.0);
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
    let mut row_ui = ui.child_ui(inner, egui::Layout::top_down(egui::Align::LEFT));
    row_ui.set_max_width(content_w);
    row_ui.spacing_mut().item_spacing.y = line_gap;

    let title_resp = row_ui
        .horizontal(|ui| {
            ui.set_max_width(content_w);
            ui.spacing_mut().item_spacing.x = gap;
            let title = ui
                .allocate_ui_with_layout(
                    egui::vec2(title_col_w, title_line_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_max_width(title_col_w);
                        ui.add(
                            egui::Label::new(
                                RichText::new(row.title)
                                    .size(title_px)
                                    .color(theme.accent_color()),
                            )
                            .truncate(true)
                            .sense(egui::Sense::click()),
                        )
                        .on_hover_text(row.command)
                    },
                )
                .inner;
            if tag_col_w > 0.0 {
                ui.allocate_ui_with_layout(
                    egui::vec2(tag_col_w, title_line_h),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.set_max_width(tag_col_w);
                        ui.add(
                            egui::Label::new(
                                RichText::new(row.tag_label)
                                    .size(tag_px)
                                    .color(theme.color_fragment_tag_text()),
                            )
                            .truncate(true),
                        )
                        .on_hover_text(row.tag_label);
                    },
                );
            }
            title
        })
        .inner;

    let cmd_trim = row.command.trim();
    row_ui
        .add(
            egui::Label::new(
                RichText::new(cmd_trim)
                    .size(theme.font_size_fragment_cmd())
                    .monospace()
                    .color(theme.color_status_bar_conn()),
            )
            .truncate(true),
        )
        .on_hover_text(cmd_trim);

    row_ui.add(
        egui::Label::new(
            RichText::new(row.stats_line)
                .size(theme.font_size_fragment_stats())
                .color(theme.color_caption_text()),
        )
        .truncate(true),
    );

    FragmentListRowResponse {
        row: row_response,
        title: title_resp,
    }
}

/// 工具栏 Button Group 一项
pub struct ButtonGroupAction<'a> {
    pub icon: IconId,
    pub label: &'a str,
    pub enabled: bool,
    pub tooltip: &'a str,
}

/// 胶囊 Segmented Control（modern）；其它主题回退为独立 filter chip。
pub fn segmented_control_row(
    ui: &mut Ui,
    theme: &Theme,
    items: &[(&str, &str)],
    active_value: &str,
    row_width: Option<f32>,
) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    if !theme.uses_modern_palette() {
        let mut picked = None;
        let chip_h = theme.size_panel_filter_chip_h();
        let chip_gap = theme.spacing_panel_gap();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(chip_gap, 0.0);
            let n = items.len() as f32;
            let avail = row_width.unwrap_or(ui.available_width()).max(96.0);
            let item_w = ((avail - chip_gap * (n - 1.0)) / n)
                .max(theme.size_panel_header_btn_min_w());
            for (value, label) in items {
                if filter_chip_button(
                    ui,
                    theme,
                    label,
                    active_value == *value,
                    egui::vec2(item_w, chip_h),
                )
                .clicked()
                {
                    picked = Some((*value).to_string());
                }
            }
        });
        return picked;
    }

    let track_pad = theme.spacing_segment_track_pad();
    let item_pad_x = theme.spacing_segment_item_x();
    let font = egui::FontId::proportional(theme.font_size_category_label());
    let mut seg_widths = Vec::with_capacity(items.len());
    for (_, label) in items {
        let text_w = ui
            .painter()
            .layout_no_wrap(label.to_string(), font.clone(), theme.text_primary())
            .size()
            .x;
        seg_widths.push((text_w + item_pad_x * 2.0).max(44.0));
    }
    let inner_w: f32 = seg_widths.iter().sum();
    let track_w = row_width
        .unwrap_or(inner_w + track_pad * 2.0)
        .max(inner_w + track_pad * 2.0);
    let track_h = theme.size_panel_filter_chip_h() + track_pad * 2.0;
    let (track_rect, track_resp) =
        ui.allocate_exact_size(egui::vec2(track_w, track_h), Sense::hover());
    let track_rect = theme.snap_rect_to_pixels(ui.ctx(), track_rect);

    ui.painter().rect(
        track_rect,
        egui::Rounding::same(theme.radius_segment_track()),
        theme.color_segment_track(),
        Stroke::NONE,
    );

    let seg_total = if row_width.is_some() {
        track_w - track_pad * 2.0
    } else {
        inner_w
    };
    let seg_gap = 0.0_f32;
    let mut picked = None;
    let mut x = track_rect.min.x + track_pad;
    let thumb_inset = 1.0;
    let _thumb_h = track_h - track_pad * 2.0;
    let thumb_rounding = egui::Rounding::same(theme.radius_segment_thumb());

    for (idx, (value, label)) in items.iter().enumerate() {
        let seg_w = if row_width.is_some() {
            (seg_total - seg_gap * (items.len() as f32 - 1.0)) / items.len() as f32
        } else {
            seg_widths[idx]
        };
        let seg_rect = egui::Rect::from_min_size(egui::pos2(x, track_rect.min.y), egui::vec2(seg_w, track_h));
        let active = active_value == *value;
        if active {
            let thumb_rect = theme.snap_rect_to_pixels(
                ui.ctx(),
                seg_rect.shrink2(egui::vec2(thumb_inset, track_pad)),
            );
            ui.painter().rect(
                thumb_rect,
                thumb_rounding,
                theme.color_segment_thumb(),
                Stroke::NONE,
            );
        }
        let text_color = if active {
            theme.color_segment_thumb_text()
        } else {
            theme.color_segment_idle_text()
        };
        let resp = ui.interact(seg_rect, ui.id().with(("seg", idx)), Sense::click());
        paint_caption_in_rect_center(ui, seg_rect, label, theme.font_size_category_label(), text_color);
        if resp.clicked() {
            picked = Some((*value).to_string());
        }
        if resp.hovered() || resp.clicked() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        x += seg_w + seg_gap;
    }
    let _ = track_resp;
    picked
}

/// 半透明工具栏 Button Group（modern）；返回被点击项索引。
pub fn button_group_toolbar(
    ui: &mut Ui,
    theme: &Theme,
    actions: &[ButtonGroupAction<'_>],
    expand_width: Option<f32>,
    id_salt: impl std::hash::Hash,
) -> Option<usize> {
    if actions.is_empty() {
        return None;
    }
    if !theme.uses_modern_palette() {
        return None;
    }

    ui.push_id(id_salt, |ui| {
    let pad = theme.spacing_button_group_pad();
    let icon_px = theme.size_icon_glyph();
    let gap = 4.0;
    let font = theme.font_size_control_btn();
    let mut item_widths = Vec::with_capacity(actions.len());
    for action in actions {
        let w = if action.label.is_empty() {
            (pad * 2.0 + icon_px).max(theme.size_control_btn_min_w() * 0.72)
        } else {
            let text_w = ui
                .painter()
                .layout_no_wrap(
                    action.label.to_string(),
                    egui::FontId::proportional(font),
                    theme.text_primary(),
                )
                .size()
                .x;
            (pad * 2.0 + icon_px + gap + text_w).max(theme.size_control_btn_min_w())
        };
        item_widths.push(w);
    }
    let items_w: f32 = item_widths.iter().sum();
    let group_w = expand_width.unwrap_or(items_w).max(items_w);
    let group_h = theme.size_control_btn_h();
    let (group_rect, _) = ui.allocate_exact_size(egui::vec2(group_w, group_h), Sense::hover());
    let group_rect = theme.snap_rect_to_pixels(ui.ctx(), group_rect);
    let hairline = theme.hairline_width(ui.ctx());
    ui.painter().rect(
        group_rect,
        egui::Rounding::same(theme.radius_list_item()),
        theme.color_button_group_fill(),
        Stroke::NONE,
    );

    let mut clicked_idx = None;
    let mut x = group_rect.min.x;
    for (idx, action) in actions.iter().enumerate() {
        let w = item_widths[idx];
        let item_rect = egui::Rect::from_min_size(egui::pos2(x, group_rect.min.y), egui::vec2(w, group_h));
        if idx > 0 {
            let x = theme.snap_x_to_pixel(ui.ctx(), item_rect.left());
            ui.painter().vline(
                x,
                item_rect.center().y - group_h * 0.28..=item_rect.center().y + group_h * 0.28,
                Stroke::new(hairline, theme.color_button_group_divider()),
            );
        }
        let sense = if action.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let resp = ui.interact(item_rect, ui.id().with(("bgrp", idx)), sense);
        let text_color = if action.enabled {
            if resp.hovered() {
                theme.text_primary()
            } else {
                theme.text_secondary().gamma_multiply(0.85)
            }
        } else {
            theme.color_control_disabled_text()
        };
        let icon_color = text_color;
        if resp.hovered() && action.enabled {
            ui.painter().rect(
                item_rect.shrink(2.0),
                egui::Rounding::same(theme.radius_list_item() - 1.0),
                theme.color_widget_hover_fill(),
                Stroke::NONE,
            );
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if action.label.is_empty() {
            let icon_rect = egui::Rect::from_center_size(
                item_rect.center(),
                egui::vec2(icon_px, icon_px),
            );
            icons::paint_icon(ui, icon_rect, action.icon, icon_color, icon_px);
        } else {
            paint_icon_caption_row_in_rect(
                ui,
                item_rect,
                action.icon,
                action.label,
                icon_px,
                gap,
                font,
                text_color,
                icon_color,
                pad,
                false,
            );
        }
        if resp.clicked() && action.enabled {
            clicked_idx = Some(idx);
        }
        x += w;
    }
    clicked_idx
    })
    .inner
}

// ── SFTP 面板专用工具条（modern：地址栏一体包 + 悬停肉垫 + 幽灵提交） ──

/// SFTP 工具条行容器：统一高度、垂直居中、深色一体底。
pub fn sftp_toolbar_band<R>(ui: &mut Ui, theme: &Theme, add: impl FnOnce(&mut Ui, &Theme) -> R) -> R {
    if !theme.uses_modern_palette() {
        return add(ui, theme);
    }
    let row_h = theme.size_sftp_toolbar_row_h();
    theme.frame_sftp_toolbar_band().show(ui, |ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width().max(1.0), row_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| add(ui, theme),
        )
        .inner
    })
    .inner
}

/// 地址栏内嵌路径输入（无下划线、透明底，随剩余宽度伸缩）。
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
            .frame(false)
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

fn sftp_toolbar_action_size(
    ui: &Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
) -> egui::Vec2 {
    let _ = icon;
    let icon_px = theme.size_icon_glyph();
    let pad_x = theme.spacing_button_group_pad() + theme.spacing_xs();
    let gap = theme.spacing_xs();
    let font = theme.font_size_control_btn();
    let text_w = if label.is_empty() {
        0.0
    } else {
        ui.painter()
            .layout_no_wrap(
                label.to_string(),
                egui::FontId::proportional(font),
                theme.text_primary(),
            )
            .size()
            .x
    };
    let content_w = if label.is_empty() {
        icon_px + pad_x * 2.0
    } else {
        pad_x * 2.0 + icon_px + gap + text_w
    };
    egui::vec2(
        content_w.max(theme.size_panel_header_btn_min_w() * 0.65),
        theme.size_control_btn_h(),
    )
}

/// SFTP 工具条内单颗操作：白字 + 图标，idle 透明，hover 圆角灰垫 + 小手。
pub fn sftp_toolbar_action_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    tooltip: &str,
    enabled: bool,
    _id: egui::Id,
) -> Response {
    if !theme.uses_modern_palette() {
        return if label.is_empty() {
            panel_action_icon_button_ex(ui, theme, icon, tooltip, enabled)
        } else {
            panel_action_button_with_icon_ex(ui, theme, icon, label, enabled)
        };
    }

    let size = sftp_toolbar_action_size(ui, theme, icon, label);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let rounding = egui::Rounding::same(theme.radius_sftp_toolbar_action());
    let hovered = response.hovered() && enabled;
    let pressed = response.is_pointer_button_down_on() && enabled;
    if hovered || pressed {
        ui.ctx().request_repaint();
    }
    let fill = if !enabled {
        Color32::TRANSPARENT
    } else if pressed {
        theme.color_widget_active_fill()
    } else if hovered {
        theme.color_sftp_toolbar_action_hover()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect(rect, rounding, fill, Stroke::NONE);
    let text_color = if enabled {
        theme.text_primary()
    } else {
        theme.color_control_disabled_text()
    };
    let icon_px = theme.size_icon_glyph();
    let pad = theme.spacing_button_group_pad() + theme.spacing_xs();
    if label.is_empty() {
        let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(icon_px, icon_px));
        icons::paint_icon(ui, icon_rect, icon, text_color, icon_px);
    } else {
        paint_icon_caption_row_in_rect(
            ui,
            rect,
            icon,
            label,
            icon_px,
            theme.spacing_xs(),
            theme.font_size_control_btn(),
            text_color,
            text_color,
            pad,
            false,
        );
    }
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    } else if response.hovered() && !enabled {
        ui.ctx().set_cursor_icon(CursorIcon::NotAllowed);
    }
    response.on_hover_text(tooltip)
}

/// SFTP 工具条内一组操作（前往 / 上传等）；返回被点击项索引。
pub fn sftp_toolbar_actions(
    ui: &mut Ui,
    theme: &Theme,
    actions: &[ButtonGroupAction<'_>],
    id_salt: impl std::hash::Hash,
) -> Option<usize> {
    if !theme.uses_modern_palette() {
        return None;
    }
    let mut clicked = None;
    ui.push_id(id_salt, |ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_xs();
        for (idx, action) in actions.iter().enumerate() {
            let tip = if action.tooltip.is_empty() {
                action.label
            } else {
                action.tooltip
            };
            if sftp_toolbar_action_button(
                ui,
                theme,
                action.icon,
                action.label,
                tip,
                action.enabled,
                ui.id().with(idx),
            )
            .clicked()
                && action.enabled
            {
                clicked = Some(idx);
            }
        }
    });
    clicked
}

/// SFTP 幽灵提交按钮（「+ 创建」：淡描边 + 加宽内边距，hover 填灰）。
pub fn sftp_ghost_submit_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    enabled: bool,
) -> Response {
    if !theme.uses_modern_palette() {
        return panel_action_button_with_icon_ex(ui, theme, icon, label, enabled);
    }

    let icon_px = theme.size_icon_glyph();
    let pad_x = theme.spacing_sm() + theme.spacing_panel_gap();
    let font = theme.font_size_control_btn();
    let text_w = ui
        .painter()
        .layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(font),
            theme.text_primary(),
        )
        .size()
        .x;
    let gap = theme.spacing_xs();
    let w = (pad_x * 2.0 + icon_px + gap + text_w).max(theme.size_control_btn_min_w());
    let h = theme.size_control_btn_h();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click());
    let rounding = egui::Rounding::same(theme.radius_sftp_toolbar_action());
    let hovered = response.hovered() && enabled;
    let pressed = response.is_pointer_button_down_on() && enabled;
    if hovered || pressed {
        ui.ctx().request_repaint();
    }
    let hairline = theme.hairline_width(ui.ctx());
    let stroke = Stroke::new(hairline, theme.color_sftp_ghost_btn_stroke());
    let fill = if !enabled {
        Color32::TRANSPARENT
    } else if pressed {
        theme.color_widget_active_fill()
    } else if hovered {
        theme.color_sftp_toolbar_action_hover()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect(rect, rounding, fill, stroke);
    let text_color = if enabled {
        theme.text_primary()
    } else {
        theme.color_control_disabled_text()
    };
    paint_icon_caption_row_in_rect(
        ui,
        rect,
        icon,
        label,
        icon_px,
        gap,
        font,
        text_color,
        text_color,
        pad_x,
        false,
    );
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    } else if response.hovered() && !enabled {
        ui.ctx().set_cursor_icon(CursorIcon::NotAllowed);
    }
    response
}

/// SFTP 路径行：单独一条深色长条，仅含路径输入。
pub fn sftp_path_toolbar_row(
    ui: &mut Ui,
    theme: &Theme,
    path_id: egui::Id,
    path_text: &mut String,
    path_hint: &str,
) -> Response {
    if !theme.uses_modern_palette() {
        let w = ui.available_width().max(96.0);
        return form_singleline_field(ui, theme, path_id, path_text, path_hint, w, false);
    }
    sftp_toolbar_band(ui, theme, |ui, theme| {
        crate::ui::layout_util::set_width_to_available(ui);
        form_singleline_field_sftp_embedded(ui, theme, path_id, path_text, path_hint)
    })
}

/// SFTP 导航行：单独一条深色长条，仅含操作按钮。
pub fn sftp_nav_toolbar_row(
    ui: &mut Ui,
    theme: &Theme,
    nav_actions: &[ButtonGroupAction<'_>],
    nav_id_salt: impl std::hash::Hash,
) -> Option<usize> {
    if !theme.uses_modern_palette() {
        return None;
    }
    sftp_toolbar_band(ui, theme, |ui, theme| {
        sftp_toolbar_actions(ui, theme, nav_actions, nav_id_salt)
    })
}

/// 均分宽度的筛选芯片行（常用/Docker、全部/在线/离线等）
pub fn filter_chip_row(
    ui: &mut Ui,
    theme: &Theme,
    labels: &[&str],
    active: &str,
    panel_w: f32,
) -> Option<String> {
    let chips: Vec<(&str, &str)> = labels.iter().map(|l| (*l, *l)).collect();
    segmented_control_row(ui, theme, &chips, active, Some(panel_w))
}

/// 分类筛选芯片（全部/在线/离线、常用/Docker 等）
pub fn filter_chip_button(
    ui: &mut Ui,
    theme: &Theme,
    label: &str,
    active: bool,
    min_size: egui::Vec2,
) -> Response {
    let modern = theme.uses_modern_palette();
    let text_color = if active {
        theme.color_filter_chip_active_text()
    } else if modern {
        theme.text_secondary().gamma_multiply(0.72)
    } else {
        theme.color_filter_chip_inactive_text()
    };
    let rounding = if modern {
        egui::Rounding::ZERO
    } else {
        egui::Rounding::same(theme.radius_category())
    };
    let (rect, response) = ui.allocate_exact_size(min_size, Sense::click());
    let hovered = response.hovered();
    let fill = if modern {
        if hovered && !active {
            theme.color_widget_hover_fill()
        } else {
            Color32::TRANSPARENT
        }
    } else if active {
        theme.color_filter_chip_active_fill()
    } else {
        theme.color_overlay_fill_subtle()
    };
    ui.painter().rect(rect, rounding, fill, egui::Stroke::NONE);
    if active && modern {
        paint_vscode_tab_bottom_indicator(ui.painter(), ui.ctx(), rect, theme);
    }
    paint_caption_in_rect_center(
        ui,
        rect,
        label,
        theme.font_size_category_label(),
        text_color,
    );
    response
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

    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(row_w, row_h), sense);

    if ui.is_enabled() {
        let visuals = ui.style().interact_selectable(&response, enabled);
        if enabled && (response.hovered() || response.has_focus()) {
            ui.painter().rect_filled(rect, 0.0, visuals.bg_fill);
        }
        let title_pos = egui::pos2(
            rect.min.x + pad.x,
            rect.center().y - title_g.size().y * 0.5,
        );
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
pub fn popup_menu_button_accel_shift(ui: &mut Ui, theme: &Theme, title: &str, key: &str) -> Response {
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

/// 偏好 / 设置区小节标题（与表单标签区分层级）
pub fn form_section_heading(theme: &Theme, text: &str) -> RichText {
    RichText::new(text)
        .size(theme.font_size_panel_title())
        .strong()
        .color(theme.color_form_label())
}

/// 表单输入区临时视觉（modern 下划线：透明 TextEdit 底）
fn with_underline_field_visuals<R>(ui: &mut Ui, theme: &Theme, add: impl FnOnce(&mut Ui) -> R) -> R {
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

fn paint_form_field_underline(ui: &Ui, theme: &Theme, rect: egui::Rect, focused: bool) {
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

/// 输入框占位符 RichText（斜体 + 弱色，与输入正文区分）
pub fn hint_rich(theme: &Theme, text: &str, font_size: f32) -> RichText {
    RichText::new(text)
        .size(font_size)
        .italics()
        .color(theme.color_form_hint())
}

/// 单行输入框（modern：透明底 + 底边线；其它主题：带底+描边）
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
                .frame(false)
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

/// 多行输入框（modern：透明底 + 底边线）
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
    let underline = theme.uses_underline_inputs();
    let inner_w = if underline {
        desired_width.max(48.0)
    } else {
        (desired_width - theme.spacing_search_input_x() * 2.0 - 4.0).max(48.0)
    };
    let shown = theme.frame_form_text_input(focused).show(ui, |ui| {
        with_underline_field_visuals(ui, theme, |ui| {
            let mut edit = egui::TextEdit::multiline(text)
                .id(id)
                .frame(false)
                .desired_width(inner_w)
                .desired_rows(rows)
                .text_color(theme.color_text_input_text())
                .font(egui::FontId::proportional(theme.font_size_control_input()));
            if password {
                edit = edit.password(true);
            }
            ui.add(edit)
        })
    });
    if underline {
        paint_form_field_underline(ui, theme, shown.response.rect, focused);
    }
    shown.inner
}

/// 只读多行文本：支持鼠标拖选与 Ctrl/Cmd+C（`&str` 缓冲不可编辑）。
/// 带可见滑轨的水平滑块（全局 `inactive.bg_fill` 为透明时仍绘制轨道）。
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

/// 带可见滑轨的水平滑块（`f64` 版本，如刷新间隔秒数）。
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
            .frame(false)
            .margin(egui::vec2(0.0, 0.0))
            .desired_width(desired_width.max(1.0))
            .code_editor(),
    )
}

/// 搜索框（左侧 🔍 + 与表单相同的底/描边/字号）；`desired_width` 为外框总宽（含描边）。
pub fn search_field(
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
    let stroke = theme.stroke_width_panel();
    let row_h = font + pad_y * 2.0 + stroke * 2.0;
    let mut outer_w = desired_width;
    if ui.max_rect().width().is_finite() {
        outer_w = outer_w.min(ui.max_rect().width());
    }
    outer_w = outer_w.min(ui.available_width()).max(72.0);

    ui.allocate_ui_with_layout(
        egui::vec2(outer_w, row_h),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            ui.set_width(outer_w);
            theme.frame_form_text_input(focused).show(ui, |ui| {
                ui.set_width(outer_w);
                ui.horizontal(|ui| {
                    ui.set_max_width(outer_w);
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm();
                    let (r, _) =
                        ui.allocate_exact_size(egui::vec2(font, font), egui::Sense::hover());
                    icons::paint_icon(ui, r, IconId::Search, theme.text_tertiary(), font);
                    let text_w = (outer_w - font - theme.spacing_sm() - pad_x * 2.0 - stroke * 2.0)
                        .max(48.0);
                    let prev_override = ui.style_mut().visuals.override_text_color;
                    ui.style_mut().visuals.override_text_color = Some(theme.color_form_hint());
                    let response = ui.add(
                        egui::TextEdit::singleline(query)
                            .id(id)
                            .frame(false)
                            .hint_text(hint_rich(theme, hint, font))
                            .text_color(theme.color_text_input_text())
                            .font(egui::FontId::proportional(font))
                            .desired_width(text_w),
                    );
                    ui.style_mut().visuals.override_text_color = prev_override;
                    response
                })
                .inner
            })
            .inner
        },
    )
    .inner
}

/// 面板内搜索行：左右留白与侧栏一致；`content_w` 为面板正文宽（右 dock 须传入，避免 outer_margin 撑出裁切）。
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
            left: 0.0,
            right: 0.0,
            top: 4.0,
            bottom: 6.0,
        }
    } else {
        theme.spacing_sidebar_search_outer()
    };
    let inset_x = margin.left + margin.right;
    let stroke_pad = theme.stroke_width_panel() * 2.0 + 1.0;
    let cap = content_w.unwrap_or_else(|| {
        crate::ui::layout_util::set_width_to_available(ui)
    });
    let search_w = (cap - inset_x - stroke_pad).max(72.0);
    egui::Frame::none()
        .outer_margin(margin)
        .show(ui, |ui| search_field(ui, theme, id, query, hint, search_w))
        .inner
}

/// 侧栏搜索框（[`panel_search_row`] 别名）
pub fn sidebar_search_field(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    query: &mut String,
    hint: &str,
    desired_width: f32,
) -> Response {
    let _ = desired_width;
    panel_search_row(ui, theme, id, query, hint, None)
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
        theme.divider_stroke(),
    );

    let msg = match crate::i18n::language(ui.ctx()) {
        crate::i18n::UiLanguage::En => format!(
            "Detected {} pending SSH Host block(s)",
            pending_count,
        ),
        crate::i18n::UiLanguage::Zh => format!(
            "检测到 {} 个未导入的 SSH 配置",
            pending_count,
        ),
    };
    let inner = rect.shrink2(egui::vec2(10.0, 0.0));
    ui.allocate_ui_at_rect(inner, |ui| {
        ui.set_height(BAR_H);
        ui.horizontal_centered(|ui| {
            let (ar, _) =
                ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
            icons::paint_icon(
                ui,
                ar,
                IconId::Alert,
                theme.amber_color(),
                theme.font_size_title_bar_info(),
            );
            ui.add_space(theme.spacing_tool_btn_gap() + 1.0);
            let label = ui.label(
                RichText::new(&msg)
                    .size(theme.font_size_title_bar_info())
                    .color(theme.text_tertiary()),
            );
            label.on_hover_text(&msg);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, theme, IconId::Close, theme.color_caption_text())
                    .on_hover_text(crate::i18n::tr(ui.ctx(), "Dismiss hint", "关闭提示"))
                    .clicked()
                {
                    action.dismiss = true;
                }
                ui.add_space(theme.spacing_region_gap());
                if chrome_small_accent_button(
                    ui,
                    theme,
                    crate::i18n::tr(ui.ctx(), "Import", "导入"),
                )
                    .on_hover_text(crate::i18n::tr(
                        ui.ctx(),
                        "Open SSH config import",
                        "打开 SSH 配置导入",
                    ))
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

/// 状态栏内容区可用高度（与底栏 Panel 内边距一致）。
pub fn status_bar_content_height(theme: &Theme) -> f32 {
    theme.chrome_bar_content_height(theme.status_bar_height())
}

/// 状态栏文字徽章（统一字号；由父级 `Align::Center` 负责垂直对齐）。
pub fn status_text_chip(
    ui: &mut Ui,
    theme: &Theme,
    text: &str,
    color: Color32,
) -> Response {
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

/// 状态栏工具按钮：图标 + 短标签（比纯图标更易识别）。
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

/// 状态栏带小图标的文字 chip（如自动重连）
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

/// 右 dock SSH 门闩：返回 `true` 表示已连接可继续绘制面板正文。
pub fn show_right_dock_ssh_gate(
    ui: &mut Ui,
    theme: &Theme,
    ctx: &egui::Context,
    terminal: Option<&crate::ui::terminal::TerminalView>,
    no_session_en: &'static str,
    no_session_zh: &'static str,
) -> bool {
    use crate::ui::terminal::RightDockSshGate;
    let Some(t) = terminal else {
        ui.label(
            RichText::new(crate::i18n::tr(ctx, no_session_en, no_session_zh))
                .color(theme.text_tertiary()),
        );
        return false;
    };
    match t.right_dock_ssh_gate() {
        RightDockSshGate::Ready => true,
        RightDockSshGate::Connecting => {
            busy_row(
                ui,
                theme,
                crate::i18n::tr(ctx, "Connecting…", "连接建立中…"),
            );
            false
        }
        RightDockSshGate::Disconnected => {
            ui.label(
                RichText::new(crate::i18n::tr(
                    ctx,
                    "SSH disconnected. Reconnect the tab to use this panel.",
                    "SSH 已断开。请重连当前标签后再使用此面板。",
                ))
                .color(theme.amber_color()),
            );
            false
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
            false
        }
        RightDockSshGate::NoSession => {
            ui.label(
                RichText::new(crate::i18n::tr(ctx, no_session_en, no_session_zh))
                    .color(theme.text_tertiary()),
            );
            false
        }
    }
}

/// 只读信息标签（连接元信息、侧栏分组等）
pub fn label_tag_chip(
    ui: &mut Ui,
    theme: &Theme,
    text: &str,
    font_size: f32,
    text_color: Color32,
) {
    theme.frame_label_tag().show(ui, |ui| {
        ui.label(RichText::new(text).size(font_size).color(text_color));
    });
}

/// 面板折叠后状态栏复原按钮（图标 + 名称 · N）
pub fn status_restore_chip(ui: &mut Ui, theme: &Theme, name: &str, count: usize) -> Response {
    let label = format!("{name} · {count}");
    let bar_h = theme.chrome_bar_content_height(theme.status_bar_height());
    let icon_px = theme.font_size_restore_btn();
    let font = egui::FontId::proportional(theme.font_size_restore_btn());
    let color = theme.accent_alpha(89);
    let text_w = ui
        .painter()
        .layout_no_wrap(label.clone(), font.clone(), color)
        .size()
        .x;
    let w = icon_px + 4.0 + text_w + 6.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(w, bar_h), Sense::click());
    paint_icon_caption_row_in_rect(
        ui,
        rect,
        IconId::ChevronRight,
        &label,
        icon_px,
        4.0,
        theme.font_size_restore_btn(),
        color,
        color,
        2.0,
        false,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

/// 弹窗标题行（仅标题，无 ×）。与底部「取消」成对，避免与标题栏关闭重复。
///
/// 标题底带需要横贯整个弹窗宽度（含越过 `modal_content_frame.inner_margin` 的 `mx` 留白），
/// 所以在 frame 内部用 `horizontal` + `right_to_left` 占位强制填满可用宽度，
/// 否则 frame 会按 `panel_header_title_leading` 的自然宽度收缩成左上角小 chip。
pub fn modal_header_title_only(ui: &mut Ui, theme: &Theme, title: &str, title_px: f32) {
    let _ = title_px;
    let mx = theme.spacing_modal_content_x();
    let my = theme.spacing_modal_content_y();
    theme
        .frame_modal_title_band()
        .stroke(egui::Stroke::new(1.0, theme.color_panel_header_divider()))
        .outer_margin(egui::Margin {
            left: -mx,
            right: -mx,
            top: -my,
            bottom: 0.0,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                panel_header_title_leading(ui, theme, IconId::Plus, title);
                // 用右对齐空 layout 吃掉剩余宽度，迫使 frame 横向铺满。
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |_ui| {},
                );
            });
        });
    ui.add_space(theme.spacing_modal_header_after_sep());
}

/// 弹窗标题行（标题 + 右侧 ×，用于仅通过标题栏关闭的弹窗）。
pub fn modal_header(ui: &mut Ui, theme: &Theme, title: &str, title_px: f32) -> bool {
    let _ = title_px;
    let mx = theme.spacing_modal_content_x();
    let my = theme.spacing_modal_content_y();
    let mut closed = false;
    theme.frame_modal_title_band()
        .stroke(egui::Stroke::new(1.0, theme.color_panel_header_divider()))
        .outer_margin(egui::Margin {
            left: -mx,
            right: -mx,
            top: -my,
            bottom: 0.0,
        })
        .show(ui, |ui| {
        ui.horizontal(|ui| {
            panel_header_title_leading(ui, theme, IconId::Plus, title);
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    if close_icon_button(ui, theme).clicked() {
                        closed = true;
                    }
                },
            );
        });
    });
    ui.add_space(theme.spacing_modal_header_after_sep());
    closed
}

/// 右侧 dock 标题行（标题 + 关闭 ×）。
#[inline]
pub fn side_panel_title_row(ui: &mut Ui, theme: &Theme, title: &str) -> bool {
    dock_panel_title_close_only(
        ui,
        theme,
        IconId::Plug,
        title,
        crate::i18n::tr(ui.ctx(), "Close", "关闭"),
    )
}

/// 侧栏小标题 + 右侧关闭（与 [`dock_panel_title_close_only`] 相同布局）。
#[inline]
pub fn side_panel_section_title(
    ui: &mut Ui,
    theme: &Theme,
    title: &str,
    _title_color: Color32,
) -> bool {
    dock_panel_title_close_only(
        ui,
        theme,
        IconId::Plug,
        title,
        crate::i18n::tr(ui.ctx(), "Close", "关闭"),
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
    paint_control_button(
        ui,
        theme,
        label,
        None,
        ControlButtonVariant::Primary,
        theme.size_modal_footer_btn_min_w_primary(),
        can_activate,
    )
}

fn paint_modal_secondary_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    paint_control_button(
        ui,
        theme,
        label,
        None,
        ControlButtonVariant::Secondary,
        theme.size_modal_footer_btn_min_w_secondary(),
        true,
    )
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
    paint_caption_in_rect_center(
        ui,
        rect,
        label,
        theme.font_size_normal(),
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

pub fn modal_secondary_icon_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
) -> Response {
    let label_size = control_button_size(
        ui,
        theme,
        tooltip,
        true,
        theme.size_modal_footer_btn_min_w_secondary(),
    );
    let response = if ui.available_width() >= label_size.x {
        paint_control_button(
            ui,
            theme,
            tooltip,
            Some(icon),
            ControlButtonVariant::Secondary,
            theme.size_modal_footer_btn_min_w_secondary(),
            true,
        )
    } else {
        paint_icon_only_button(
            ui,
            theme,
            icon,
            ControlButtonVariant::Secondary,
            theme.size_modal_footer_btn_min_w_secondary(),
            true,
        )
    };
    response.on_hover_text(tooltip)
}

pub fn modal_primary_icon_button(ui: &mut Ui, theme: &Theme, icon: IconId, tooltip: &str) -> Response {
    modal_primary_icon_button_ex(ui, theme, icon, tooltip, true)
}

pub fn modal_primary_icon_button_ex(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    tooltip: &str,
    can_activate: bool,
) -> Response {
    let label_size = control_button_size(
        ui,
        theme,
        tooltip,
        true,
        theme.size_modal_footer_btn_min_w_primary(),
    );
    let response = if ui.available_width() >= label_size.x {
        paint_control_button(
            ui,
            theme,
            tooltip,
            Some(icon),
            ControlButtonVariant::Primary,
            theme.size_modal_footer_btn_min_w_primary(),
            can_activate,
        )
    } else {
        paint_icon_only_button(
            ui,
            theme,
            icon,
            ControlButtonVariant::Primary,
            theme.size_modal_footer_btn_min_w_primary(),
            can_activate,
        )
    };
    response.on_hover_text(tooltip)
}

/// 弹窗底栏主操作（纯图标），用于 `ui.add(...)`。
pub struct ModalPrimaryIconButton<'a> {
    theme: &'a Theme,
    icon: IconId,
    tooltip: &'a str,
    can_activate: bool,
}

impl Widget for ModalPrimaryIconButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        modal_primary_icon_button_ex(
            ui,
            self.theme,
            self.icon,
            self.tooltip,
            self.can_activate,
        )
    }
}

impl ModalPrimaryIconButton<'_> {
    pub fn can_activate(mut self, can: bool) -> Self {
        self.can_activate = can;
        self
    }
}

pub fn modal_primary_icon_button_widget<'a>(
    theme: &'a Theme,
    icon: IconId,
    tooltip: &'a str,
) -> ModalPrimaryIconButton<'a> {
    ModalPrimaryIconButton {
        theme,
        icon,
        tooltip,
        can_activate: true,
    }
}

/// 弹窗底栏主操作：图标 + 可见文字。
pub fn modal_primary_button_with_icon_ex(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    can_activate: bool,
) -> Response {
    paint_control_button(
        ui,
        theme,
        label,
        Some(icon),
        ControlButtonVariant::Primary,
        theme.size_modal_footer_btn_min_w_primary(),
        can_activate,
    )
}

pub fn modal_primary_button_with_icon(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
) -> Response {
    modal_primary_button_with_icon_ex(ui, theme, icon, label, true)
}

/// 弹窗底栏主操作（图标 + 文字），用于 `ui.add(...)`。
pub struct ModalPrimaryButtonWithIcon<'a> {
    theme: &'a Theme,
    icon: IconId,
    label: &'a str,
    can_activate: bool,
}

impl Widget for ModalPrimaryButtonWithIcon<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        modal_primary_button_with_icon_ex(
            ui,
            self.theme,
            self.icon,
            self.label,
            self.can_activate,
        )
    }
}

impl ModalPrimaryButtonWithIcon<'_> {
    pub fn can_activate(mut self, can: bool) -> Self {
        self.can_activate = can;
        self
    }
}

pub fn modal_primary_button_with_icon_widget<'a>(
    theme: &'a Theme,
    icon: IconId,
    label: &'a str,
) -> ModalPrimaryButtonWithIcon<'a> {
    ModalPrimaryButtonWithIcon {
        theme,
        icon,
        label,
        can_activate: true,
    }
}

pub fn modal_danger_icon_button(ui: &mut Ui, theme: &Theme, icon: IconId, tooltip: &str) -> Response {
    let label_size = control_button_size(
        ui,
        theme,
        tooltip,
        false,
        theme.size_modal_footer_btn_min_w_secondary(),
    );
    let response = if ui.available_width() >= label_size.x {
        paint_modal_danger_button(ui, theme, tooltip)
    } else {
        paint_icon_only_button(
            ui,
            theme,
            icon,
            ControlButtonVariant::Danger,
            theme.size_modal_footer_btn_min_w_secondary(),
            true,
        )
    };
    response.on_hover_text(tooltip)
}

/// 面板 / dock 内行内次要按钮（与排序芯片、弹窗「取消」同族）
pub fn panel_action_icon_button(ui: &mut Ui, theme: &Theme, icon: IconId, tooltip: &str) -> Response {
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

pub fn panel_action_primary_icon_button(ui: &mut Ui, theme: &Theme, icon: IconId, tooltip: &str) -> Response {
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

/// 面板 / dock 内行内次要按钮（与排序芯片、弹窗「取消」同族）
pub fn panel_action_button(ui: &mut Ui, theme: &Theme, label: &str) -> Response {
    panel_action_button_ex(ui, theme, label, true)
}

/// 带启用态的面板次要按钮
pub fn panel_action_button_ex(
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
        ControlButtonVariant::Secondary,
        theme.size_control_btn_min_w(),
        enabled,
    )
}

/// 面板内行内主按钮（保存、克隆等）
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
        ControlButtonVariant::ToolbarPrimary,
        theme.size_control_btn_min_w(),
        enabled,
    )
}

/// 图标 + 文字的次要按钮（侧栏 SFTP / 资源面板等比纯图标更易识别的工具按钮）。
pub fn panel_action_button_with_icon_ex(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    enabled: bool,
) -> Response {
    paint_control_button(
        ui,
        theme,
        label,
        Some(icon),
        ControlButtonVariant::Secondary,
        theme.size_control_btn_min_w(),
        enabled,
    )
}

/// 图标 + 文字的主按钮（最显眼的「上传」等正向操作）。
pub fn panel_action_primary_button_with_icon_ex(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    enabled: bool,
) -> Response {
    paint_control_button(
        ui,
        theme,
        label,
        Some(icon),
        ControlButtonVariant::ToolbarPrimary,
        theme.size_control_btn_min_w(),
        enabled,
    )
}

/// 数字框（`DragValue` 等）包进与单行输入相同的底+描边
pub fn form_drag_value_field(
    ui: &mut Ui,
    theme: &Theme,
    id: egui::Id,
    add_field: impl FnOnce(&mut Ui) -> Response,
) -> Response {
    let focused = ui.memory(|m| m.has_focus(id));
    let underline = theme.uses_underline_inputs();
    let shown = theme.frame_form_text_input(focused).show(ui, |ui| {
        with_underline_field_visuals(ui, theme, add_field)
    });
    if underline {
        paint_form_field_underline(ui, theme, shown.response.rect, focused);
    }
    shown.inner
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
