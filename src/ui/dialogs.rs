//! 对话框组件
#![allow(dead_code)]
//!
//! 提供新建会话、编辑会话等对话框

use eframe::egui;

use crate::ui::theme::Theme;

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

    /// 显示对话框（文字与输入框颜色随 `Theme`）
    pub fn show(&mut self, ctx: &egui::Context, theme: &Theme) {
        if !self.visible {
            return;
        }

        egui::Window::new("新建会话")
            .resizable(true)
            .collapsible(false)
            .default_width(400.0)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new("会话名称").color(theme.fg_medium_color()),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.name)
                            .desired_width(f32::INFINITY)
                            .text_color(theme.fg_high_color()),
                    );

                    ui.separator();

                    ui.label(
                        egui::RichText::new("主机地址").color(theme.fg_medium_color()),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.host)
                            .desired_width(f32::INFINITY)
                            .text_color(theme.fg_high_color()),
                    );

                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("端口").color(theme.fg_medium_color()));
                        ui.add(egui::DragValue::new(&mut self.port).text_color(theme.fg_high_color()));
                    });

                    ui.separator();

                    ui.label(
                        egui::RichText::new("用户名").color(theme.fg_medium_color()),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.username)
                            .desired_width(f32::INFINITY)
                            .text_color(theme.fg_high_color()),
                    );

                    ui.label(egui::RichText::new("密码").color(theme.fg_medium_color()));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.password)
                            .password(true)
                            .desired_width(f32::INFINITY)
                            .text_color(theme.fg_high_color()),
                    );

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
