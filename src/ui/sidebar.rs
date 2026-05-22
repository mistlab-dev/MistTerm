//! 侧边栏
//!
//! 显示会话列表、新建/删除会话等操作

use eframe::egui;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::core::session::{session_color_tag_rgb, SessionManager};
use crate::core::session_sort::{sort_sessions, SessionSortBy};
use crate::ui::layout_util;
use crate::ui::theme::Theme;

pub struct SidebarOutput {
    pub response: egui::Response,
    pub selected_session_id: Option<String>,
    pub delete_session_id: Option<String>,
    pub edit_session_id: Option<String>,
    pub create_session_clicked: bool,
    pub collapse_clicked: bool,
    pub view_log_session_id: Option<String>,
}

/// 左栏整列（导入条 + 圆角面板）的附加动作
pub struct SidebarColumnActions {
    pub open_ssh_import: bool,
    pub dismiss_ssh_banner: bool,
}

/// 侧边栏组件
pub struct Sidebar;

impl Sidebar {
    /// 左栏整列：SSH 导入条（可选）→ 圆角连接面板（[`Sidebar::show`]）
    pub fn show_column<'a>(
        ui: &mut egui::Ui,
        layout_h: f32,
        sidebar_width: f32,
        ssh_import_banner_dismissed: bool,
        ssh_pending_imports: usize,
        session_manager: &'a SessionManager,
        selected_id: &Option<String>,
        search_query: &mut String,
        filter: &mut String,
        sort_by: &mut SessionSortBy,
        connected_sessions: &HashSet<String>,
        search_field_id: egui::Id,
        theme: &Theme,
    ) -> (SidebarOutput, SidebarColumnActions) {
        let mut actions = SidebarColumnActions {
            open_ssh_import: false,
            dismiss_ssh_banner: false,
        };
        ui.set_width(sidebar_width);
        ui.set_min_height(layout_h);
        if !ssh_import_banner_dismissed && ssh_pending_imports > 0 {
            if let Some(act) =
                crate::ui::chrome::ssh_import_sidebar_banner(ui, theme, ssh_pending_imports)
            {
                if act.import {
                    actions.open_ssh_import = true;
                }
                if act.dismiss {
                    actions.dismiss_ssh_banner = true;
                }
            }
        }
        let panel_h = ui.available_height().max(120.0);
        let output = crate::ui::chrome::sidebar_panel_frame(theme)
            .show(ui, |ui| {
                ui.set_width(sidebar_width);
                ui.set_min_height(panel_h);
                ui.set_height(panel_h);
                Self::show(
                    ui,
                    panel_h,
                    session_manager,
                    selected_id,
                    search_query,
                    filter,
                    sort_by,
                    connected_sessions,
                    search_field_id,
                    theme,
                )
            })
            .inner;
        (output, actions)
    }

    /// 显示侧边栏
    /// 
    /// 返回双击事件响应
    pub fn show<'a>(
        ui: &mut egui::Ui,
        panel_h: f32,
        session_manager: &'a SessionManager,
        selected_id: &Option<String>,
        search_query: &mut String,
        filter: &mut String,
        sort_by: &mut SessionSortBy,
        connected_sessions: &HashSet<String>,
        search_field_id: egui::Id,
        theme: &Theme,
    ) -> SidebarOutput {
        // 占满宿主分配的侧栏列宽（勿再 cap 200px，否则列宽 > 内容宽会出现一条空白缝）
        let width = layout_util::finite_avail_minus(
            ui,
            0.0,
            160.0,
            ui.max_rect().width().max(160.0),
        );
        let mut selected_session_id = None;
        let mut delete_session_id = None;
        let mut edit_session_id = None;
        let mut create_session_clicked = false;
        let mut collapse_clicked = false;
        let mut view_log_session_id = None;
        
        let body_h = panel_h.max(120.0);
        let response = ui.allocate_ui_with_layout(
            egui::vec2(width, body_h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_height(body_h);
                ui.set_height(body_h);
                theme.frame_panel_header_band().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = theme.spacing_status_left_gap();
                            crate::ui::chrome::panel_header_title_leading(
                                ui,
                                theme,
                                crate::ui::icons::IconId::Plug,
                                "连接",
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = theme.spacing_tool_btn_gap();
                                if crate::ui::chrome::sidebar_header_icon_button(
                                    ui,
                                    theme,
                                    crate::ui::icons::IconId::SidebarCollapse,
                                    theme.color_sidebar_header_icon(),
                                )
                                .on_hover_text("收起连接栏")
                                .clicked()
                                {
                                    collapse_clicked = true;
                                }
                                ui.add_space(theme.spacing_panel_gap());
                                if crate::ui::chrome::panel_header_new_button(ui, theme)
                                    .on_hover_text(format!(
                                        "新建会话 · {}",
                                        crate::platform::accel("N")
                                    ))
                                    .clicked()
                                {
                                    create_session_clicked = true;
                                }
                            });
                        });
                    });
                crate::ui::chrome::panel_header_divider(ui, theme);
                // 与命令片段面板一致：分隔线后留出一段呼吸空间，再进入搜索区
                ui.add_space(theme.spacing_sm());

                crate::ui::chrome::form_singleline_field(
                    ui,
                    theme,
                    search_field_id,
                    search_query,
                    "搜索会话…",
                    width,
                    false,
                );
                ui.add_space(2.0);
                let sort_icon = match *sort_by {
                    SessionSortBy::Name | SessionSortBy::NameDesc => {
                        crate::ui::icons::IconId::SortName
                    }
                    SessionSortBy::LastConnected => crate::ui::icons::IconId::SortRecent,
                    SessionSortBy::CreatedAt => crate::ui::icons::IconId::SortUsage,
                };
                let row = crate::ui::chrome::filter_chip_row_with_sort(
                    ui,
                    theme,
                    &["全部", "在线", "离线"],
                    filter.as_str(),
                    sort_icon,
                    "",
                );
                if let Some(picked) = row.picked {
                    *filter = picked;
                }
                if row.cycle_sort {
                    *sort_by = match *sort_by {
                        SessionSortBy::Name => SessionSortBy::NameDesc,
                        SessionSortBy::NameDesc => SessionSortBy::LastConnected,
                        SessionSortBy::LastConnected => SessionSortBy::CreatedAt,
                        SessionSortBy::CreatedAt => SessionSortBy::Name,
                    };
                }

                // 会话列表（占满标题/搜索/筛选下方的剩余高度）
                let list_h = ui.available_height().max(80.0);
                egui::ScrollArea::vertical()
                    .id_source("mistterm_sidebar_sessions")
                    .max_height(list_h)
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                    ui.set_min_width(width);
                    let mut sessions = session_manager
                        .list_sessions()
                        .iter()
                        .filter(|s| {
                            if search_query.trim().is_empty() {
                                return true;
                            }
                            let query = search_query.to_lowercase();
                            s.name.to_lowercase().contains(&query)
                                || s.host.to_lowercase().contains(&query)
                                || s.group.to_lowercase().contains(&query)
                        })
                        .cloned()
                        .filter(|s| match filter.as_str() {
                            "在线" => connected_sessions.contains(&s.id),
                            "离线" => !connected_sessions.contains(&s.id),
                            _ => true,
                        })
                        .collect::<Vec<_>>();

                    sort_sessions(&mut sessions, *sort_by);
                    
                    if sessions.is_empty() {
                        ui.centered_and_justified(|ui| {
                            let hint_font = theme.font_size_sidebar_control();
                            let hint_color = theme.text_tertiary();
                            if search_query.trim().is_empty() {
                                ui.label(
                                    egui::RichText::new("暂无会话")
                                        .size(hint_font)
                                        .color(hint_color),
                                );
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    let px = hint_font;
                                    let (r, _) = ui.allocate_exact_size(
                                        egui::vec2(px, px),
                                        egui::Sense::hover(),
                                    );
                                    crate::ui::icons::paint_icon(
                                        ui,
                                        r,
                                        crate::ui::icons::IconId::Plus,
                                        hint_color,
                                        px,
                                    );
                                    ui.label(
                                        egui::RichText::new("点击 创建")
                                            .size(hint_font)
                                            .color(hint_color),
                                    );
                                });
                            } else {
                                ui.label(
                                    egui::RichText::new("没有匹配的会话")
                                        .size(hint_font)
                                        .color(hint_color),
                                );
                            }
                        });
                    } else {
                        let mut current_group = String::new();
                        for session in &sessions {
                            if session.group != current_group {
                                current_group = session.group.clone();
                                ui.add_space(theme.spacing_panel_gap());
                                crate::ui::chrome::label_tag_chip(
                                    ui,
                                    theme,
                                    &format!("[{}]", current_group),
                                    theme.font_size_connection_meta(),
                                    theme.color_section_title(),
                                );
                            }
                            let is_selected = selected_id.as_ref() == Some(&session.id);
                            let row_inner_w = {
                                let a = ui.available_width();
                                if a.is_finite() && a < 10_000.0 {
                                    a
                                } else {
                                    width
                                }
                            };
                            let (row_rect, response) = ui.allocate_exact_size(
                                egui::vec2(row_inner_w, theme.size_session_list_row_h()),
                                egui::Sense::click(),
                            );
                            let bg = if is_selected {
                                theme.list_row_selected_bg()
                            } else if response.hovered() {
                                theme.list_row_hover_bg()
                            } else {
                                egui::Color32::TRANSPARENT
                            };
                            ui.painter().rect_filled(
                                row_rect,
                                theme.radius_list_item(),
                                bg,
                            );
                            if is_selected {
                                crate::ui::chrome::paint_sidebar_selection_accent(
                                    ui.painter(),
                                    row_rect,
                                    theme,
                                );
                            }

                            let status_text = if connected_sessions.contains(&session.id) {
                                relative_last_connected(session.last_connected_at)
                            } else {
                                "离线".to_string()
                            };
                            let mut row_ui = ui.child_ui(
                                row_rect.shrink2(egui::vec2(
                                    theme.spacing_list_item_x(),
                                    theme.spacing_list_item_y(),
                                )),
                                egui::Layout::left_to_right(egui::Align::Center),
                            );
                            let _online = connected_sessions.contains(&session.id);
                            let env_color = session_color_tag_rgb(&session.color_tag)
                                .map(|(r, g, b)| egui::Color32::from_rgb(r, g, b));
                            let dot_r = 3.0_f32;
                            let (dot_rect, _) = row_ui.allocate_exact_size(
                                egui::vec2(dot_r * 2.0, dot_r * 2.0),
                                egui::Sense::hover(),
                            );
                            let center = dot_rect.center();
                            if let Some(rgb) = env_color {
                                row_ui.painter().circle_filled(center, dot_r, rgb);
                            } else {
                                row_ui.painter().circle_stroke(
                                    center,
                                    dot_r,
                                    egui::Stroke::new(1.0, theme.border_divider_color()),
                                );
                            }
                            row_ui.add_space(theme.spacing_tab_dot_text());
                            row_ui.label(
                                egui::RichText::new(&session.name)
                                    .size(theme.font_size_connection_name())
                                    .color(if is_selected {
                                        theme.text_primary()
                                    } else {
                                        theme.text_secondary()
                                    }),
                            );
                            row_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(status_text)
                                        .size(theme.font_size_connection_meta())
                                        .color(if connected_sessions.contains(&session.id) {
                                            theme.color_status_online_muted()
                                        } else {
                                            theme.color_status_offline_muted()
                                        }),
                                );
                            });
                            
                            if response.clicked() {
                                selected_session_id = Some(session.id.clone());
                            }
                            
                            // 右键菜单
                            response.context_menu(|ui| {
                                crate::ui::chrome::apply_context_menu_style(ui, theme);
                                if crate::ui::chrome::popup_menu_button(ui, theme, "编辑").clicked() {
                                    edit_session_id = Some(session.id.clone());
                                    ui.close_menu();
                                }
                                if crate::ui::chrome::popup_menu_button(ui, theme, "删除").clicked() {
                                    delete_session_id = Some(session.id.clone());
                                    ui.close_menu();
                                }
                                if crate::ui::chrome::popup_menu_button(ui, theme, "查看日志…")
                                    .clicked()
                                {
                                    view_log_session_id = Some(session.id.clone());
                                    ui.close_menu();
                                }
                            });
                            ui.add_space(theme.spacing_list_item_gap());
                        }
                    }
                });
            },
        )
        .response;

        SidebarOutput {
            response,
            selected_session_id,
            delete_session_id,
            edit_session_id,
            create_session_clicked,
            collapse_clicked,
            view_log_session_id,
        }
    }
}

fn relative_last_connected(ts: Option<i64>) -> String {
    let Some(last) = ts else {
        return "刚刚".to_string();
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(last);
    let diff = now.saturating_sub(last).max(0);
    if diff < 60 {
        "刚刚".to_string()
    } else if diff < 3600 {
        format!("{}m", diff / 60)
    } else if diff < 86_400 {
        format!("{}h", diff / 3600)
    } else {
        format!("{}d", diff / 86_400)
    }
}
