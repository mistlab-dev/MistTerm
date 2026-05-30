//! 对话框组件
#![allow(dead_code)]
//!
//! 提供新建会话、编辑会话等对话框

use eframe::egui;

use crate::ui::layout_util;
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

        let mut open = self.visible;
        let mut close_via_header = false;
        let mut dismiss = false;
        let modal_sz = layout_util::modal_edit_size(ctx);
        crate::ui::chrome::modal_window("legacy_new_session", theme, ctx)
            .open(&mut open)
            .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
            .resizable(true)
            .default_size(modal_sz)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    if crate::ui::chrome::modal_header(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "New session", "新建会话"),
                        crate::ui::chrome::modal_title_font_size(theme),
                    ) {
                        close_via_header = true;
                    }
                    let form_w = layout_util::finite_content_width_inset(ui, 0.0, 280.0, ui.available_width());
                    crate::ui::chrome::form_field_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Session name", "会话名称"),
                    );
                    crate::ui::chrome::form_singleline_field(
                        ui,
                        theme,
                        egui::Id::new("legacy_new_session_name"),
                        &mut self.name,
                        "",
                        form_w,
                        false,
                    );

                    ui.separator();

                    crate::ui::chrome::form_field_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Host address", "主机地址"),
                    );
                    crate::ui::chrome::form_singleline_field(
                        ui,
                        theme,
                        egui::Id::new("legacy_new_session_host"),
                        &mut self.host,
                        crate::i18n::tr(ctx, "IP or hostname", "IP 或域名"),
                        form_w,
                        false,
                    );

                    ui.horizontal(|ui| {
                        crate::ui::chrome::form_field_label(
                            ui,
                            theme,
                            crate::i18n::tr(ctx, "Port", "端口"),
                        );
                        crate::ui::chrome::form_drag_value_field(
                            ui,
                            theme,
                            egui::Id::new("legacy_new_session_port"),
                            |ui| ui.add(egui::DragValue::new(&mut self.port).speed(1.0)),
                        );
                    });

                    ui.separator();

                    crate::ui::chrome::form_field_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Username", "用户名"),
                    );
                    crate::ui::chrome::form_singleline_field(
                        ui,
                        theme,
                        egui::Id::new("legacy_new_session_user"),
                        &mut self.username,
                        crate::i18n::tr(ctx, "e.g. root", "如 root"),
                        form_w,
                        false,
                    );

                    crate::ui::chrome::form_field_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Password", "密码"),
                    );
                    crate::ui::chrome::form_singleline_field(
                        ui,
                        theme,
                        egui::Id::new("legacy_new_session_pass"),
                        &mut self.password,
                        "",
                        form_w,
                        true,
                    );

                    ui.separator();

                    crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                        if crate::ui::chrome::modal_primary_icon_button(
                            ui,
                            th,
                            crate::ui::icons::IconId::Plus,
                            crate::i18n::tr(ctx, "Create", "创建"),
                        )
                            .clicked() {
                            dismiss = true;
                        }
                        if crate::ui::chrome::modal_secondary_icon_button(
                            ui,
                            th,
                            crate::ui::icons::IconId::Cross,
                            crate::i18n::tr(ctx, "Cancel", "取消"),
                        )
                            .clicked() {
                            dismiss = true;
                        }
                    });
                });
            });
        if close_via_header || dismiss {
            open = false;
            self.reset();
        }
        self.visible = open;
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
