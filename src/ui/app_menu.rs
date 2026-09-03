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

        egui::menu::menu_button(ui, label(l.terminal_menu), |ui| {
            crate::ui::chrome::apply_menu_popup_style(ui, theme);
            if crate::ui::chrome::popup_menu_button(ui, theme, l.new_session).clicked() {
                self.show_new_session_dialog = true;
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.new_tab).clicked() {
                self.open_new_tab_from_selection(ctx);
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button_enabled(
                ui,
                theme,
                l.import_ssh,
                ssh_import_enabled,
            )
            .clicked()
            {
                self.open_ssh_import_dialog(ctx);
                ui.close_menu();
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button(ui, theme, l.close_tab).clicked() {
                self.request_close_active_tab();
                ui.close_menu();
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button(ui, theme, l.disconnect).clicked() {
                self.disconnect_ssh_keep_buffer_active(ctx);
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.reconnect).clicked() {
                self.reconnect_active_tab(ctx);
                ui.close_menu();
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button(ui, theme, l.preferences).clicked() {
                self.show_preferences_dialog = true;
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, crate::i18n::tr(ctx, "Quit", "退出"))
                .clicked()
            {
                frame.close();
                ui.close_menu();
            }
        });
        egui::menu::menu_button(ui, label(l.edit_menu), |ui| {
            crate::ui::chrome::apply_menu_popup_style(ui, theme);
            if crate::ui::chrome::popup_menu_button(ui, theme, l.copy).clicked() {
                self.menu_copy_for_context(ctx);
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.paste).clicked() {
                self.menu_paste_for_context(ctx);
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.select_all).clicked() {
                self.menu_select_all_for_context(ctx);
                ui.close_menu();
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button(ui, theme, l.find_in_terminal).clicked() {
                self.toggle_terminal_search();
                ui.close_menu();
            }
        });
        egui::menu::menu_button(ui, label(l.view_menu), |ui| {
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
                ui.close_menu();
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
                ui.close_menu();
            }
            let maximized = frame.info().window_info.maximized;
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
                frame.set_maximized(!maximized);
                ui.close_menu();
            }
            ui.separator();
            if crate::ui::chrome::menu_toggle_item(ui, theme, self.show_sftp_panel, l.sftp_panel)
                .clicked()
            {
                self.toggle_sftp_panel(ctx);
                ui.close_menu();
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
                ui.close_menu();
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
                ui.close_menu();
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
                ui.close_menu();
            }
            if crate::ui::chrome::menu_toggle_item(ui, theme, self.show_ai_panel, l.ai_panel)
                .clicked()
            {
                self.toggle_ai_panel(ctx);
                ui.close_menu();
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
                        ui.close_menu();
                    }
                }
            });
        });
        egui::menu::menu_button(ui, label(l.tools_menu), |ui| {
            crate::ui::chrome::apply_menu_popup_style(ui, theme);
            if crate::ui::chrome::popup_menu_button(ui, theme, l.ai_settings).clicked() {
                self.show_ai_settings_dialog = true;
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.fragment_library).clicked() {
                self.fragment_library.open = true;
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.quick_fragments).clicked() {
                self.quick_selector.open = true;
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.command_history).clicked() {
                self.menu_open_command_history(ctx);
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.batch_exec).clicked() {
                self.menu_open_batch_exec(ctx);
                ui.close_menu();
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button(ui, theme, l.credentials).clicked() {
                if self.ensure_right_dock_allowed_or_warn(ctx) {
                    self.open_right_dock_panel(ActiveRightDock::Credential);
                }
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.team_account).clicked() {
                self.show_preferences_dialog = true;
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.cloud_sync).clicked() {
                if self.ensure_right_dock_allowed_or_warn(ctx) {
                    self.open_right_dock_panel(ActiveRightDock::CloudSync);
                }
                ui.close_menu();
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button(ui, theme, l.session_logs).clicked() {
                self.menu_open_session_log_browser(ctx);
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button(
                ui,
                theme,
                crate::i18n::tr(ctx, "Audit timeline", "审计时间线"),
            )
            .clicked()
            {
                self.open_audit_timeline_dialog();
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button(
                ui,
                theme,
                crate::i18n::tr(ctx, "Install audit Agent", "安装审计 Agent"),
            )
            .clicked()
            {
                self.open_agent_install_dialog();
                ui.close_menu();
            }
        });
        egui::menu::menu_button(ui, label(l.help_menu), |ui| {
            crate::ui::chrome::apply_menu_popup_style(ui, theme);
            if crate::ui::chrome::popup_menu_button(ui, theme, l.help_guide).clicked() {
                self.help_docs_dialog.open_page(HelpPage::QuickStart);
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.help_shortcuts).clicked() {
                self.help_docs_dialog.open_page(HelpPage::Shortcuts);
                ui.close_menu();
            }
            ui.separator();
            if crate::ui::chrome::popup_menu_button(ui, theme, l.help_online_docs).clicked() {
                if !crate::platform::open_url(crate::platform::DOCS_INDEX_URL) {
                    self.notify_auto(
                        crate::i18n::tr(ctx, "Failed to open browser", "无法打开浏览器")
                            .to_string(),
                    );
                }
                ui.close_menu();
            }
            if crate::ui::chrome::popup_menu_button(ui, theme, l.help_report_issue).clicked() {
                self.open_report_issue(ctx);
                ui.close_menu();
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
                        ui.close_menu();
                    }
                    if crate::ui::chrome::popup_menu_button(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Copy recent summary", "复制最近摘要"),
                    )
                    .clicked()
                    {
                        self.copy_recent_hang_report_summary(ctx);
                        ui.close_menu();
                    }
                    if crate::ui::chrome::popup_menu_button(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Report with summary", "带摘要提交 Issue"),
                    )
                    .clicked()
                    {
                        self.open_issue_with_recent_hang_summary(ctx);
                        ui.close_menu();
                    }
                },
            );
            ui.separator();
            if crate::ui::chrome::popup_menu_button(ui, theme, l.help_about).clicked() {
                self.show_about_dialog = true;
                ui.close_menu();
            }
        });
    }
}
