//! 对话框组件
#![allow(dead_code)]
//!
//! 提供新建会话、编辑会话等对话框

use eframe::egui;

/// 新建会话对话框
pub struct NewSessionDialog {
    /// 会话名称
    name: String,
    
    /// 主机地址
    host: String,
    
    /// 端口
    port: u16,
    
    /// 用户名
    username: String,
    
    /// 密码
    password: String,
    
    /// 是否显示
    visible: bool,
}

impl NewSessionDialog {
    /// 创建新的对话框
    pub fn new() -> Self {
        Self {
            name: String::new(),
            host: String::new(),
            port: 22,
            username: String::new(),
            password: String::new(),
            visible: false,
        }
    }

    /// 显示对话框
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.visible {
            return;
        }

        egui::Window::new("新建会话")
            .resizable(true)
            .collapsible(false)
            .default_width(400.0)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.label("会话名称");
                    ui.text_edit_singleline(&mut self.name);
                    
                    ui.separator();
                    
                    ui.label("主机地址");
                    ui.text_edit_singleline(&mut self.host);
                    
                    ui.horizontal(|ui| {
                        ui.label("端口");
                        ui.add(egui::DragValue::new(&mut self.port));
                    });
                    
                    ui.separator();
                    
                    ui.label("用户名");
                    ui.text_edit_singleline(&mut self.username);
                    
                    ui.label("密码");
                    ui.add(egui::TextEdit::singleline(&mut self.password).password(true));
                    
                    ui.separator();
                    
                    ui.horizontal(|ui| {
                        if ui.button("取消").clicked() {
                            self.visible = false;
                            self.reset();
                        }
                        
                        if ui.button("创建").clicked() {
                            // TODO: 调用回调创建会话
                            self.visible = false;
                            self.reset();
                        }
                    });
                });
            });
    }

    /// 打开对话框
    pub fn open(&mut self) {
        self.visible = true;
    }

    /// 重置表单
    fn reset(&mut self) {
        self.name.clear();
        self.host.clear();
        self.port = 22;
        self.username.clear();
        self.password.clear();
    }
}

impl Default for NewSessionDialog {
    fn default() -> Self {
        Self::new()
    }
}
