//! 侧边栏
//!
//! 显示会话列表、新建/删除会话等操作

use eframe::egui;
use crate::core::session::SessionManager;

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
    ) -> egui::Response {
        let width = if ui.available_width() > 250.0 { 200.0 } else { ui.available_width() };
        
        ui.allocate_ui_with_layout(
            egui::vec2(width, ui.available_height()),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                // 标题栏
                ui.horizontal(|ui| {
                    ui.heading("会话");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("➕").clicked() {
                            // TODO: 打开新建会话对话框
                        }
                    });
                });
                ui.separator();

                // 会话列表
                ui.vertical(|ui| {
                    let sessions = session_manager.list_sessions();
                    
                    if sessions.is_empty() {
                        ui.centered_and_justified(|ui| {
                            ui.small("暂无会话");
                            ui.small("点击 ➕ 创建");
                        });
                    } else {
                        for session in sessions {
                            let is_selected = selected_id.as_ref() == Some(&session.id);
                            
                            let response = ui.selectable_label(is_selected, &session.name);
                            
                            if response.clicked() {
                                // 会话选择事件通过回调通知父组件
                                // 这里只负责 UI 渲染
                            }
                            
                            // 右键菜单
                            response.context_menu(|ui| {
                                if ui.button("编辑").clicked() {
                                    // TODO: 打开编辑对话框
                                    ui.close_menu();
                                }
                                if ui.button("删除").clicked() {
                                    // TODO: 确认删除
                                    ui.close_menu();
                                }
                            });
                        }
                    }
                });
            }
        ).response
    }
}
