//! #335：空态「打开已有连接」主机选择弹层。
//! 有 ≥1 台已保存主机时，空态第三 CTA 打开这里；选一台即开新会话标签。
//! 无保存主机时入口隐藏（见 `show_welcome`），本弹层不会被打开。

use super::*;

impl MistTermApp {
    pub(crate) fn render_open_existing_connection_modal(
        &mut self,
        ctx: &egui::Context,
        theme: &crate::ui::theme::Theme,
    ) {
        if !self.dialog_flags.open_existing_connection {
            return;
        }
        let mut open = true;
        let mut should_close = false;
        let mut open_session_id: Option<String> = None;

        // 与侧栏同口径排序：最近连接优先。
        let mut sessions: Vec<_> = self.session_manager.list_sessions().to_vec();
        crate::core::session_sort::sort_sessions(
            &mut sessions,
            crate::core::session_sort::SessionSortBy::LastConnected,
        );

        let count_line = if sessions.is_empty() {
            crate::i18n::tr(
                ctx,
                "No saved hosts yet. Use \"New connection\" to add one.",
                "还没有已保存的主机，先用「新建连接」添加一台。",
            )
            .to_string()
        } else {
            crate::i18n::tr(
                ctx,
                "Select a saved host to open a session.",
                "选择一台已保存的主机，打开会话。",
            )
            .to_string()
        };

        let sz = egui::vec2(460.0, 420.0);
        crate::ui::chrome::modal_window("open_existing_connection_modal", theme, ctx)
            .open(&mut open)
            .default_pos(crate::ui::layout_util::modal_center_pos(ctx, sz))
            .movable(true)
            .resizable(false)
            .fixed_size(sz)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    let title = crate::i18n::tr(
                        ctx,
                        "Open existing connection",
                        "打开已有连接",
                    );
                    Self::modal_header(ui, theme, title, &mut should_close);
                    ui.label(
                        egui::RichText::new(&count_line)
                            .size(theme.font_size_small())
                            .color(theme.color_form_hint()),
                    );
                    ui.add_space(theme.spacing_sm());
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for session in &sessions {
                            let host_line = if session.port != crate::core::session::DEFAULT_SSH_PORT {
                                format!("{}:{}", session.host, session.port)
                            } else {
                                session.host.clone()
                            };
                            let label = format!("{}  ·  {}", session.name, host_line);
                            if crate::ui::chrome::popup_menu_button(ui, theme, &label)
                                .on_hover_text(format!("{}@{}", session.username, host_line))
                                .clicked()
                            {
                                open_session_id = Some(session.id.clone());
                            }
                        }
                    });
                });
            });

        if let Some(sid) = open_session_id {
            if let Some(session) = self.session_manager.get_session(&sid).cloned() {
                self.open_new_tab_for_session(ctx, &session);
                self.dialog_flags.open_existing_connection = false;
                return;
            }
        }
        if should_close || !open {
            self.dialog_flags.open_existing_connection = false;
        }
    }
}
