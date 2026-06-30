//! 团队片段附加功能弹窗：版本历史、外部分享、团队设置。

use eframe::egui;

use crate::core::team::{
    create_fragment_share_blocking, delete_fragment_share_blocking,
    fetch_fragment_versions_blocking, fetch_team_settings_blocking,
    list_fragment_shares_blocking, update_team_fragment_blocking, update_team_settings_blocking,
    ExternalShare, FragmentVersion, TeamServerSettings, TeamService,
};
use crate::core::{AuditCategory, AuditEvent, AuditLogger, AuditOutcome};
use crate::i18n;
use crate::ui::chrome;
use crate::ui::layout_util;
use crate::ui::theme::Theme;

fn modal_header_title(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    chrome::modal_header_title_only(ui, theme, title, chrome::modal_title_font_size(theme));
}

// ───────────────────────── 版本历史 ─────────────────────────

#[derive(Debug, Clone, Default)]
pub struct FragmentVersionsState {
    pub open: bool,
    pub fragment_id: String,
    pub fragment_title: String,
    pub versions: Vec<FragmentVersion>,
    pub loaded: bool,
    pub error: String,
}

pub fn open_versions(state: &mut FragmentVersionsState, fragment_id: &str, title: &str) {
    state.open = true;
    state.fragment_id = fragment_id.to_string();
    state.fragment_title = title.to_string();
    state.versions.clear();
    state.loaded = false;
    state.error.clear();
}

pub fn show_fragment_versions_modal(
    ctx: &egui::Context,
    theme: &Theme,
    service: &mut TeamService,
    state: &mut FragmentVersionsState,
    audit: &AuditLogger,
) {
    if !state.open {
        return;
    }
    if !state.loaded {
        match fetch_fragment_versions_blocking(service, &state.fragment_id) {
            Ok(v) => {
                state.versions = v;
                state.error.clear();
            }
            Err(e) => state.error = e,
        }
        state.loaded = true;
    }

    let mut open = state.open;
    let mut should_close = false;
    let can_edit = service.state.current_role().can_edit();
    let mut restore_revision: Option<i64> = None;

    let modal_sz = layout_util::modal_edit_size(ctx);
    chrome::modal_window("team_fragment_versions", theme, ctx)
        .open(&mut open)
        .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
        .movable(true)
        .resizable(false)
        .fixed_size(modal_sz)
        .show(ctx, |ui| {
            chrome::modal_content_frame(theme).show(ui, |ui| {
                ui.push_id("team_fragment_versions_form", |ui| {
                    modal_header_title(
                        ui,
                        theme,
                        i18n::tr(ctx, "Version history", "版本历史"),
                    );
                    ui.label(
                        chrome::rich_caption(theme, &state.fragment_title).weak(),
                    );
                    ui.add_space(theme.spacing_sm());

                    if !state.error.is_empty() {
                        ui.label(
                            chrome::rich_caption(theme, &state.error).color(theme.red_color()),
                        );
                    } else if state.versions.is_empty() {
                        ui.label(chrome::rich_caption(
                            theme,
                            i18n::tr(ctx, "No version history yet", "暂无版本历史"),
                        ));
                    }

                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for v in &state.versions {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        chrome::rich_caption(
                                            theme,
                                            &format!("r{}", v.revision),
                                        )
                                        .strong(),
                                    );
                                    let meta = format!(
                                        "{} · {}",
                                        v.created_at.clone().unwrap_or_default(),
                                        v.updated_by,
                                    );
                                    ui.label(chrome::rich_caption(theme, &meta).weak());
                                });
                                ui.label(chrome::rich_caption(theme, &v.title));
                                ui.monospace(&v.command);
                                ui.horizontal(|ui| {
                                    if ui
                                        .button(i18n::tr(ctx, "Copy command", "复制命令"))
                                        .clicked()
                                    {
                                        ctx.copy_text(v.command.clone());
                                    }
                                    if can_edit
                                        && ui
                                            .button(i18n::tr(ctx, "Restore", "恢复此版本"))
                                            .clicked()
                                    {
                                        restore_revision = Some(v.revision);
                                    }
                                });
                                ui.separator();
                            }
                        });

                    ui.add_space(theme.spacing_sm());
                    chrome::modal_footer_actions(ui, theme, |ui, th| {
                        if chrome::modal_secondary_icon_button(
                            ui,
                            th,
                            crate::ui::icons::IconId::Close,
                            i18n::tr(ctx, "Close", "关闭"),
                        )
                        .clicked()
                        {
                            should_close = true;
                        }
                    });
                });
            });
        });

    if let Some(rev) = restore_revision {
        if let Some(version) = state.versions.iter().find(|v| v.revision == rev).cloned() {
            if let Some(current) = service.find_team_fragment(&state.fragment_id) {
                match update_team_fragment_blocking(
                    service,
                    &current,
                    &version.title,
                    &version.command,
                    None,
                ) {
                    Ok(updated) => {
                        audit.record(
                            AuditEvent::new(
                                AuditCategory::Fragment,
                                "fragment.restore_version",
                                AuditOutcome::Success,
                            )
                            .with_resource(&updated.id)
                            .with_detail(serde_json::json!({ "revision": rev })),
                        );
                        state.error = i18n::tr(
                            ctx,
                            "Version restored",
                            "已恢复到所选版本",
                        )
                        .to_string();
                        state.loaded = false;
                    }
                    Err(e) => state.error = e.message,
                }
            }
        }
    }

    state.open = open && !should_close;
}

