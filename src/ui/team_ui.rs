//! 团队平台 UI 片段(偏好设置 / 云端同步面板复用)。

use eframe::egui;

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
/// `id_scope` 须在每处调用点唯一(如 `"preferences_team"` / `"cloud_sync_team"`)，
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
                let selected = teams
                    .iter()
                    .find(|m| m.team.id == current)
                    .map(|m| m.team.name.as_str())
                    .unwrap_or("—");
                chrome::form_combo(
                    ui,
                    theme,
                    "current_team",
                    selected,
                    pref_w.min(ui.available_width()),
                    |ui| {
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
                    },
                );
            }

            ui.horizontal(|ui| {
                if chrome::panel_action_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Server,
                    i18n::tr(ctx, "Members", "成员"),
                )
                .clicked()
                {
                    action = TeamUiAction::OpenMembers;
                }
                if chrome::panel_action_primary_icon_button(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Cloud,
                    i18n::tr(ctx, "Sync", "同步"),
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
                    i18n::tr(ctx, "Sign out", "退出"),
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
            // #279：授权说明改步骤式（1/2/3），避免一整段说明。
            for line in [
                i18n::tr(
                    ctx,
                    "1. Choose Google or GitHub below.",
                    "1. 在下方选择 Google 或 GitHub。",
                ),
                i18n::tr(
                    ctx,
                    "2. Finish the authorization in the browser tab MistTerm opens, until you see the success page.",
                    "2. 在 MistTerm 打开的浏览器页完成授权，直到看见「登录成功」页面。",
                ),
                i18n::tr(
                    ctx,
                    "3. Or sign in with email/password directly in the form below.",
                    "3. 也可以直接用下方的邮箱/密码登录。",
                ),
            ] {
                ui.label(
                    chrome::rich_caption(theme, line).color(theme.color_form_hint()),
                );
            }
            ui.add_space(theme.spacing_xs());

            let oauth_busy = service.is_busy();
            // 两枚 OAuth 按钮通栏等分。
            let gap = theme.spacing_sm();
            let half = ((pref_w.min(ui.available_width()) - gap) * 0.5).max(72.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                // #279：第三方登录按钮带品牌图标（复用 icons.rs 既有图标，不引新依赖）。
                if chrome::panel_action_button_with_icon_ex_min_w(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Search,
                    i18n::tr(ctx, "Google", "Google"),
                    !oauth_busy,
                    half,
                )
                .clicked()
                {
                    service.spawn_oauth_login(OAuthProvider::Google);
                    action = TeamUiAction::LoginRequested;
                }
                if chrome::panel_action_button_with_icon_ex_min_w(
                    ui,
                    theme,
                    crate::ui::icons::IconId::GitBranch,
                    i18n::tr(ctx, "GitHub", "GitHub"),
                    !oauth_busy,
                    half,
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

            chrome::settings_row_divider(ui, theme);
            ui.add(
                egui::Label::new(
                    chrome::rich_caption(
                        theme,
                        i18n::tr(ctx, "Or sign in with password", "或使用密码登录"),
                    )
                    .color(theme.color_form_hint()),
                )
                .wrap(true),
            );
            ui.add_space(theme.spacing_xs());

            // #279：账号类型改下拉（去掉反向复选框「关闭则用用户名」）。
            chrome::form_field_label(
                ui,
                theme,
                i18n::tr(ctx, "Account type", "账号类型"),
            );
            let mut use_email = form.use_email;
            let kind_text = if use_email {
                i18n::tr(ctx, "Email", "邮箱")
            } else {
                i18n::tr(ctx, "Username", "用户名")
            };
            chrome::form_combo(
                ui,
                theme,
                "login_account_kind",
                kind_text,
                pref_w.min(ui.available_width()),
                |ui| {
                    ui.selectable_value(&mut use_email, true, i18n::tr(ctx, "Email", "邮箱"));
                    ui.selectable_value(&mut use_email, false, i18n::tr(ctx, "Username", "用户名"));
                },
            );
            form.use_email = use_email;
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

            // #279：「保持登录状态」底层没有单独字段：团队会话本来就由加密的
            // team_state.json + 系统钥匙串令牌持久化，这里只做 UI 态（默认勾选），
            // 不影响登录请求；如日后需要「退出后清除令牌」再补底层字段。
            let keep_id = ui.make_persistent_id("team_keep_signed_in");
            let mut keep_signed_in = ui
                .data_mut(|d| d.get_persisted::<bool>(keep_id))
                .unwrap_or(true);
            if ui
                .checkbox(
                    &mut keep_signed_in,
                    i18n::tr(ctx, "Stay signed in on this computer", "保持登录状态（这台电脑）"),
                )
                .changed()
            {
                ui.data_mut(|d| d.insert_persisted(keep_id, keep_signed_in));
            }

            // #279：登录按钮不再通栏。
            if chrome::panel_action_primary_button_with_icon_ex(
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

            if !service.state.last_error.is_empty() {
                ui.label(
                    chrome::rich_caption(theme, &service.state.last_error)
                        .color(theme.red_color()),
                );
            }
            if !oauth_busy && !service.status_line.is_empty() {
                ui.label(chrome::rich_caption(theme, &service.status_line));
            }

            ui.add_space(theme.spacing_sm());
            // 勿用 with_layout(Align::BOTTOM)：会吃掉滚动区剩余高度，登录钮下方出现大块空白。
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_md();
                if chrome::panel_caption_link_with_hover(
                    ui,
                    theme,
                    i18n::tr(ctx, "Register", "去网站注册"),
                    Some("mistlab.dev/register"),
                )
                .clicked()
                {
                    shell::open_url(team_web_register_url());
                }
                if chrome::panel_caption_link_with_hover(
                    ui,
                    theme,
                    i18n::tr(ctx, "Forgot password", "忘记密码"),
                    Some("mistlab.dev/forgot-password"),
                )
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
