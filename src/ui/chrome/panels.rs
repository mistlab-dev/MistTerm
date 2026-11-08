//! 面板装饰：面板框、标题行、filter chip、列表行、分段控件（#398 自 chrome.rs 拆出）。
//! 纯移动，零行为变更。

use crate::ui::icons::{self, IconId};
use crate::ui::theme::Theme;
use eframe::egui::{
    self, Button, Color32, CursorIcon, Painter, Response, RichText, Sense, Stroke, Ui, Widget,
};
use super::*;

pub(crate) fn session_sort_icon(sort: crate::core::session_sort::SessionSortBy) -> IconId {
    use crate::core::session_sort::SessionSortBy;
    match sort {
        SessionSortBy::Name | SessionSortBy::NameDesc => IconId::SortName,
        SessionSortBy::LastConnected => IconId::SortRecent,
        SessionSortBy::CreatedAt => IconId::SortUsage,
    }
}


/// 下拉 / 右键 / ComboBox 弹出层共用的控件色(含 `widgets.open`，避免子菜单发黑底)。
///
/// 注意：此处的 `selection` 专供弹出层内 SelectableLabel / selectable_value(弱灰选中)，
/// 与全局主题里的「文本拖选」色分离——弹窗外 TextEdit 仍用 theme_manager 的拖选色。
pub fn apply_popup_widget_visuals(visuals: &mut egui::Visuals, theme: &Theme) {
    let rounding = egui::CornerRadius::same(theme.radius_list_item() as u8);
    let menu_bg = theme.color_menu_popup_fill();
    let hover = theme.list_row_hover_bg();
    let selected = theme.color_list_row_selected_fill();
    let active = theme.color_widget_active_fill();

    visuals.window_fill = menu_bg;
    visuals.menu_corner_radius = rounding;
    visuals.widgets.inactive.bg_fill = menu_bg;
    visuals.widgets.inactive.weak_bg_fill = menu_bg;
    visuals.widgets.inactive.corner_radius = rounding;
    visuals.widgets.hovered.bg_fill = hover;
    visuals.widgets.hovered.weak_bg_fill = hover;
    visuals.widgets.hovered.corner_radius = rounding;
    visuals.widgets.active.bg_fill = active;
    visuals.widgets.active.weak_bg_fill = active;
    visuals.widgets.active.corner_radius = rounding;
    visuals.widgets.inactive.fg_stroke.color = theme.text_secondary();
    visuals.widgets.hovered.fg_stroke.color = theme.text_primary();
    visuals.widgets.active.fg_stroke.color = theme.text_primary();

    let open = &mut visuals.widgets.open;
    open.weak_bg_fill = active;
    open.bg_fill = active;
    open.bg_stroke = egui::Stroke::NONE;
    open.fg_stroke.color = theme.text_primary();
    open.corner_radius = rounding;

    // 菜单选中：与侧栏同一档实色灰，勿用文本拖选的高亮白底
    visuals.selection.bg_fill = selected;
    visuals.selection.stroke = egui::Stroke::NONE;
}


pub(crate) fn apply_sidebar_menu_popup_style(ui: &mut Ui, theme: &Theme) {
    apply_popup_widget_visuals(&mut ui.style_mut().visuals, theme);
    ui.style_mut().spacing.button_padding = egui::vec2(12.0, 6.0);
    ui.style_mut().spacing.item_spacing = egui::vec2(0.0, 2.0);
    ui.style_mut().spacing.indent = 0.0;
}


/// 区域外框：左、上、右(不画底边，避免与底栏顶部分隔线叠成双行)
pub fn paint_rect_border_ltr(painter: &Painter, rect: egui::Rect, stroke: Stroke) {
    if rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }
    painter.vline(rect.min.x, rect.y_range(), stroke);
    painter.hline(rect.x_range(), rect.min.y, stroke);
    painter.vline(rect.max.x - 0.5, rect.y_range(), stroke);
}


/// 侧栏 / 右 dock 壳层描边：左、上、右 + 底部分隔线(底线用 divider，避免与状态栏叠粗线)。
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
        painter.hline(rect.x_range(), rect.max.y - 0.5, theme.divider_stroke());
    }
}


/// 区域外框：仅左右(顶线由 Tab 条底部分隔线承担，避免与 PTY 顶行叠线)
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


/// 右侧 dock / 左侧连接栏外框：统一底色与内容区内边距。
pub fn region_panel_frame(theme: &Theme) -> egui::Frame {
    theme.frame_region_panel()
}


/// 左连接栏外框(底缘贴状态栏顶线，底角不圆；描边由 [`paint_region_panel_shell_border`] 统一绘制)
pub fn sidebar_panel_frame(theme: &Theme) -> egui::Frame {
    theme
        .frame_region_panel_flush_bottom()
        .stroke(egui::Stroke::NONE)
        .inner_margin(theme.right_dock_content_margin())
}


