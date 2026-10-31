use super::*;

impl MistTermApp {
    pub(crate) fn show_application_menu_bar(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        frame: &mut eframe::Frame,
    ) {
        if self.uses_native_menu_bar() {
            return;
        }
        let label = |text: &str| {
            egui::RichText::new(text)
                .size(theme.font_size_menu_item())
                .color(theme.text_secondary())
        };
        let ssh_import_enabled = self.ssh_config_path.exists();
        let l = crate::i18n::menu::labels(crate::i18n::language(ctx));

        ui.menu_button(label(l.terminal_menu), |ui| {
            crate::ui::chrome::apply_menu_popup_style(ui, theme);
            if crate::ui::chrome::popup_menu_button(ui, theme, l.new_session).clicked() {
                self.show_new_session_dialog = true;
            }
            let new_tab_accel = crate::platform::new_tab_accel();
            if crate::ui::chrome::popup_menu_button_shortcut(
                ui,
                theme,
                l.new_tab,
                Some(&new_tab_accel),
            )
            .clicked()
            {
                self.open_new_tab_from_selection(ctx);
            }
            let local_shell_accel = crate::platform::local_shell_accel();
            if crate::ui::chrome::popup_menu_button_shortcut(
                ui,
                theme,
                l.local_shell,
                Some(&local_shell_accel),
            )
            .clicked()
            {
                self.open_local_shell_tab(ctx);
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button_accel_shift(
                ui,
                theme,
                l.split_horizontal,
                "d",
            )
            .clicked()
            {
                if let Some(idx) = self.active_tab {
                    self.split_tab_at(ctx, idx, crate::ui::tab_pane::TabLayout::SplitHorizontal);
                }
            }
            if crate::ui::chrome::popup_menu_button_accel_shift(ui, theme, l.split_vertical, "u")
                .clicked()
            {
                if let Some(idx) = self.active_tab {
                    self.split_tab_at(ctx, idx, crate::ui::tab_pane::TabLayout::SplitVertical);
                }
            }
            let broadcast_on = self
                .active_tab
                .and_then(|idx| self.tabs.get(idx))
                .is_some_and(|t| t.broadcast_input);
            let broadcast_label = if broadcast_on {
                format!("✓ {}", l.broadcast_input)
            } else {
                l.broadcast_input.to_string()
            };
            if crate::ui::chrome::popup_menu_button_accel_shift(ui, theme, &broadcast_label, "b")
                .clicked()
            {
                self.toggle_broadcast_input(ctx);
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.merge_split).clicked() {
                self.merge_active_split(ctx);
            }
            // session-import 模块 gating（#122）：关闭后导入入口不可用；已导入会话数据不受影响。
            let session_import_enabled = crate::modules::session_import::SessionImportModule::import_enabled(&self.module_registry);
            if crate::ui::chrome::popup_menu_button_enabled(
                ui,
                theme,
                l.import_ssh,
                ssh_import_enabled && session_import_enabled,
            )
            .clicked()
            {
                self.open_ssh_import_dialog(ctx);
            }
            if crate::ui::chrome::popup_menu_button_enabled(
                ui,
                theme,
                &format!("{}…", l.import_foreign),
                session_import_enabled,
            )
            .clicked()
            {
                self.open_foreign_import_dialog();
            }
            ui.separator();
            let close_tab_accel = crate::platform::close_tab_accel();
            if crate::ui::chrome::popup_menu_button_shortcut(
                ui,
                theme,
                l.close_tab,
                Some(&close_tab_accel),
            )
            .clicked()
            {
                self.request_close_active_tab();
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button(ui, theme, l.disconnect).clicked() {
                self.disconnect_ssh_keep_buffer_active(ctx);
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.reconnect).clicked() {
                self.reconnect_active_tab(ctx);
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button(ui, theme, l.batch_exec).clicked() {
                self.menu_open_batch_exec(ctx);
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button(ui, theme, crate::i18n::tr(ctx, "Quit", "退出"))
                .clicked()
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.menu_button(label(l.edit_menu), |ui| {
            crate::ui::chrome::apply_menu_popup_style(ui, theme);
            let copy_accel = crate::platform::terminal_copy_accel();
            if crate::ui::chrome::popup_menu_button_shortcut(ui, theme, l.copy, Some(&copy_accel))
                .clicked()
            {
                self.menu_copy_for_context(ctx);
            }
            let paste_accel = crate::platform::terminal_paste_accel();
            if crate::ui::chrome::popup_menu_button_shortcut(ui, theme, l.paste, Some(&paste_accel))
                .clicked()
            {
                self.menu_paste_for_context(ctx);
            }
            if crate::ui::chrome::popup_menu_button_accel_shift(ui, theme, l.select_all, "a").clicked()
            {
                self.menu_select_all_for_context(ctx);
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button_accel(ui, theme, l.find_in_terminal, "f").clicked()
            {
                self.toggle_terminal_search();
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button(ui, theme, l.preferences).clicked() {
                self.show_preferences_dialog = true;
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.ai_settings).clicked() {
                self.show_ai_settings_dialog = true;
            }
        });
        ui.menu_button(label(l.view_menu), |ui| {
            crate::ui::chrome::apply_menu_popup_style(ui, theme);
            if crate::ui::chrome::popup_menu_button(
                ui,
                theme,
                &format!(
                    "{} · {}",
                    if self.activity_rail_collapsed {
                        l.show_activity_rail
                    } else {
                        l.hide_activity_rail
                    },
                    crate::platform::accel("B"),
                ),
            )
            .clicked()
            {
                self.toggle_activity_rail();
            }
            if crate::ui::chrome::popup_menu_button(
                ui,
                theme,
                if self.sidebar_collapsed {
                    l.expand_sidebar
                } else {
                    l.collapse_sidebar
                },
            )
            .clicked()
            {
                self.sidebar_collapsed = !self.sidebar_collapsed;
                self.sidebar_user_dismissed_responsive = self.sidebar_collapsed;
            }
            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            if crate::ui::chrome::popup_menu_button(
                ui,
                theme,
                if maximized {
                    l.restore_window
                } else {
                    l.maximize_window
                },
            )
            .clicked()
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            ui.separator();
            if crate::ui::chrome::menu_toggle_item(ui, theme, self.show_sftp_panel, l.sftp_panel)
                .clicked()
            {
                self.toggle_sftp_panel(ctx);
            }
            if crate::ui::chrome::menu_toggle_item(
                ui,
                theme,
                self.show_port_forward_panel,
                l.port_forward_panel,
            )
            .clicked()
            {
                self.toggle_port_forward_panel(ctx);
            }
            if crate::ui::chrome::menu_toggle_item(
                ui,
                theme,
                self.show_fragment_panel,
                l.fragment_panel,
            )
            .clicked()
            {
                self.toggle_fragment_sidebar(ctx);
            }
            if crate::ui::chrome::menu_toggle_item(
                ui,
                theme,
                self.show_monitor_panel,
                l.monitor_panel,
            )
            .clicked()
            {
                self.toggle_monitor_panel(ctx);
            }
            if crate::ui::chrome::menu_toggle_item(ui, theme, self.show_ai_panel, l.ai_panel)
                .clicked()
            {
                self.toggle_ai_panel(ctx);
            }
            ui.separator();
            ui.menu_button(label(l.theme_menu), |ui| {
                crate::ui::chrome::apply_menu_popup_style(ui, theme);
                let current_idx = self.theme_manager.current;
                let theme_labels: Vec<String> = self
                    .theme_manager
                    .list_themes()
                    .iter()
                    .map(|t| crate::i18n::theme_display_name(ctx, &t.name).into_owned())
                    .collect();
                for (i, label) in theme_labels.iter().enumerate() {
                    let selected = i == current_idx;
                    if crate::ui::chrome::menu_theme_item(ui, theme, selected, label).clicked() {
                        self.theme_manager.set_theme_index(i);
                        self.theme_manager.save();
                        ui.ctx().request_repaint();
                    }
                }
            });
        });
        // 工具四段（#198）：片段与发送 / 历史 / 规则 / 账号与数据；偏好不进工具。
        ui.menu_button(label(l.tools_menu), |ui| {
            crate::ui::chrome::apply_menu_popup_style(ui, theme);
            // 1. 片段与发送
            if crate::ui::chrome::popup_menu_button(ui, theme, l.fragment_library).clicked() {
                self.fragment_library.open = true;
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.quick_fragments).clicked() {
                self.quick_selector.open = true;
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.send_sequences).clicked() {
                self.send_sequence_dialog.open_manager(&self.send_sequences);
            }
            ui.separator();
            // 2. 历史
            if crate::ui::chrome::popup_menu_button(ui, theme, l.command_history).clicked() {
                self.menu_open_command_history(ctx);
            }
            if crate::ui::chrome::popup_menu_button_accel_shift(ui, theme, l.paste_history, "P")
                .clicked()
            {
                self.menu_open_paste_history(ctx);
            }
            ui.separator();
            // 3. 规则与检查
            if crate::ui::chrome::popup_menu_button(ui, theme, l.rule_manager).clicked() {
                self.rule_manager_dialog
                    .open_output_trigger(&self.output_trigger_rules);
            }
            ui.separator();
            // 4. 账号与数据
            if crate::ui::chrome::popup_menu_button(ui, theme, l.credentials).clicked() {
                if self.ensure_right_dock_allowed_or_warn(ctx) {
                    self.open_right_dock_panel(ActiveRightDock::Credential);
                }
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.session_logs).clicked() {
                self.menu_open_session_log_browser(ctx);
            }
        });
        ui.menu_button(label(l.team_menu), |ui| {
            crate::ui::chrome::apply_menu_popup_style(ui, theme);
            if crate::ui::chrome::popup_menu_button(ui, theme, l.team_sign_in).clicked() {
                self.show_preferences_dialog = true;
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.team_members).clicked() {
                self.team_members_dialog.open(&mut self.team_service);
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button(ui, theme, l.cloud_sync).clicked() {
                if self.ensure_right_dock_allowed_or_warn(ctx) {
                    self.open_right_dock_panel(ActiveRightDock::CloudSync);
                }
            }
        });
        ui.menu_button(label(l.help_menu), |ui| {
            crate::ui::chrome::apply_menu_popup_style(ui, theme);
            if crate::ui::chrome::popup_menu_button(
                ui,
                theme,
                crate::i18n::tr(ctx, "Getting started", "新人上手"),
            )
            .clicked()
            {
                self.help_docs_dialog.open_page(HelpPage::Onboarding);
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.help_guide).clicked() {
                self.help_docs_dialog.open_page(HelpPage::QuickStart);
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.help_shortcuts).clicked() {
                self.help_docs_dialog.open_page(HelpPage::Shortcuts);
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button(ui, theme, l.help_online_docs).clicked() {
                if !crate::platform::open_url(crate::platform::DOCS_INDEX_URL) {
                    self.notify_auto(
                        crate::i18n::tr(ctx, "Failed to open browser", "无法打开浏览器")
                            .to_string(),
                    );
                }
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.ask_knowledge).clicked() {
                self.open_ask_knowledge_dialog();
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button(ui, theme, l.help_report_issue).clicked() {
                self.open_report_issue(ctx);
            }
            ui.menu_button(
                crate::i18n::tr(ctx, "Diagnostics", "诊断"),
                |ui| {
                    crate::ui::chrome::apply_menu_popup_style(ui, theme);
                    if crate::ui::chrome::popup_menu_button(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Open crash reports folder", "打开崩溃报告目录"),
                    )
                    .clicked()
                    {
                        self.open_crash_report_folder(ctx);
                    }
                    if crate::ui::chrome::popup_menu_button(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Open hang reports folder", "打开卡顿报告目录"),
                    )
                    .clicked()
                    {
                        self.open_hang_report_folder(ctx);
                    }
                },
            );
            ui.menu_button(
                crate::i18n::tr(ctx, "Advanced", "高级"),
                |ui| {
                    crate::ui::chrome::apply_menu_popup_style(ui, theme);
                    if crate::ui::chrome::popup_menu_button(ui, theme, l.audit_timeline).clicked() {
                        self.open_audit_timeline_dialog();
                    }
                    if crate::ui::chrome::popup_menu_button(ui, theme, l.install_audit_agent)
                        .clicked()
                    {
                        self.open_agent_install_dialog();
                    }
                    ui.separator();
                    ui.separator();
                    if crate::ui::chrome::popup_menu_button(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Open crash reports folder", "打开崩溃报告目录"),
                    )
                    .clicked()
                    {
                        self.open_crash_report_folder(ctx);
                    }
                    ui.menu_button(
                        crate::i18n::tr(ctx, "Freeze diagnostics", "卡顿诊断"),
                        |ui| {
                            crate::ui::chrome::apply_menu_popup_style(ui, theme);
                            if crate::ui::chrome::popup_menu_button(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Open diagnostics folder", "打开诊断目录"),
                            )
                            .clicked()
                            {
                                self.open_hang_report_folder(ctx);
                            }
                            if crate::ui::chrome::popup_menu_button(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Copy recent summary", "复制最近摘要"),
                            )
                            .clicked()
                            {
                                self.copy_recent_hang_report_summary(ctx);
                            }
                            if crate::ui::chrome::popup_menu_button(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Report with summary", "带摘要提交 Issue"),
                            )
                            .clicked()
                            {
                                self.open_issue_with_recent_hang_summary(ctx);
                            }
                        },
                    );
                },
            );
            ui.separator();
            let update_label = self.update_menu_label(ctx);
            if crate::ui::chrome::popup_menu_button(ui, theme, &update_label).clicked() {
                self.update_check_now(ctx);
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.help_about).clicked() {
                self.show_about_dialog = true;
            }
        });
    }
}
