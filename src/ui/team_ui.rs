//! 团队平台 UI 片段（偏好设置 / 云端同步面板复用）。

use eframe::egui::{self, RichText};

use crate::core::team::{
    team_web_forgot_password_url, team_web_register_url, OAuthProvider, TeamService,
};
use crate::core::{AuditCategory, AuditEvent, AuditOutcome};
use crate::i18n;
use crate::platform::shell;
use crate::ui::chrome;
use crate::ui::theme::Theme;

pub struct TeamLoginForm {
    pub identifier: String,
    pub password: String,
    pub use_email: bool,
}

impl Default for TeamLoginForm {
    fn default() -> Self {
        Self {
            identifier: String::new(),
            password: String::new(),
            use_email: true,
        }
    }
}

/// 绘制团队登录 / 团队选择 / 同步控件。
///
/// `id_scope` 须在每处调用点唯一（如 `"preferences_team"` / `"cloud_sync_team"`），
/// 避免偏好设置与云端同步同帧绘制时 egui 控件 ID 冲突。
pub fn paint_team_controls(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    theme: &Theme,
    service: &mut TeamService,
    form: &mut TeamLoginForm,
    audit: Option<&crate::core::AuditLogger>,
    pref_w: f32,
    id_scope: &str,
) -> TeamUiAction {
    ui.push_id(id_scope, |ui| {
        let mut action = TeamUiAction::None;

        if service.is_logged_in() {
            if let Some(user) = &service.state.user {
                ui.label(
                    chrome::rich_caption(theme, &format!(
                        "{}: {} ({})",
                        i18n::tr(ctx, "Signed in", "已登录"),
                        user.display_name,
                        user.email
                    ))
                    .strong(),
                );
            }

            if !service.state.teams.is_empty() {
                chrome::form_field_label(
                    ui,
                    theme,
                    i18n::tr(ctx, "Current team", "当前团队"),
                );
                let current = service
                    .state
                    .current_team_id
                    .clone()
                    .unwrap_or_default();
                let teams = service.state.teams.clone();
                egui::ComboBox::from_id_source(ui.make_persistent_id("current_team"))
                    .selected_text(
                        teams
                            .iter()
                            .find(|m| m.team.id == current)
                            .map(|m| m.team.name.as_str())
                            .unwrap_or("—"),
                    )
                    .width(pref_w.min(ui.available_width()))
                    .show_ui(ui, |ui| {
                        chrome::apply_menu_popup_style(ui, theme);
                        for m in &teams {
                            let label = format!("{} ({})", m.team.name, m.role);
                            if ui
                                .selectable_value(
                                    &mut service.state.current_team_id,
                                    Some(m.team.id.clone()),
                                    label,
                                )
                                .clicked()
                            {
                                let _ = service.state.save();
                                action = TeamUiAction::TeamChanged;
                            }
                        }
                    });
            }

            ui.horizontal(|ui| {
                if chrome::panel_action_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Server,
                    i18n::tr(ctx, "Team members…", "团队成员…"),
                )
                .clicked()
                {
                    action = TeamUiAction::OpenMembers;
                }
                if chrome::panel_action_primary_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Cloud,
                    i18n::tr(ctx, "Sync team fragments now", "立即同步团队片段"),
                )
                .clicked()
                {
                    service.spawn_sync_current_team();
                    action = TeamUiAction::SyncRequested;
                }
                if chrome::panel_action_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Close,
                    i18n::tr(ctx, "Sign out", "退出登录"),
                )
                .clicked()
                {
                    service.logout();
                    if let Some(audit) = audit {
                        audit.record(AuditEvent::new(
                            AuditCategory::Auth,
                            "team.logout",
                            AuditOutcome::Success,
                        ));
                    }
                    action = TeamUiAction::LoggedOut;
                }
            });

            if let Some(ts) = service.state.last_sync_unix {
                let t = chrono::DateTime::from_timestamp(ts, 0)
                    .map(|x| x.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "—".to_string());
                ui.label(chrome::rich_caption(
                    theme,
                    &format!(
                        "{}{}",
                        i18n::tr(ctx, "Last team sync: ", "最近团队同步："),
                        t
                    ),
                ));
            }
            if !service.state.last_error.is_empty() {
                ui.label(
                    chrome::rich_caption(theme, &service.state.last_error)
                        .color(theme.red_color()),
                );
            }
            if !service.status_line.is_empty() {
                ui.label(chrome::rich_caption(theme, &service.status_line));
            }
        } else {
            ui.label(
                chrome::rich_caption(
                    theme,
                    i18n::tr(
                        ctx,
                        "Sign in with Google or GitHub: complete authorization in the browser tab opened by MistTerm until you see a success page—not the mistlab.dev dashboard alone. Or use email/password below.",
                        "使用 Google 或 GitHub：请在 MistTerm 打开的浏览器标签里完成授权，直到出现「登录成功」页（仅登录网站控制台不算完成）。也可在下方用邮箱/密码登录。",
                    ),
                )
                .color(theme.color_form_hint()),
            );
            ui.add_space(theme.spacing_sm());

            let oauth_busy = service.is_busy();
            ui.horizontal(|ui| {
                if chrome::panel_action_button_ex(
                    ui,
                    theme,
                    i18n::tr(ctx, "Google", "Google 登录"),
                    !oauth_busy,
                )
                .clicked()
                {
                    service.spawn_oauth_login(OAuthProvider::Google);
                    action = TeamUiAction::LoginRequested;
                }
                if chrome::panel_action_button_ex(
                    ui,
                    theme,
                    i18n::tr(ctx, "GitHub", "GitHub 登录"),
                    !oauth_busy,
                )
                .clicked()
                {
                    service.spawn_oauth_login(OAuthProvider::Github);
                    action = TeamUiAction::LoginRequested;
                }
            });

            if oauth_busy {
                if !service.status_line.is_empty() {
                    ui.label(
                        chrome::rich_caption(theme, &service.status_line)
                            .color(theme.color_form_hint()),
                    );
                }
                ui.horizontal(|ui| {
                    if chrome::panel_action_button_ex(
                        ui,
                        theme,
                        i18n::tr(ctx, "Cancel", "取消"),
                        true,
                    )
                    .clicked()
                    {
                        service.cancel_oauth_login();
                    }
                });
            }

            ui.add_space(theme.spacing_sm());
            ui.label(
                chrome::rich_caption(
                    theme,
                    i18n::tr(ctx, "Or sign in with password", "或使用密码登录"),
                )
                .color(theme.color_form_hint()),
            );
            ui.add_space(theme.spacing_xs());

            chrome::form_checkbox_with_id(
                ui,
                theme,
                "use_email",
                &mut form.use_email,
                i18n::tr(ctx, "Sign in with email (off = username)", "使用邮箱登录（关闭则用用户名）"),
            );
            chrome::form_field_label(
                ui,
                theme,
                i18n::tr(ctx, "Account", "账号"),
            );
            chrome::form_singleline_field(
                ui,
                theme,
                ui.make_persistent_id("login_id"),
                &mut form.identifier,
                i18n::tr(ctx, "email or username", "邮箱或用户名"),
                pref_w,
                false,
            );
            chrome::form_field_label(
                ui,
                theme,
                i18n::tr(ctx, "Password", "密码"),
            );
            chrome::form_singleline_field(
                ui,
                theme,
                ui.make_persistent_id("login_pw"),
                &mut form.password,
                "",
                pref_w,
                true,
            );

            ui.horizontal(|ui| {
                if chrome::panel_action_primary_icon_button_ex(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Check,
                    i18n::tr(ctx, "Sign in", "登录"),
                    !oauth_busy,
                )
                .clicked()
                {
                    service.spawn_login(
                        form.identifier.clone(),
                        form.password.clone(),
                        form.use_email,
                    );
                    action = TeamUiAction::LoginRequested;
                }
            });

            if !service.state.last_error.is_empty() {
                ui.label(
                    chrome::rich_caption(theme, &service.state.last_error)
                        .color(theme.red_color()),
                );
            }
            if !oauth_busy && !service.status_line.is_empty() {
                ui.label(chrome::rich_caption(theme, &service.status_line));
            }

            ui.add_space(theme.spacing_xs());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_md();
                if ui
                    .link(RichText::new(i18n::tr(
                        ctx,
                        "Register on mistlab.dev",
                        "在 mistlab.dev 注册",
                    )))
                    .clicked()
                {
                    shell::open_url(team_web_register_url());
                }
                if ui
                    .link(RichText::new(i18n::tr(
                        ctx,
                        "Forgot password (web)",
                        "忘记密码（网页）",
                    )))
                    .clicked()
                {
                    shell::open_url(team_web_forgot_password_url());
                }
            });
        }

        action
    })
    .inner
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamUiAction {
    None,
    LoginRequested,
    SyncRequested,
    TeamChanged,
    LoggedOut,
    OpenMembers,
}