pub(crate) fn ssh_import_chip_actions(
    ui: &mut Ui,
    theme: &Theme,
    pending_ssh_imports: usize,
) -> TitleBarChromeResult {
    let mut out = TitleBarChromeResult::default();
    let ht_dismiss = crate::i18n::tr(ui.ctx(), "Dismiss SSH import banner", "关闭导入提示");
    if close_icon_button_with_tooltip(ui, theme, ht_dismiss).clicked() {
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
                let (r, _) = ui.allocate_exact_size(
                    egui::vec2(theme.size_icon_glyph(), theme.size_icon_glyph()),
                    egui::Sense::hover(),
                );
                icons::paint_icon(
                    ui,
                    r,
                    IconId::Alert,
                    theme.amber_color(),
                    theme.size_icon_glyph(),
                );
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
                    .corner_radius(4.0),
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


/// 会话列表选中行左侧 3px 强调条(§4.4)
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


/// 面板标题行左侧：图标 + 文案(侧栏 / dock / 弹窗统一)
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


/// 标题行右侧操作区宽度(工具按钮 + 关闭 ×；RTL 顺序为 close, …tools)
/// 标题行右侧工具按钮描述(用于预留宽度)
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


/// 筛选芯片行 + 右侧排序芯片(同一行，不占额外表头行)
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

    egui::Frame::NONE
        .outer_margin(egui::Margin {
            left: 0,
            right: 0,
            top: 2,
            bottom: 4,
        })
        .show(ui, |ui| {
            let row_w = ui.available_width().max(96.0);
            // 所有主题都用胶囊分段 + 排序 chip 的布局(暗夜设计稿)。
            if true {
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
    pub status_label: Option<&'a str>,
}

/// 命令片段列表行交互结果
pub struct FragmentListRowResponse {
    pub row: Response,
    pub title: Response,
}

/// 片段列表标题行：标签列宽(随文案测量，带上限)。
pub(crate) fn fragment_list_tag_column_width(ui: &Ui, theme: &Theme, tag_label: &str, content_w: f32) -> f32 {
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
pub fn fragment_list_row(
    ui: &mut Ui,
    theme: &Theme,
    row: FragmentListRow<'_>,
) -> FragmentListRowResponse {
    let pad_x = theme.spacing_fragment_row_pad_x();
    let pad_y = theme.spacing_fragment_row_pad_y();
    let gap = theme.spacing_fragment_row_tag_gap();
    let line_gap = theme.spacing_fragment_row_line_gap();
    let title_px = theme.font_size_fragment_title();
    let tag_px = theme.font_size_fragment_tag();
    let title_line_h = title_px.max(tag_px) + theme.spacing_fragment_title_line_pad();

    let row_w = crate::ui::layout_util::side_panel_row_width(ui);
    let content_w = (row_w - 2.0 * pad_x).max(0.0);
    let tag_col_w = fragment_list_tag_column_width(ui, theme, row.tag_label, content_w);
    let title_col_w = (content_w - gap - tag_col_w).max(0.0);
    // 行高按内容算(标题+命令+统计三行)，保证悬停背景盖住统计行。
    let (cmd_h, stats_h) = ui.fonts_mut(|f| {
        (
            f.row_height(&egui::FontId::monospace(theme.font_size_fragment_cmd())),
            f.row_height(&egui::FontId::proportional(theme.font_size_fragment_stats())),
        )
    });
    let content_h = title_line_h + cmd_h + stats_h + 2.0 * line_gap + 2.0 * pad_y;
    let row_h = theme.size_fragment_list_row_min_h().max(content_h.ceil());

    let (row_rect, row_response) =
        ui.allocate_at_least(egui::vec2(row_w, row_h), egui::Sense::click());
    let bg = if row_response.hovered() {
        theme.list_row_hover_bg()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(row_rect, theme.radius_card(), bg);

    let inner = egui::Rect::from_min_max(
        egui::pos2(row_rect.left() + pad_x, row_rect.top() + pad_y),
        egui::pos2(row_rect.right() - pad_x, row_rect.bottom() - pad_y),
    );
    let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::LEFT)));
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
                        let title_resp = ui
                            .add(
                                egui::Label::new(
                                    RichText::new(row.title)
                                        .size(title_px)
                                        .color(theme.accent_color()),
                                )
                                .truncate()
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_text(row.command);
                        if let Some(status) = row.status_label {
                            let badge_color = match status {
                                "draft" => theme.accent_dim_color(),
                                "archived" => theme.text_tertiary(),
                                _ => theme.accent_color(),
                            };
                            let status_text = match status {
                                "draft" => crate::i18n::tr(ui.ctx(), "Draft", "草稿"),
                                "archived" => crate::i18n::tr(ui.ctx(), "Archived", "已归档"),
                                "published" => crate::i18n::tr(ui.ctx(), "Published", "已发布"),
                                other => other,
                            };
                            ui.label(
                                RichText::new(format!("[{}]", status_text))
                                    .size(tag_px * 0.85)
                                    .color(badge_color),
                            );
                        }
                        title_resp
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
                            .truncate(),
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
            .truncate(),
        )
        .on_hover_text(cmd_trim);

    row_ui.add(
        egui::Label::new(
            RichText::new(row.stats_line)
                .size(theme.font_size_fragment_stats())
                .color(theme.color_caption_text()),
        )
        .truncate(),
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

/// 胶囊 Segmented Control(modern)；其它主题回退为独立 filter chip。
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
    // 所有内置主题统一用胶囊分段控件(与暗夜设计稿一致)；颜色由各主题令牌决定。
    if false {
        let mut picked = None;
        let chip_h = theme.size_panel_filter_chip_h();
        let chip_gap = theme.spacing_panel_gap();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(chip_gap, 0.0);
            let n = items.len() as f32;
            let avail = row_width.unwrap_or(ui.available_width()).max(96.0);
            let item_w =
                ((avail - chip_gap * (n - 1.0)) / n).max(theme.size_panel_header_btn_min_w());
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
        egui::CornerRadius::same(theme.radius_segment_track() as u8),
        theme.color_segment_track(),
        Stroke::NONE,
        egui::StrokeKind::Middle,
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
    let thumb_rounding = egui::CornerRadius::same(theme.radius_segment_thumb() as u8);

    for (idx, (value, label)) in items.iter().enumerate() {
        let seg_w = if row_width.is_some() {
            (seg_total - seg_gap * (items.len() as f32 - 1.0)) / items.len() as f32
        } else {
            seg_widths[idx]
        };
        let seg_rect =
            egui::Rect::from_min_size(egui::pos2(x, track_rect.min.y), egui::vec2(seg_w, track_h));
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
                egui::StrokeKind::Middle,
            );
        }
        let text_color = if active {
            theme.color_segment_thumb_text()
        } else {
            theme.color_segment_idle_text()
        };
        let resp = ui.interact(seg_rect, ui.scope_id().with(("seg", idx)), Sense::click());
        paint_caption_in_rect_center(
            ui,
            seg_rect,
            label,
            theme.font_size_category_label(),
            text_color,
        );
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


/// 半透明工具栏 Button Group(modern)；返回被点击项索引。
pub fn button_group_toolbar(
    ui: &mut Ui,
    theme: &Theme,
    actions: &[ButtonGroupAction<'_>],
    expand_width: Option<f32>,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
) -> Option<usize> {
    if actions.is_empty() {
        return None;
    }
    if !theme.uses_modern_palette() {
        return None;
    }

    ui.push_id(egui::Id::unique(id_salt), |ui| {
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
            egui::CornerRadius::same(theme.radius_list_item() as u8),
            theme.color_button_group_fill(),
            Stroke::NONE,
            egui::StrokeKind::Middle,
        );

        let mut clicked_idx = None;
        let mut x = group_rect.min.x;
        for (idx, action) in actions.iter().enumerate() {
            let w = item_widths[idx];
            let item_rect =
                egui::Rect::from_min_size(egui::pos2(x, group_rect.min.y), egui::vec2(w, group_h));
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
            let resp = ui.interact(item_rect, ui.scope_id().with(("bgrp", idx)), sense);
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
                    egui::CornerRadius::same((theme.radius_list_item() - 1.0) as u8),
                    theme.color_widget_hover_fill(),
                    Stroke::NONE,
                    egui::StrokeKind::Middle,
                );
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if action.label.is_empty() {
                let icon_rect =
                    egui::Rect::from_center_size(item_rect.center(), egui::vec2(icon_px, icon_px));
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

// ── SFTP 面板专用工具条(modern：地址栏一体包 + 悬停肉垫 + 幽灵提交) ──


/// SFTP 工具条行容器：统一高度、垂直居中、深色一体底。
pub fn sftp_toolbar_band<R>(
    ui: &mut Ui,
    theme: &Theme,
    add: impl FnOnce(&mut Ui, &Theme) -> R,
) -> R {
    if !theme.uses_modern_palette() {
        return add(ui, theme);
    }
    let row_h = theme.size_sftp_toolbar_row_h();
    theme
        .frame_sftp_toolbar_band()
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width().max(1.0), row_h),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| add(ui, theme),
            )
            .inner
        })
        .inner
}


/// SFTP 工具条内单颗操作：与面板次要按钮同一套(浅底+描边)，勿再单独画一套「幽灵」样式。
pub fn sftp_toolbar_action_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    tooltip: &str,
    enabled: bool,
    _id: egui::Id,
) -> Response {
    let response = if label.is_empty() {
        panel_action_icon_button_ex(ui, theme, icon, tooltip, enabled)
    } else {
        panel_action_button_with_icon_ex(ui, theme, icon, label, enabled)
    };
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(tooltip)
    }
}


/// SFTP 工具条内一组操作(前往 / 上传等)；返回被点击项索引。
pub fn sftp_toolbar_actions(
    ui: &mut Ui,
    theme: &Theme,
    actions: &[ButtonGroupAction<'_>],
    id_salt: impl std::hash::Hash + std::fmt::Debug,
) -> Option<usize> {
    if !theme.uses_modern_palette() {
        return None;
    }
    let mut clicked = None;
    ui.push_id(egui::Id::unique(id_salt), |ui| {
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
                ui.scope_id().with(idx),
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


/// SFTP 提交类按钮(「+ 创建」等)— 与面板次要按钮同族，不再单独画幽灵样式。
pub fn sftp_ghost_submit_button(
    ui: &mut Ui,
    theme: &Theme,
    icon: IconId,
    label: &str,
    enabled: bool,
) -> Response {
    panel_action_button_with_icon_ex(ui, theme, icon, label, enabled)
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
    nav_id_salt: impl std::hash::Hash + std::fmt::Debug,
) -> Option<usize> {
    if !theme.uses_modern_palette() {
        return None;
    }
    sftp_toolbar_band(ui, theme, |ui, theme| {
        sftp_toolbar_actions(ui, theme, nav_actions, nav_id_salt)
    })
}


/// 均分宽度的筛选芯片行(常用/Docker、全部/在线/离线等)
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


/// 分类筛选芯片(全部/在线/离线、常用/Docker 等)
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
        egui::CornerRadius::ZERO
    } else {
        egui::CornerRadius::same(theme.radius_category() as u8)
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
    ui.painter().rect(rect, rounding, fill, egui::Stroke::NONE, egui::StrokeKind::Middle);
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

#[path = "../chrome_menu.rs"]
mod chrome_menu;

pub use chrome_menu::{
    apply_context_menu_style, apply_menu_popup_style, measure_popup_menu_row_width,
    menu_item_label, menu_item_label_accel, menu_item_label_accel_shift, menu_theme_check_slot,
    menu_theme_item, menu_toggle_item, popup_menu_button, popup_menu_button_accel,
    popup_menu_button_accel_shift, popup_menu_button_enabled, popup_menu_button_shortcut,
    popup_menu_button_shortcut_enabled, popup_menu_section_label, prime_menu_popup_width,
};


/// 侧栏搜索框([`panel_search_row`] 别名)
/// 左栏顶部 SSH 配置导入提示条(§4.2，约 34px，弱提示)
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
    painter.hline(rect.x_range(), rect.bottom() - 1.0, theme.divider_stroke());

    let msg = match crate::i18n::language(ui.ctx()) {
        crate::i18n::UiLanguage::En => {
            format!("Detected {} pending SSH Host block(s)", pending_count,)
        }
        crate::i18n::UiLanguage::Zh => format!("检测到 {} 个未导入的 SSH 配置", pending_count,),
    };
    let inner = rect.shrink2(egui::vec2(10.0, 0.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.set_height(BAR_H);
        ui.horizontal_centered(|ui| {
            let (ar, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
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


/// 只读信息标签(连接元信息、侧栏分组等)
pub fn label_tag_chip(
    ui: &mut Ui,
    theme: &Theme,
    text: &str,
    font_size: f32,
    text_color: Color32,
) -> Response {
    theme
        .frame_label_tag()
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(font_size).color(text_color));
        })
        .response
}


/// 面板折叠后状态栏复原按钮(图标 + 名称 · N)
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


/// 右侧 dock 标题行(标题 + 关闭 ×)。
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


/// 侧栏小标题 + 右侧关闭(与 [`dock_panel_title_close_only`] 相同布局)。
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

#[path = "../chrome_modal_actions.rs"]
mod chrome_modal_actions;

pub use chrome_modal_actions::{
    modal_danger_button, modal_danger_icon_button, modal_footer_actions, modal_primary_button,
    modal_primary_button_widget, modal_primary_button_with_icon,
    modal_primary_button_with_icon_ex, modal_primary_button_with_icon_widget,
    modal_primary_icon_button, modal_primary_icon_button_ex, modal_primary_icon_button_widget,
    modal_secondary_button, modal_secondary_icon_button, ModalPrimaryButton,
    ModalPrimaryButtonWithIcon, ModalPrimaryIconButton,
};

