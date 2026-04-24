//! 主应用程序
//!
//! 包含主窗口、侧边栏、终端区域等

use eframe::egui;
use crate::core::{SessionConfig, SessionManager};
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
    
    /// 是否显示新建会话对话框
    show_new_session_dialog: bool,
    
    /// 新建会话表单
    new_session_name: String,
    new_session_host: String,
    new_session_port: u16,
    new_session_username: String,
    new_session_password: String,
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
            show_new_session_dialog: false,
            new_session_name: String::new(),
            new_session_host: String::new(),
            new_session_port: 22,
            new_session_username: String::new(),
            new_session_password: String::new(),
        }
    }

    /// 获取当前选中的会话
    pub fn selected_session(&self) -> Option<&SessionConfig> {
        self.selected_session_id
            .as_ref()
            .and_then(|id| self.session_manager.get_session(id))
    }

    /// 选择会话
    pub fn select_session(&mut self, session_id: &str) {
        self.selected_session_id = Some(session_id.to_string());
        self.status_message = format!("已选择会话：{}", session_id);
        
        // 初始化终端
        self.terminal = Some(TerminalView::new());
    }

    /// 创建并连接会话
    fn create_and_connect_session(&mut self) {
        if self.new_session_name.is_empty() || self.new_session_host.is_empty() {
            self.status_message = "请填写会话名称和主机地址".to_string();
            return;
        }

        // 创建会话
        let session = self.session_manager.create_session(
            &self.new_session_name,
            &self.new_session_host,
            &self.new_session_username,
        );

        // 选择会话
        self.selected_session_id = Some(session.id.clone());
        
        // 创建终端并连接
        let mut terminal = TerminalView::new();
        terminal.connect(
            &self.new_session_host,
            self.new_session_port,
            &self.new_session_username,
            &self.new_session_password,
        );
        self.terminal = Some(terminal);
        
        self.status_message = format!("正在连接：{}", self.new_session_name);
        self.reset_new_session_form();
    }

    /// 重置新建会话表单
    fn reset_new_session_form(&mut self) {
        self.new_session_name.clear();
        self.new_session_host.clear();
        self.new_session_port = 22;
        self.new_session_username.clear();
        self.new_session_password.clear();
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
                        self.show_new_session_dialog = true;
                        ui.close_menu();
                    }
                    if ui.button("删除会话").clicked() {
                        if let Some(id) = self.selected_session_id.clone() {
                            self.delete_session(&id);
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("退出").clicked() {
                        // 关闭窗口
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

        // 显示新建会话对话框
        if self.show_new_session_dialog {
            egui::Window::new("新建会话")
                .resizable(true)
                .collapsible(false)
                .default_width(400.0)
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.label("会话名称");
                        ui.text_edit_singleline(&mut self.new_session_name);
                        
                        ui.separator();
                        
                        ui.label("主机地址");
                        ui.text_edit_singleline(&mut self.new_session_host);
                        
                        ui.horizontal(|ui| {
                            ui.label("端口");
                            ui.add(egui::DragValue::new(&mut self.new_session_port));
                        });
                        
                        ui.separator();
                        
                        ui.label("用户名");
                        ui.text_edit_singleline(&mut self.new_session_username);
                        
                        ui.label("密码");
                        ui.add(egui::TextEdit::singleline(&mut self.new_session_password).password(true));
                        
                        ui.separator();
                        
                        ui.horizontal(|ui| {
                            if ui.button("取消").clicked() {
                                self.show_new_session_dialog = false;
                                self.reset_new_session_form();
                            }
                            
                            if ui.button("创建并连接").clicked() {
                                self.create_and_connect_session();
                                self.show_new_session_dialog = false;
                            }
                        });
                    });
                });
        }
    }
}

impl MistTermApp {
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
