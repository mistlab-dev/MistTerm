use super::*;
use crate::core::SESSION_COLOR_TAGS;

impl MistTermApp {
    pub(crate) fn render_new_session_modal(
        &mut self,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
    ) {
        // 显示新建会话对话框
        if self.show_new_session_dialog {
            let mut open = self.show_new_session_dialog;
            let mut should_close = false;
            let modal_sz = layout_util::modal_new_session_size(ctx);
            let modal_resp = crate::ui::chrome::modal_window("new_session_modal", theme, ctx)
                .open(&mut open)
                .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
                .movable(true)
                .resizable(false)
                .fixed_size(modal_sz)
                .show(ctx, |ui| {
                    let required_missing =
                        self.new_session_name.trim().is_empty() || self.new_session_host.trim().is_empty();
                    let form_w = layout_util::finite_content_width_inset(ui, 4.0, 300.0, 340.0);

                    crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                            ui.push_id("new_session_form", |ui| {
                            Self::modal_header_title_only(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "New session", "新建会话"),
                            );

                            ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
                            Self::ui_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Session name", "会话名称"),
                            );
                            Self::ui_form_singleline(
                                ui,
                                theme,
                                "new_session_name",
                                &mut self.new_session_name,
                                crate::i18n::tr(ctx, "e.g. prod-server-01", "例：生产服务器-01"),
                                form_w,
                                false,
                            );

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let row_w = layout_util::finite_content_width_inset(ui, 4.0, 300.0, 340.0);
                                let host_w = (row_w - 98.0).max(160.0);
                                ui.vertical(|ui| {
                                    ui.set_width(host_w);
                                    Self::ui_field_label(
                                        ui,
                                        theme,
                                        crate::i18n::tr(ctx, "Host", "主机地址"),
                                    );
                                    Self::ui_form_singleline(
                                        ui,
                                        theme,
                                        "new_session_host",
                                        &mut self.new_session_host,
                                        crate::i18n::tr(ctx, "IP or hostname", "IP 或域名"),
                                        host_w,
                                        false,
                                    );
                                });
                                ui.vertical(|ui| {
                                    ui.set_width(88.0);
                                    Self::ui_field_label(
                                        ui,
                                        theme,
                                        crate::i18n::tr(ctx, "Port", "端口"),
                                    );
                                    Self::ui_form_port(
                                        ui,
                                        theme,
                                        "new_session_port",
                                        &mut self.new_session_port_str,
                                        &mut self.new_session_port,
                                        88.0,
                                    );
                                });
                            });

                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let row_w = layout_util::finite_content_width_inset(ui, 4.0, 300.0, 340.0);
                                let half = ((row_w - 10.0) / 2.0).max(140.0);
                                ui.vertical(|ui| {
                                    ui.set_width(half);
                                    Self::ui_field_label(
                                        ui,
                                        theme,
                                        crate::i18n::tr(ctx, "Username", "用户名"),
                                    );
                                    Self::ui_form_singleline(
                                        ui,
                                        theme,
                                        "new_session_username",
                                        &mut self.new_session_username,
                                        crate::i18n::tr(ctx, "e.g. root", "如 root"),
                                        half,
                                        false,
                                    );
                                });
                                ui.vertical(|ui| {
                                    ui.set_width(half);
                                    Self::ui_field_label(
                                        ui,
                                        theme,
                                        crate::i18n::tr(ctx, "Password", "密码"),
                                    );
                                    Self::ui_form_singleline(
                                        ui,
                                        theme,
                                        "new_session_password",
                                        &mut self.new_session_password,
                                        crate::i18n::tr(ctx, "Optional", "可留空"),
                                        half,
                                        true,
                                    );
                                });
                            });

                            Self::ui_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "SSH private key path", "SSH 私钥路径"),
                            );
                            Self::ui_form_singleline(
                                ui,
                                theme,
                                "new_session_private_key_path",
                                &mut self.new_session_private_key_path,
                                crate::i18n::tr(
                                    ctx,
                                    "~/.ssh/id_rsa (empty = password or default keys)",
                                    "~/.ssh/id_rsa（留空则用密码或系统默认密钥）",
                                ),
                                form_w,
                                false,
                            );
                            crate::ui::chrome::form_checkbox(
                                ui,
                                theme,
                                &mut self.new_session_use_ssh_agent,
                                crate::i18n::tr(
                                    ctx,
                                    "Use SSH agent (ssh-agent / Pageant)",
                                    "使用 SSH Agent（ssh-agent / Pageant）",
                                ),
                            );

                            Self::ui_field_label(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Accent color tag", "环境色标"),
                            );
                            egui::ComboBox::from_id_source("new_session_color")
                                .selected_text(crate::i18n::session_color_tag(
                                    ctx,
                                    SESSION_COLOR_TAGS
                                        .iter()
                                        .find(|(v, _)| *v == self.new_session_color_tag.as_str())
                                        .map(|(v, _)| *v)
                                        .unwrap_or_else(|| self.new_session_color_tag.as_str()),
                                ))
                                .show_ui(ui, |ui| {
                                    crate::ui::chrome::apply_menu_popup_style(ui, theme);
                                    for (value, _) in SESSION_COLOR_TAGS {
                                        let label = crate::i18n::session_color_tag(ctx, value);
                                        ui.selectable_value(
                                            &mut self.new_session_color_tag,
                                            value.to_string(),
                                            label,
                                        );
                                    }
                                });

                            if required_missing {
                                ui.add_space(theme.spacing_sm());
                                ui.label(
                                    egui::RichText::new(crate::i18n::tr(
                                        ctx,
                                        "Enter session name and host first.",
                                        "请先填写会话名称和主机地址",
                                    ))
                                    .size(theme.font_size_panel_title())
                                    .color(theme.red_a128()),
                                );
                            }

                            ui.add_space(theme.spacing_list_item_x());
                            ui.horizontal(|ui| {
                                crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                                    let can_save = !required_missing;
                                    let save_connect = ui
                                        .add(
                                            crate::ui::chrome::modal_primary_button_with_icon_widget(
                                                th,
                                                crate::ui::icons::IconId::Rocket,
                                                crate::i18n::tr(ctx, "Save & connect", "保存并连接"),
                                            )
                                            .can_activate(can_save),
                                        )
                                        .on_hover_text(if can_save {
                                            crate::i18n::tr(
                                                ctx,
                                                "Save profile and open a terminal tab",
                                                "保存会话并打开终端连接",
                                            )
                                        } else {
                                            crate::i18n::tr(
                                                ctx,
                                                "Enter session name and host first.",
                                                "请先填写会话名称和主机地址",
                                            )
                                        });
                                    if save_connect.clicked() && can_save {
                                        self.create_and_connect_session(ui.ctx());
                                        should_close = true;
                                    }
                                    if crate::ui::chrome::modal_secondary_icon_button(
                                        ui,
                                        th,
                                        crate::ui::icons::IconId::Cross,
                                        crate::i18n::tr(ctx, "Cancel", "取消"),
                                    )
                                    .clicked() {
                                        self.reset_new_session_form();
                                        should_close = true;
                                        ui.ctx().input_mut(|i| i.pointer = egui::PointerState::default());
                                    }
                                });
                            });
                            });
                    });
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !required_missing {
                        self.create_and_connect_session(ui.ctx());
                        should_close = true;
                    }
                });
            if let Some(inner) = &modal_resp {
                crate::ui::chrome::raise_window_response(ctx, &inner.response);
            }
            if should_close {
                self.show_new_session_dialog = false;
            } else {
                self.show_new_session_dialog = open;
            }
        }
    }
}
