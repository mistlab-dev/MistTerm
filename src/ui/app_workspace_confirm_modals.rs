use super::*;

impl MistTermApp {
    pub(crate) fn render_workspace_confirm_modals(
        &mut self,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
    ) {
        if self.large_upload_pending_path.is_some() {
            let path_hint = self
                .large_upload_pending_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let mut open = true;
            #[derive(Clone, Copy)]
            enum LargePick {
                Dismiss,
                Zmodem,
                Scp,
            }
            let mut pick: Option<LargePick> = None;
            let mut should_close = false;
            let modal_sz = layout_util::modal_quick_fragment_size(ctx);
            crate::ui::chrome::modal_window("large_upload_modal", theme, ctx)
                .open(&mut open)
                .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
                .movable(true)
                .resizable(false)
                .fixed_size(modal_sz)
                .show(ctx, |ui| {
                    crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                        Self::modal_header_title_only(
                            ui,
                            theme,
                            crate::i18n::tr(ctx, "Large file upload", "大文件上传"),
                            &mut should_close,
                        );
                        ui.label(
                            egui::RichText::new(
                                crate::i18n::tr(
                                    ctx,
                                    "\"{0}\" ≥ 10MB: SCP has no resume; ZMODEM needs lrzsz on the host and sends rz -y to the PTY.",
                                    "「{0}」≥ 10MB：SCP 无断点续传；ZMODEM 需远端 lrzsz，并向 PTY 发送 rz -y。",
                                )
                                .replace("{0}", &path_hint),
                            )
                            .size(theme.font_size_panel_title())
                            .color(theme.color_body_text_muted()),
                        );
                        ui.add_space(theme.spacing_list_item_x());
                        ui.horizontal(|ui| {
                            if crate::ui::chrome::modal_primary_button(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "ZMODEM (recommended)", "ZMODEM(推荐)"),
                            )
                                .clicked()
                            {
                                pick = Some(LargePick::Zmodem);
                            }
                            if crate::ui::chrome::modal_secondary_button(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Use SCP anyway", "仍用 SCP"),
                            )
                                .clicked()
                            {
                                pick = Some(LargePick::Scp);
                            }
                        });
                        ui.add_space(theme.spacing_md());
                        if crate::ui::chrome::modal_secondary_icon_button(
                            ui,
                            theme,
                            crate::ui::icons::IconId::Cross,
                            crate::i18n::tr(ctx, "Cancel", "取消"),
                        )
                        .clicked() {
                            pick = Some(LargePick::Dismiss);
                        }
                    });
                });
            if (!open || should_close) && pick.is_none() {
                pick = Some(LargePick::Dismiss);
            }
            match pick {
                Some(LargePick::Zmodem) => {
                    if let Some(p) = self.large_upload_pending_path.take() {
                        if let Some(t) = self.current_terminal_mut() {
                            t.queue_zmodem_upload_after_rz(p.clone());
                            self.notify_auto(format!(
                                "{} {}",
                                crate::i18n::tr(
                                    ctx,
                                    "rz -y sent; ZMODEM upload after handshake:",
                                    "已发送 rz -y，握手就绪后将通过 ZMODEM 上传：",
                                ),
                                p.display(),
                            ));
                        }
                    }
                }
                Some(LargePick::Scp) => {
                    if let Some(p) = self.large_upload_pending_path.take() {
                        if let Some(t) = self.current_terminal_mut() {
                            match t.start_upload(p.as_path()) {
                                Ok(_) => {
                                    self.notify_auto(format!(
                                        "{} {}",
                                        crate::i18n::tr(
                                            ctx,
                                            "Starting SCP upload:",
                                            "开始 SCP 上传："
                                        ),
                                        p.display(),
                                    ));
                                }
                                Err(e) => {
                                    self.notify_error(format!(
                                        "{} {}",
                                        crate::i18n::tr(
                                            ctx,
                                            "SCP upload start failed:",
                                            "SCP 上传启动失败：",
                                        ),
                                        e,
                                    ));
                                }
                            }
                        }
                    }
                }
                Some(LargePick::Dismiss) => {
                    self.large_upload_pending_path = None;
                }
                None => {}
            }
        }

        if let Some((del_id, del_name)) = self.delete_session_confirm.clone() {
            let mut open = true;
            let mut should_close = false;
            let mut do_delete = false;
            let modal_sz = layout_util::modal_confirm_size(ctx);
            crate::ui::chrome::modal_window("delete_session_confirm", theme, ctx)
                .open(&mut open)
                .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
                .movable(true)
                .resizable(false)
                .fixed_size(modal_sz)
                .show(ctx, |ui| {
                    crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                        Self::modal_header_title_only(
                            ui,
                            theme,
                            crate::i18n::tr(ctx, "Delete session", "删除会话"),
                            &mut should_close,
                        );
                        ui.label(
                            egui::RichText::new(
                                crate::i18n::tr(
                                    ctx,
                                    "Delete session profile for \"{0}\"? This cannot be undone.",
                                    "确认删除「{0}」的会话配置?此操作不可恢复。",
                                )
                                .replace("{0}", &del_name),
                            )
                            .size(theme.font_size_normal())
                            .color(theme.color_body_text_muted()),
                        );
                        ui.add_space(theme.spacing_lg());
                        crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                            if crate::ui::chrome::modal_danger_icon_button(
                                ui,
                                th,
                                crate::ui::icons::IconId::Trash,
                                crate::i18n::tr(ctx, "Delete", "删除"),
                            )
                            .clicked()
                            {
                                do_delete = true;
                                should_close = true;
                            }
                            if crate::ui::chrome::modal_secondary_icon_button(
                                ui,
                                th,
                                crate::ui::icons::IconId::Cross,
                                crate::i18n::tr(ctx, "Cancel", "取消"),
                            )
                            .clicked()
                            {
                                should_close = true;
                            }
                        });
                    });
                });
            if do_delete {
                self.delete_session(ctx, &del_id);
            }
            if !open || should_close {
                self.delete_session_confirm = None;
            }
        }

        if let Some(confirm) = self.cmd_audit_confirm.clone() {
            let mut open = true;
            let mut should_close = false;
            let mut proceed = false;
            let timeout_secs = self.cmd_audit_engine.confirm_timeout_secs();
            let timed_out =
                confirm.started.elapsed() >= std::time::Duration::from_secs(timeout_secs.max(30));
            if timed_out {
                should_close = true;
            }
            let command = confirm.command.clone();
            let audit = confirm.audit.clone();
            let from_server = matches!(confirm.source, crate::core::CmdAuditSource::Server);
            let modal_sz = layout_util::modal_confirm_size(ctx);
            crate::ui::chrome::modal_window("cmd_audit_confirm", theme, ctx)
                .open(&mut open)
                .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
                .movable(true)
                .resizable(false)
                .fixed_size(modal_sz)
                .show(ctx, |ui| {
                    crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                        Self::modal_header_title_only(
                            ui,
                            theme,
                            if from_server {
                                crate::i18n::tr(
                                    ctx,
                                    "Server policy: confirm to run",
                                    "服务器策略：确认执行",
                                )
                            } else {
                                crate::i18n::tr(
                                    ctx,
                                    "Local check: confirm to run",
                                    "本地检查：确认执行",
                                )
                            },
                            &mut should_close,
                        );
                        ui.label(
                            egui::RichText::new(if from_server {
                                crate::i18n::tr(
                                    ctx,
                                    "The remote server requires confirmation before running:",
                                    "远程服务器要求确认后才能执行：",
                                )
                            } else {
                                crate::i18n::tr(
                                    ctx,
                                    "Local quick check flagged a sensitive operation (not a server policy):",
                                    "本地快捷提示检测到敏感操作(非服务器强制策略)：",
                                )
                            })
                            .size(theme.font_size_normal())
                            .color(theme.color_body_text_muted()),
                        );
                        ui.add_space(theme.spacing_sm());
                        ui.label(
                            egui::RichText::new(format!(
                                "{} {}",
                                crate::i18n::tr(ctx, "Command:", "命令:"),
                                command_preview(&command, 120),
                            ))
                            .size(theme.font_size_normal())
                            .color(theme.color_body_text_muted()),
                        );
                        if let Some(m) = audit.matches.first() {
                            ui.add_space(theme.spacing_sm());
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} {} ({})",
                                    crate::i18n::tr(ctx, "Rule:", "匹配规则:"),
                                    m.rule_id,
                                    m.level,
                                ))
                                .size(theme.font_size_small())
                                .color(theme.color_body_text_muted()),
                            );
                            if !m.message.is_empty() {
                                ui.label(
                                    egui::RichText::new(&m.message)
                                        .size(theme.font_size_small())
                                        .color(theme.color_body_text_muted()),
                                );
                            }
                        }
                        ui.add_space(theme.spacing_lg());
                        crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                            if crate::ui::chrome::modal_primary_button_with_icon(
                                ui,
                                th,
                                crate::ui::icons::IconId::Check,
                                if from_server {
                                    crate::i18n::tr(ctx, "Approve and send", "放行并发送")
                                } else {
                                    crate::i18n::tr(ctx, "Confirm send", "确认发送")
                                },
                            )
                            .clicked()
                            {
                                proceed = true;
                                should_close = true;
                            }
                            if crate::ui::chrome::modal_secondary_icon_button(
                                ui,
                                th,
                                crate::ui::icons::IconId::Cross,
                                crate::i18n::tr(ctx, "Cancel", "取消"),
                            )
                            .clicked()
                            {
                                should_close = true;
                            }
                        });
                    });
                });
            if timed_out && self.cmd_audit_confirm.is_some() {
                self.confirm_cmd_audit(ctx, false);
            } else if should_close {
                self.confirm_cmd_audit(ctx, proceed);
            }
            if !open && self.cmd_audit_confirm.is_some() {
                self.cmd_audit_confirm = None;
            }
        }

        if let Some(pending_idx) = self.close_tab_confirm_idx {
            if pending_idx >= self.tabs.len() {
                self.close_tab_confirm_idx = None;
            } else {
                let tab_title = self.tabs[pending_idx].display_title();
                let mut open = true;
                let mut should_close = false;
                let mut confirmed = false;
                let modal_sz = layout_util::modal_clone_size(ctx);
                crate::ui::chrome::modal_window("close_tab_confirm", theme, ctx)
                    .open(&mut open)
                    .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
                    .movable(true)
                    .resizable(false)
                    .fixed_size(modal_sz)
                    .show(ctx, |ui| {
                        crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                            Self::modal_header_title_only(
                                ui,
                                theme,
                                crate::i18n::tr(ctx, "Close tab", "关闭标签"),
                                &mut should_close,
                            );
                            ui.label(
                                egui::RichText::new(
                                    crate::i18n::tr(
                                        ctx,
                                        "Tab \"{0}\" is still connected or negotiating. Close anyway?",
                                        "标签「{0}」仍连接或握手中，确定关闭?",
                                    )
                                    .replace("{0}", &tab_title),
                                )
                                .size(theme.font_size_normal())
                                .color(theme.color_body_text_muted()),
                            );
                            ui.add_space(theme.spacing_lg());
                            crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                                if crate::ui::chrome::modal_primary_button_with_icon(
                                    ui,
                                    th,
                                    crate::ui::icons::IconId::Check,
                                    crate::i18n::tr(ctx, "Close anyway", "仍要关闭"),
                                )
                                    .clicked() {
                                    confirmed = true;
                                    should_close = true;
                                }
                                if crate::ui::chrome::modal_secondary_icon_button(
                                    ui,
                                    th,
                                    crate::ui::icons::IconId::Cross,
                                    crate::i18n::tr(ctx, "Cancel", "取消"),
                                )
                                .clicked() {
                                    should_close = true;
                                }
                            });
                        });
                    });
                if confirmed && pending_idx < self.tabs.len() {
                    self.remove_tab_at(pending_idx);
                }
                if !open || should_close {
                    self.close_tab_confirm_idx = None;
                }
            }
        }
    }
}
