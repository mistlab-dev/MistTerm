//! 团队成员列表弹窗(`GET /v1/teams/{team_id}/members`)

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
    ) -> bool {
        if !self.open {
            return false;
        }
        if !self.requested_fetch && service.is_logged_in() && !service.is_busy() {
            service.spawn_list_team_members();
            self.requested_fetch = true;
        }

        let title = crate::i18n::tr(ctx, "Team Members", "团队成员");
        let mut open = self.open;
        let mut should_close = false;
        let mut go_to_login = false;
        let logged_in = service.is_logged_in();
        let modal_sz = layout_util::modal_team_members_size(
            ctx,
            logged_in,
            service.team_members.len(),
        );
        chrome::modal_window("team_members_dialog", theme, ctx)
            .open(&mut open)
            .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
            .default_size(modal_sz)
            .movable(true)
            .resizable(true)
            .show(ctx, |ui| {
                chrome::modal_content_frame(theme).show(ui, |ui| {
                    if chrome::modal_header(
                        ui,
                        theme,
                        title,
                        chrome::modal_title_font_size(theme),
                    ) {
                        should_close = true;
                    }
                    // #282：未登录时不再显示占位团队名「Team」。
                    if logged_in {
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
                    } else {
                        ui.label(
                            chrome::rich_caption(
                                theme,
                                crate::i18n::tr(ctx, "Not signed in", "未登录"),
                            )
                            .color(theme.text_tertiary()),
                        );
                    }
                    ui.add_space(theme.spacing_sm());

                    if !logged_in {
                        // #282：未登录是中性状态，不是红色错误。
                        ui.label(
                            chrome::rich_caption(
                                theme,
                                crate::i18n::tr(
                                    ctx,
                                    "Sign in under Preferences → Team & sync to view members.",
                                    "请先在「偏好设置 → 团队与同步」登录后再查看成员。",
                                ),
                            )
                            .color(theme.color_form_hint()),
                        );
                        ui.add_space(theme.spacing_sm());
                        if chrome::panel_action_primary_button_with_icon_ex(
                            ui,
                            theme,
                            crate::ui::icons::IconId::Check,
                            crate::i18n::tr(ctx, "Go to sign-in", "去登录"),
                            true,
                        )
                        .clicked()
                        {
                            go_to_login = true;
                        }
                    } else if service.is_busy() && service.team_members.is_empty() {
                        ui.label(
                            chrome::rich_caption(
                                theme,
                                crate::i18n::tr(ctx, "Loading…", "加载中…"),
                            )
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
                                crate::i18n::tr(
                                    ctx,
                                    "No members yet. Sign in to a team that already has members, then tap Refresh.",
                                    "还没有成员。请登录已有成员的团队，再点「刷新」。",
                                ),
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
                    // #282：未登录时刷新按钮直接置灰，不等到点击后再报错。
                    if chrome::panel_action_button_ex(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Refresh", "刷新"),
                        logged_in && !service.is_busy(),
                    )
                    .clicked()
                    {
                        service.spawn_list_team_members();
                        self.requested_fetch = true;
                    }
                });
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            should_close = true;
        }
        self.open = open && !should_close && !go_to_login;
        if !self.open {
            self.requested_fetch = false;
        }
        go_to_login
    }
}

fn localize_members_error(ctx: &egui::Context, msg: &str) -> String {
    if msg.contains("404") || msg.contains("Not Found") {
        // #233：主文白话，不摊 REST 路径。
        return crate::i18n::tr(
            ctx,
            "This team server does not offer a member list yet.",
            "这个团队服务器暂时还没有成员列表。",
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
