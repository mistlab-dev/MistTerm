//! 团队成员列表弹窗（`GET /v1/teams/{team_id}/members`）

use crate::core::team::{TeamMember, TeamService};
use crate::ui::chrome;
use crate::ui::layout_util;
use crate::ui::theme::Theme;
use eframe::egui;

pub struct TeamMembersDialog {
    pub open: bool,
    requested_fetch: bool,
}

impl Default for TeamMembersDialog {
    fn default() -> Self {
        Self {
            open: false,
            requested_fetch: false,
        }
    }
}

impl TeamMembersDialog {
    pub fn open(&mut self, service: &mut TeamService) {
        self.open = true;
        self.requested_fetch = false;
        service.team_members.clear();
        service.team_members_error = None;
    }

    pub fn show_modal(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        service: &mut TeamService,
    ) {
        if !self.open {
            return;
        }
        if !self.requested_fetch && !service.is_busy() {
            service.spawn_list_team_members();
            self.requested_fetch = true;
        }

        let title = crate::i18n::tr(ctx, "Team Members", "团队成员");
        let mut keep_open = self.open;
        egui::Window::new(title)
            .id(egui::Id::new("team_members_dialog"))
            .collapsible(false)
            .resizable(true)
            .default_width(420.0)
            .min_width(320.0)
            .show(ctx, |ui| {
                layout_util::set_width_to_available(ui);
                ui.label(
                    chrome::rich_caption(
                        theme,
                        &format!(
                            "{}: {}",
                            crate::i18n::tr(ctx, "Team", "团队"),
                            service.current_team_name()
                        ),
                    ),
                );
                ui.add_space(theme.spacing_sm());

                if !service.is_logged_in() {
                    ui.label(
                        chrome::rich_caption(
                            theme,
                            crate::i18n::tr(
                                ctx,
                                "Sign in under Preferences → Team to view members.",
                                "请先在「偏好设置 → 团队平台」登录后再查看成员。",
                            ),
                        )
                        .color(theme.red_color()),
                    );
                } else if service.is_busy() && service.team_members.is_empty() {
                    ui.label(
                        chrome::rich_caption(theme, crate::i18n::tr(ctx, "Loading…", "加载中…"))
                            .color(theme.text_tertiary()),
                    );
                } else if let Some(err) = &service.team_members_error {
                    ui.label(
                        chrome::rich_caption(theme, &localize_members_error(ctx, err))
                            .color(theme.red_color()),
                    );
                } else if service.team_members.is_empty() {
                    ui.label(
                        chrome::rich_caption(
                            theme,
                            crate::i18n::tr(ctx, "No members returned.", "暂无成员数据。"),
                        )
                        .color(theme.text_tertiary()),
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(280.0)
                        .show(ui, |ui| {
                            paint_members_table(ui, theme, ctx, &service.team_members);
                        });
                }

                ui.add_space(theme.spacing_sm());
                ui.horizontal(|ui| {
                    if chrome::panel_action_button_ex(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Refresh", "刷新"),
                        service.is_logged_in() && !service.is_busy(),
                    )
                    .clicked()
                    {
                        service.spawn_list_team_members();
                        self.requested_fetch = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if chrome::panel_action_primary_button_ex(
                            ui,
                            theme,
                            crate::i18n::tr(ctx, "Close", "关闭"),
                            true,
                        )
                        .clicked()
                        {
                            keep_open = false;
                        }
                    });
                });
            });
        self.open = keep_open;
        if !self.open {
            self.requested_fetch = false;
        }
    }
}

fn localize_members_error(ctx: &egui::Context, msg: &str) -> String {
    if msg.contains("404") || msg.contains("Not Found") {
        return crate::i18n::tr(
            ctx,
            "Member list API is not available on the server yet.",
            "服务端尚未提供成员列表接口（GET /v1/teams/{id}/members）。",
        )
        .to_string();
    }
    msg.to_string()
}

fn paint_members_table(
    ui: &mut egui::Ui,
    theme: &Theme,
    ctx: &egui::Context,
    members: &[TeamMember],
) {
    let cap_font = egui::FontId::proportional(theme.font_size_small());
    let cap = theme.text_tertiary();
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(crate::i18n::tr(ctx, "Name", "名称"))
                .font(cap_font.clone())
                .color(cap),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(crate::i18n::tr(ctx, "Role", "角色"))
                    .font(cap_font)
                    .color(cap),
            );
        });
    });
    ui.separator();
    for m in members {
        let name = if !m.display_name.is_empty() {
            m.display_name.as_str()
        } else if !m.username.is_empty() {
            m.username.as_str()
        } else {
            m.email.as_str()
        };
        let sub = if !m.email.is_empty() && m.email != name {
            Some(m.email.as_str())
        } else {
            None
        };
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(name).color(theme.text_primary()));
                if let Some(s) = sub {
                    ui.label(egui::RichText::new(s).small().color(theme.text_tertiary()));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(&m.role).color(theme.text_secondary()));
            });
        });
        ui.add_space(theme.spacing_xs());
    }
}
