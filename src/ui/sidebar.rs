//! 侧边栏
//!
//! 显示会话列表、新建/删除会话等操作

use eframe::egui;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};
use crate::core::session::SessionManager;
use crate::ui::theme::Theme;

pub struct SidebarOutput {
    pub response: egui::Response,
    pub selected_session_id: Option<String>,
    pub delete_session_id: Option<String>,
    pub edit_session_id: Option<String>,
    pub create_session_clicked: bool,
    pub collapse_clicked: bool,
}

/// 侧边栏组件
pub struct Sidebar;

impl Sidebar {
    /// 显示侧边栏
    /// 
    /// 返回双击事件响应
    pub fn show<'a>(
        ui: &mut egui::Ui,
        session_manager: &'a SessionManager,
        selected_id: &Option<String>,
        search_query: &str,
        filter: &str,
        connected_sessions: &HashSet<String>,
        theme: &Theme,
    ) -> SidebarOutput {
        let width = if ui.available_width() > 250.0 { 200.0 } else { ui.available_width() };
        let mut selected_session_id = None;
        let mut delete_session_id = None;
        let mut edit_session_id = None;
        let mut create_session_clicked = false;
        let mut collapse_clicked = false;
        
        let response = ui.allocate_ui_with_layout(
            egui::vec2(width, ui.available_height()),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                // 标题栏
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("连接")
                            .size(10.0)
                            .strong()
                            .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 51)),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("−")
                                        .size(14.0)
                                        .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 51)),
                                )
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE)
                                .frame(false),
                            )
                            .clicked()
                        {
                            collapse_clicked = true;
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("＋")
                                        .size(12.0)
                                        .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, 72)),
                                )
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE)
                                .frame(false),
                            )
                            .clicked()
                        {
                            create_session_clicked = true;
                        }
                    });
                });
                ui.separator();
                ui.small(egui::RichText::new("最近连接").color(theme.fg_low_color()).size(11.0));

                // 会话列表
                ui.vertical(|ui| {
                    let mut sessions = session_manager
                        .list_sessions()
                        .iter()
                        .filter(|s| {
                            if search_query.trim().is_empty() {
                                return true;
                            }
                            let query = search_query.to_lowercase();
                            s.name.to_lowercase().contains(&query) || s.host.to_lowercase().contains(&query)
                        })
                        .cloned()
                        .filter(|s| match filter {
                            "在线" => connected_sessions.contains(&s.id),
                            "离线" => !connected_sessions.contains(&s.id),
                            _ => true,
                        })
                        .collect::<Vec<_>>();

                    sessions.sort_by(|a, b| {
                        let a_online = connected_sessions.contains(&a.id);
                        let b_online = connected_sessions.contains(&b.id);
                        b_online
                            .cmp(&a_online)
                            .then_with(|| b.last_connected_at.cmp(&a.last_connected_at))
                            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                    });
                    
                    if sessions.is_empty() {
                        ui.centered_and_justified(|ui| {
                            if search_query.trim().is_empty() {
                                ui.small("暂无会话");
                                ui.small("点击 ➕ 创建");
                            } else {
                                ui.small("没有匹配的会话");
                            }
                        });
                    } else {
                        for session in &sessions {
                            if session.group != current_group {
                                current_group = session.group.clone();
                                ui.add_space(6.0);
                                ui.small(
                                    egui::RichText::new(format!("📁 {}", current_group))
                                        .size(10.0)
                                        .color(theme.fg_low_color()),
                                );
                            }
                            let is_selected = selected_id.as_ref() == Some(&session.id);
                            let (row_rect, response) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 36.0),
                                egui::Sense::click(),
                            );
                            let bg = if is_selected {
                                theme.list_row_selected_bg()
                            } else if response.hovered() {
                                theme.list_row_hover_bg()
                            } else {
                                egui::Color32::TRANSPARENT
                            };
                            ui.painter().rect_filled(row_rect.shrink2(egui::vec2(0.0, 2.0)), 4.0, bg);

                            let status_text = if connected_sessions.contains(&session.id) {
                                relative_last_connected(session.last_connected_at)
                            } else {
                                "离线".to_string()
                            };
                            let mut row_ui = ui.child_ui(
                                row_rect.shrink2(egui::vec2(10.0, 8.0)),
                                egui::Layout::left_to_right(egui::Align::Center),
                            );
                            let color = if connected_sessions.contains(&session.id) {
                                theme.green_color()
                            } else {
                                theme.red_color()
                            };
                            row_ui.colored_label(color, "●");
                            row_ui.add_space(6.0);
                            row_ui.label(
                                egui::RichText::new(&session.name)
                                    .size(12.0)
                                    .color(if is_selected {
                                        theme.fg_high_color()
                                    } else {
                                        theme.fg_medium_color()
                                    }),
                            );
                            row_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(
                                    egui::RichText::new(status_text)
                                        .size(10.0)
                                        .color(if connected_sessions.contains(&session.id) {
                                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 77)
                                        } else {
                                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 64)
                                        }),
                                );
                            });
                            
                            if response.clicked() {
                                selected_session_id = Some(session.id.clone());
                            }
                            
                            // 右键菜单
                            response.context_menu(|ui| {
                                if ui.button("编辑").clicked() {
                                    edit_session_id = Some(session.id.clone());
                                    ui.close_menu();
                                }
                                if ui.button("删除").clicked() {
                                    delete_session_id = Some(session.id.clone());
                                    ui.close_menu();
                                }
                            });
                            ui.add_space(1.0);
                        }
                    }
                });

                ui.add_space(10.0);
                ui.separator();
                ui.small(egui::RichText::new("命令片段").color(theme.fg_low_color()).size(11.0));
                for (name, cmd) in [
                    ("重启 Nginx", "systemctl restart nginx"),
                    ("查看日志", "tail -f /var/log/nginx/error.log"),
                ] {
                    ui.label(egui::RichText::new(name).size(13.0));
                    let compact_cmd = if cmd.len() > 26 {
                        format!("{}...", &cmd[..26])
                    } else {
                        cmd.to_string()
                    };
                    ui.small(
                        egui::RichText::new(compact_cmd)
                            .color(theme.fg_low_color())
                            .size(11.0),
                    );
                    ui.add_space(6.0);
                }
            }
        ).response;

        SidebarOutput {
            response,
            selected_session_id,
            delete_session_id,
            edit_session_id,
            create_session_clicked,
            collapse_clicked,
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