// ───────────────────────── 外部分享 ─────────────────────────

#[derive(Debug, Clone)]
pub struct FragmentSharesState {
    pub open: bool,
    pub fragment_id: String,
    pub fragment_title: String,
    pub shares: Vec<ExternalShare>,
    pub loaded: bool,
    pub error: String,
    pub expires_hours_str: String,
    pub last_share_url: String,
}

impl Default for FragmentSharesState {
    fn default() -> Self {
        Self {
            open: false,
            fragment_id: String::new(),
            fragment_title: String::new(),
            shares: Vec::new(),
            loaded: false,
            error: String::new(),
            expires_hours_str: "24".to_string(),
            last_share_url: String::new(),
        }
    }
}

pub fn open_shares(state: &mut FragmentSharesState, fragment_id: &str, title: &str) {
    state.open = true;
    state.fragment_id = fragment_id.to_string();
    state.fragment_title = title.to_string();
    state.shares.clear();
    state.loaded = false;
    state.error.clear();
    state.last_share_url.clear();
}

pub fn show_fragment_shares_modal(
    ctx: &egui::Context,
    theme: &Theme,
    service: &mut TeamService,
    state: &mut FragmentSharesState,
    audit: &AuditLogger,
) {
    if !state.open {
        return;
    }
    if !state.loaded {
        match list_fragment_shares_blocking(service, &state.fragment_id) {
            Ok(s) => {
                state.shares = s;
                state.error.clear();
            }
            Err(e) => state.error = e,
        }
        state.loaded = true;
    }

    let mut open = state.open;
    let mut should_close = false;
    let can_edit = service.state.current_role().can_edit();
    let mut create_share = false;
    let mut revoke_share: Option<String> = None;

    let modal_sz = layout_util::modal_edit_size(ctx);
    chrome::modal_window("team_fragment_shares", theme, ctx)
        .open(&mut open)
        .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
        .movable(true)
        .resizable(false)
        .fixed_size(modal_sz)
        .show(ctx, |ui| {
            chrome::modal_content_frame(theme).show(ui, |ui| {
                ui.push_id("team_fragment_shares_form", |ui| {
                    modal_header_title(
                        ui,
                        theme,
                        i18n::tr(ctx, "External shares", "外部分享"),
                    );
                    ui.label(chrome::rich_caption(theme, &state.fragment_title).weak());
                    ui.add_space(theme.spacing_sm());

                    if can_edit {
                        ui.horizontal(|ui| {
                            ui.label(chrome::rich_caption(
                                theme,
                                i18n::tr(ctx, "Expires in (hours, 0 = never)", "有效期（小时，0 = 永久）"),
                            ));
                            ui.add(
                                egui::TextEdit::singleline(&mut state.expires_hours_str)
                                    .desired_width(60.0),
                            );
                            if chrome::modal_primary_button_with_icon(
                                ui,
                                theme,
                                crate::ui::icons::IconId::Plus,
                                i18n::tr(ctx, "Create link", "生成链接"),
                            )
                            .clicked()
                            {
                                create_share = true;
                            }
                        });
                        ui.add_space(theme.spacing_xs());
                    }

                    if !state.last_share_url.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(
                                chrome::rich_caption(
                                    theme,
                                    i18n::tr(ctx, "New link:", "新链接："),
                                )
                                .strong(),
                            );
                            if ui.button(i18n::tr(ctx, "Copy URL", "复制链接")).clicked() {
                                ctx.copy_text(state.last_share_url.clone());
                            }
                        });
                        ui.monospace(&state.last_share_url);
                        ui.add_space(theme.spacing_xs());
                    }

                    if !state.error.is_empty() {
                        ui.label(
                            chrome::rich_caption(theme, &state.error).color(theme.red_color()),
                        );
                    } else if state.shares.is_empty() {
                        ui.label(chrome::rich_caption(
                            theme,
                            i18n::tr(ctx, "No active share links", "暂无分享链接"),
                        ));
                    }

                    egui::ScrollArea::vertical()
                        .max_height(240.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for s in &state.shares {
                                ui.horizontal(|ui| {
                                    ui.monospace(&s.share_token);
                                    let meta = format!(
                                        "{} {} · {}",
                                        i18n::tr(ctx, "views:", "浏览："),
                                        s.view_count,
                                        s.expires_at
                                            .clone()
                                            .unwrap_or_else(|| {
                                                i18n::tr(ctx, "never", "永久").to_string()
                                            }),
                                    );
                                    ui.label(chrome::rich_caption(theme, &meta).weak());
                                });
                                ui.horizontal(|ui| {
                                    if can_edit
                                        && ui
                                            .button(i18n::tr(ctx, "Revoke", "撤销"))
                                            .clicked()
                                    {
                                        revoke_share = Some(s.id.clone());
                                    }
                                });
                                ui.separator();
                            }
                        });

                    ui.add_space(theme.spacing_sm());
                    chrome::modal_footer_actions(ui, theme, |ui, th| {
                        if chrome::modal_secondary_icon_button(
                            ui,
                            th,
                            crate::ui::icons::IconId::Close,
                            i18n::tr(ctx, "Close", "关闭"),
                        )
                        .clicked()
                        {
                            should_close = true;
                        }
                    });
                });
            });
        });

    if create_share {
        let hours: i64 = state.expires_hours_str.trim().parse().unwrap_or(0);
        match create_fragment_share_blocking(service, &state.fragment_id, hours) {
            Ok(resp) => {
                state.last_share_url = resp.share_url;
                state.error.clear();
                state.loaded = false;
                audit.record(
                    AuditEvent::new(
                        AuditCategory::Fragment,
                        "fragment.share_create",
                        AuditOutcome::Success,
                    )
                    .with_resource(&state.fragment_id),
                );
            }
            Err(e) => state.error = e,
        }
    }
    if let Some(share_id) = revoke_share {
        match delete_fragment_share_blocking(service, &share_id) {
            Ok(()) => {
                state.loaded = false;
                state.error.clear();
                audit.record(
                    AuditEvent::new(
                        AuditCategory::Fragment,
                        "fragment.share_revoke",
                        AuditOutcome::Success,
                    )
                    .with_resource(&share_id),
                );
            }
            Err(e) => state.error = e,
        }
    }

    state.open = open && !should_close;
}

