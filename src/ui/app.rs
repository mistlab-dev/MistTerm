//! 主应用程序
//!
//! 包含主窗口、侧边栏、终端区域等

use eframe::egui;
use crate::core::{SessionManager, SessionConfig};
use crate::ui::sidebar::Sidebar;
use crate::ui::terminal::TerminalView;

/// 主应用程序
pub struct MistTermApp {
    /// 会话管理器
    session_manager: SessionManager,
    
    /// 当前选中的会话 ID
    selected_session_id: Option<String>,
    
    /// 侧边栏状态
    sidebar_collapsed: bool,
    
    /// 终端视图（延迟初始化）
    terminal: Option<TerminalView>,
    
    /// 状态栏信息
    status_message: String,
}

impl MistTermApp {
    /// 创建新的应用实例
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let session_manager = SessionManager::new();
        let sessions = session_manager.list_sessions();
        
        // 自动选择第一个会话
        let selected_session_id = sessions.first().map(|s| s.id.clone());

        Self {
            session_manager,
            selected_session_id,
            sidebar_collapsed: false,
            terminal: None,
            status_message: "就绪".to_string(),
        }
    }

    /// 获取当前选中的会话
    pub fn selected_session(&self) -> Option<&crate::core::session::SessionConfig> {
        self.selected_session_id
            .as_ref()
            .and_then(|id| self.session_manager.get_session(id))
    }

    /// 选择会话
    pub fn select_session(&mut self, session_id: &str) {
        self.selected_session_id = Some(session_id.to_string());
        self.status_message = format!("已选择会话：{}", session_id);
        
        // 初始化终端
        self.terminal = Some(TerminalView::new(session_id.to_string()));
    }

    /// 创建新会话
    pub fn create_session(&mut self, name: &str, host: &str, username: &str) {
        let session = self.session_manager.create_session(name, host, username);
        self.select_session(&session.id);
        self.status_message = format!("已创建会话：{}", name);
    }

    /// 删除会话
    pub fn delete_session(&mut self, session_id: &str) {
        self.session_manager.delete_session(session_id);
        if self.selected_session_id.as_ref() == Some(&session_id.to_string()) {
            self.selected_session_id = None;
            self.terminal = None;
        }
        self.status_message = format!("已删除会话：{}", session_id);
    }
}

impl eframe::App for MistTermApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 顶部菜单栏
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // 文件菜单
                ui.menu_button("文件", |ui| {
                    if ui.button("新建会话 ⌘N").clicked() {
                        ui.close_menu();
                    }
                    if ui.button("删除会话").clicked() {
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("退出").clicked() {
                        ui.close_menu();
                    }
                });

                // 视图菜单
                ui.menu_button("视图", |ui| {
                    if ui.button(self.sidebar_collapsed.then(|| "展开侧边栏").unwrap_or("折叠侧边栏")).clicked() {
                        self.sidebar_collapsed = !self.sidebar_collapsed;
                        ui.close_menu();
                    }
                });

                // 帮助菜单
                ui.menu_button("帮助", |ui| {
                    if ui.button("关于").clicked() {
                        ui.close_menu();
                    }
                });
            });
        });

        // 主内容区：侧边栏 + 终端
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                // 侧边栏
                if !self.sidebar_collapsed {
                    let sidebar_response = Sidebar::show(ui, &self.session_manager, &self.selected_session_id);
                    
                    if sidebar_response.double_clicked() {
                        self.sidebar_collapsed = true;
                    }
                } else {
                    // 折叠状态：显示展开按钮
                    if ui.button("☰").clicked() {
                        self.sidebar_collapsed = false;
                    }
                }

                // 分隔条
                ui.add_space(4.0);

                // 终端区域
                ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                    if let Some(terminal) = &mut self.terminal {
                        terminal.show(ui);
                    } else {
                        self.show_welcome(ui);
                    }
                });
            });
        });

        // 底部状态栏
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("📡 {}", self.status_message));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label("MistTerm v0.1.0");
                });
            });
        });
    }
}

impl MistTermApp {
    /// 显示菜单栏
    fn show_menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::TopBottomPanel::top("menu_bar").show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                // 文件菜单
                ui.menu_button("文件", |ui| {
                    if ui.button("新建会话 ⌘N").clicked() {
                        // TODO: 打开新建会话对话框
                        ui.close_menu();
                    }
                    if ui.button("删除会话").clicked() {
                        // TODO: 删除当前会话
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("退出").clicked() {
                        ui.close_menu();
                    }
                });

                // 视图菜单
                ui.menu_button("视图", |ui| {
                    if ui.button(self.sidebar_collapsed.then(|| "展开侧边栏").unwrap_or("折叠侧边栏")).clicked() {
                        self.sidebar_collapsed = !self.sidebar_collapsed;
                        ui.close_menu();
                    }
                });

                // 帮助菜单
                ui.menu_button("帮助", |ui| {
                    if ui.button("关于").clicked() {
                        ui.close_menu();
                    }
                });
            });
        });
    }

    /// 显示欢迎界面
    fn show_welcome(&self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
            ui.heading("欢迎使用 MistTerm");
            ui.separator();
            ui.colored_label(
                ui.style().visuals.selection.bg_fill,
                "🚀 快速开始"
            );
            ui.horizontal(|ui| {
                ui.label("1. 点击左侧");
                ui.label("➕ 创建新会话");
            });
            ui.horizontal(|ui| {
                ui.label("2. 选择会话");
                ui.label("🔌 建立连接");
            });
            ui.horizontal(|ui| {
                ui.label("3. 使用");
                ui.label("rz/sz");
                ui.label("进行文件传输");
            });
            ui.separator();
            ui.small("提示：双击侧边栏可以折叠/展开");
        });
    }
}
