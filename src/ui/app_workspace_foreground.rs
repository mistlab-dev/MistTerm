use super::*;

impl MistTermApp {
    pub(crate) fn render_workspace_right_dock_foreground(
        &mut self,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
        top_chrome_height: f32,
    ) {
        let mut cred_action: Option<CredentialPanelAction> = None;

        crate::ui::chrome::paint_right_dock_screen_gutter(ctx, theme, top_chrome_height);

        // 仅抑制会与右 dock 标题栏 × 重叠的模态窗；偏好/关于/帮助等视口居中窗仍保留 dock。
        let paint_right_dock_fg = !self.suppress_right_dock_foreground();
        // 右→左绘制 Foreground：靠左的 dock 后绘、叠在上层，关闭钮不被右邻 dock 挡住。
        if paint_right_dock_fg {
            self.show_fragment_panel_foreground(ctx, theme);
        }
        if paint_right_dock_fg && self.credential_panel.open {
            let mut close_cred = false;
            self.credential_panel.show_foreground_panel(
                ctx,
                theme,
                &self.app_settings.vault,
                &self.audit_logger,
                &mut cred_action,
                &mut close_cred,
            );
            if close_cred {
                self.credential_panel.open = false;
            }
        }
        if let Some(CredentialPanelAction::UseForQuickConnect(c)) = cred_action {
            self.apply_credential_to_new_session_form(ctx, c);
        }

        if paint_right_dock_fg && self.cloud_sync_panel.open {
            let fragments_export_path = FragmentManager::default_config_path();
            let sessions_export_path = self.session_manager.storage_path().clone();
            let theme_export_path = ThemeManager::config_path();
            let mut cloud_sync_deps = CloudSyncDeps {
                fragments_path: &fragments_export_path,
                sessions_path: &sessions_export_path,
                theme_path: &theme_export_path,
                fragment_manager: &mut self.fragment_manager,
                theme_manager: &mut self.theme_manager,
                session_manager: &mut self.session_manager,
                credential_panel: &mut self.credential_panel,
                audit: Some(&self.audit_logger),
            };
            let mut close_cloud = false;
            let team_action = self.cloud_sync_panel.show_foreground_panel(
                ctx,
                theme,
                &mut cloud_sync_deps,
                &mut close_cloud,
                Some(&mut self.team_service),
                Some(&mut self.team_login_form),
                Some(&mut self.app_settings),
            );
            if matches!(team_action, crate::ui::team_ui::TeamUiAction::OpenMembers) {
                self.team_members_dialog.open(&mut self.team_service);
            }
            if let Some(err) = self.cloud_sync_panel.take_pending_toast_error() {
                self.notify_error(err);
            }
            if close_cloud {
                self.cloud_sync_panel.open = false;
            }
        }

        if paint_right_dock_fg && self.show_monitor_panel {
            self.monitor_panel
                .show_foreground_panel(ctx, theme, &mut self.show_monitor_panel);
        }
        if paint_right_dock_fg && self.show_sftp_panel {
            let mut close_sftp_panel = false;
            let current_terminal_ref = self
                .active_tab
                .and_then(|idx| self.tabs.get(idx).and_then(|t| t.active_terminal()));
            self.sftp_panel.show_foreground_panel(
                ctx,
                theme,
                current_terminal_ref,
                &self.audit_logger,
                &mut close_sftp_panel,
            );
            let (sftp_ok, sftp_err) = self.sftp_panel.take_pending_status();
            if let Some(e) = sftp_err {
                self.notify_error(e);
            } else if let Some(m) = sftp_ok {
                self.notify_auto(m);
            }
            if close_sftp_panel {
                self.show_sftp_panel = false;
            }
        }
        if paint_right_dock_fg && self.show_port_forward_panel {
            let mut close_port_forward = false;
            let current_terminal_ref = self
                .active_tab
                .and_then(|idx| self.tabs.get(idx).and_then(|t| t.active_terminal()));
            let session_profile = self.active_tab_session_profile();
            self.port_forward_panel.show_foreground_panel(
                ctx,
                theme,
                current_terminal_ref,
                session_profile.as_ref(),
                &mut close_port_forward,
            );
            if close_port_forward {
                self.show_port_forward_panel = false;
                self.port_forward_last_tab = None;
            }
        }
        if !self.show_monitor_panel {
            self.sync_monitor_panel_to_active_tab();
        }
        if self.show_ai_panel || self.show_ai_settings_dialog {
            self.ai_panel.poll_background(ctx, &mut self.app_settings);
            if let Some(err) = self.ai_panel.take_pending_toast_error() {
                self.notify_error(err);
            }
        }
        if paint_right_dock_fg && self.show_ai_panel {
            self.ai_panel.show_foreground_panel(
                ctx,
                theme,
                &mut self.show_ai_panel,
                &mut self.app_settings,
            );
            if let Some(err) = self.ai_panel.take_pending_toast_error() {
                self.notify_error(err);
            }
        }
        // 改宽手柄：全部 dock 正文之后、屏上左→右绘制，避免右邻 dock 正文挡住左缝（如监控+片段并排）。
        if paint_right_dock_fg {
            if self.show_ai_panel {
                crate::ui::chrome::show_right_dock_resize_grip_for_slot(
                    ctx,
                    theme,
                    "mistterm_ai_fg",
                    self.ai_panel.last_panel_slot_rect(),
                    layout_util::AI_PANEL_ID,
                    layout_util::SidePanelProfile::Standard,
                );
            }
            if self.show_port_forward_panel {
                crate::ui::chrome::show_right_dock_resize_grip_for_slot(
                    ctx,
                    theme,
                    "mistterm_port_fwd_fg",
                    self.port_forward_panel.last_panel_slot_rect(),
                    "port_forward_panel",
                    layout_util::SidePanelProfile::Standard,
                );
            }
            if self.show_monitor_panel {
                crate::ui::chrome::show_right_dock_resize_grip_for_slot(
                    ctx,
                    theme,
                    "mistterm_monitor_fg",
                    self.monitor_panel.last_panel_slot_rect(),
                    layout_util::MONITOR_PANEL_ID,
                    layout_util::SidePanelProfile::Monitor,
                );
            }
            if self.show_sftp_panel {
                crate::ui::chrome::show_right_dock_resize_grip_for_slot(
                    ctx,
                    theme,
                    "mistterm_sftp_fg",
                    self.sftp_panel.last_panel_slot_rect(),
                    "sftp_browser_panel",
                    layout_util::SidePanelProfile::Standard,
                );
            }
            if self.cloud_sync_panel.open {
                crate::ui::chrome::show_right_dock_resize_grip_for_slot(
                    ctx,
                    theme,
                    "mistterm_cloud_sync_fg",
                    self.cloud_sync_panel.last_panel_slot_rect(),
                    "cloud_sync_panel",
                    layout_util::SidePanelProfile::Standard,
                );
            }
            if self.credential_panel.open {
                crate::ui::chrome::show_right_dock_resize_grip_for_slot(
                    ctx,
                    theme,
                    "mistterm_credential_fg",
                    self.credential_panel.last_panel_slot_rect(),
                    "credential_panel",
                    layout_util::SidePanelProfile::Standard,
                );
            }
            if self.show_fragment_panel {
                crate::ui::chrome::show_right_dock_resize_grip_for_slot(
                    ctx,
                    theme,
                    "mistterm_fragment_fg",
                    self.fragment_panel_slot_rect,
                    layout_util::FRAGMENT_PANEL_ID,
                    layout_util::SidePanelProfile::Fragment,
                );
            }
        }
    }
}