// ───────────────────────── 团队设置 ─────────────────────────

#[derive(Debug, Clone, Default)]
pub struct TeamSettingsState {
    pub open: bool,
    pub loaded: bool,
    pub error: String,
    pub audit_retention_days_str: String,
    pub allow_guest_access: bool,
    pub require_mfa: bool,
}

pub fn open_team_settings(state: &mut TeamSettingsState) {
    state.open = true;
    state.loaded = false;
    state.error.clear();
}

pub fn show_team_settings_modal(
    ctx: &egui::Context,
    theme: &Theme,
    service: &mut TeamService,
    state: &mut TeamSettingsState,
) {
    if !state.open {
        return;
    }
    if !state.loaded {
        match fetch_team_settings_blocking(service) {
            Ok(s) => {
                state.audit_retention_days_str = s.audit_retention_days.to_string();
                state.allow_guest_access = s.allow_guest_access;
                state.require_mfa = s.require_mfa;
                state.error.clear();
            }
            Err(e) => state.error = e,
        }
        state.loaded = true;
    }

    let mut open = state.open;
    let mut should_close = false;
    let is_admin = service.state.current_role().can_delete();
    let mut save_now = false;

    let modal_sz = layout_util::modal_edit_size(ctx);
    chrome::modal_window("team_settings", theme, ctx)
        .open(&mut open)
        .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
        .movable(true)
        .resizable(false)
        .fixed_size(modal_sz)
        .show(ctx, |ui| {
            chrome::modal_content_frame(theme).show(ui, |ui| {
                ui.push_id("team_settings_form", |ui| {
                    modal_header_title(ui, theme, i18n::tr(ctx, "Team settings", "团队设置"));
                    ui.add_space(theme.spacing_sm());

                    ui.add_enabled_ui(is_admin, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(chrome::rich_caption(
                                theme,
                                i18n::tr(ctx, "Audit retention (days)", "审计保留（天）"),
                            ));
                            ui.add(
                                egui::TextEdit::singleline(&mut state.audit_retention_days_str)
                                    .desired_width(80.0),
                            );
                        });
                        ui.checkbox(
                            &mut state.allow_guest_access,
                            i18n::tr(ctx, "Allow guest access", "允许访客访问"),
                        );
                        ui.checkbox(
                            &mut state.require_mfa,
                            i18n::tr(ctx, "Require MFA", "强制多因素认证"),
                        );
                    });

                    if !is_admin {
                        ui.add_space(theme.spacing_xs());
                        ui.label(chrome::rich_caption(
                            theme,
                            i18n::tr(ctx, "Admin role required to edit", "仅管理员可修改"),
                        ));
                    }

                    if !state.error.is_empty() {
                        ui.add_space(theme.spacing_xs());
                        ui.label(
                            chrome::rich_caption(theme, &state.error).color(theme.red_color()),
                        );
                    }

                    ui.add_space(theme.spacing_sm());
                    chrome::modal_footer_actions(ui, theme, |ui, th| {
                        if chrome::modal_secondary_icon_button(
                            ui,
                            th,
                            crate::ui::icons::IconId::Close,
                            i18n::tr(ctx, "Close", "关闭"),
                        )
                        .clicked()
                        {
                            should_close = true;
                        }
                        if is_admin
                            && chrome::modal_primary_button_with_icon(
                                ui,
                                th,
                                crate::ui::icons::IconId::Check,
                                i18n::tr(ctx, "Save", "保存"),
                            )
                            .clicked()
                        {
                            save_now = true;
                        }
                    });
                });
            });
        });

    if save_now {
        let days: i64 = state.audit_retention_days_str.trim().parse().unwrap_or(0);
        let settings = TeamServerSettings {
            audit_retention_days: days,
            allow_guest_access: state.allow_guest_access,
            require_mfa: state.require_mfa,
        };
        match update_team_settings_blocking(service, &settings) {
            Ok(saved) => {
                state.audit_retention_days_str = saved.audit_retention_days.to_string();
                state.allow_guest_access = saved.allow_guest_access;
                state.require_mfa = saved.require_mfa;
                state.error = i18n::tr(ctx, "Settings saved", "设置已保存").to_string();
            }
            Err(e) => state.error = e,
        }
    }

    state.open = open && !should_close;
}
