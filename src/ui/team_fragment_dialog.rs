//! 团队片段：创建/编辑与 409 冲突解决弹窗。

use eframe::egui;

use crate::core::team::{
    create_team_fragment_blocking, update_team_fragment_blocking,
    TeamFragment, TeamService,
};
use crate::core::{AuditCategory, AuditEvent, AuditOutcome, AuditLogger};
use crate::i18n;
use crate::ui::chrome;
use crate::ui::layout_util;
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Default)]
pub struct TeamFragmentEditorState {
    pub open: bool,
    /// `None` = 新建
    pub editing: Option<TeamFragment>,
    pub title: String,
    pub command: String,
    pub category: String,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct TeamFragmentConflictState {
    pub local: TeamFragment,
    pub server: TeamFragment,
    pub pending_title: String,
    pub pending_command: String,
    pub error: String,
}

pub fn open_create_editor(editor: &mut TeamFragmentEditorState) {
    editor.open = true;
    editor.editing = None;
    editor.title.clear();
    editor.command.clear();
    editor.category.clear();
    editor.error.clear();
}

fn modal_header_title(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    chrome::modal_header_title_only(ui, theme, title, chrome::modal_title_font_size(theme));
}

pub fn open_edit_editor(editor: &mut TeamFragmentEditorState, frag: &TeamFragment) {
    editor.open = true;
    editor.editing = Some(frag.clone());
    editor.title = frag.title.clone();
    editor.command = frag.command.clone();
    editor.category = frag.category.clone();
    editor.error.clear();
}

pub fn show_team_fragment_editor_modal(
    ctx: &egui::Context,
    theme: &Theme,
    service: &mut TeamService,
    editor: &mut TeamFragmentEditorState,
    conflict: &mut Option<TeamFragmentConflictState>,
    audit: &AuditLogger,
) {
    if !editor.open {
        return;
    }
    let mut open = editor.open;
    let mut should_close = false;
    let title = if editor.editing.is_some() {
        i18n::tr(ctx, "Edit team snippet", "编辑团队片段")
    } else {
        i18n::tr(ctx, "New team snippet", "新建团队片段")
    };

    let modal_sz = layout_util::modal_edit_size(ctx);
    crate::ui::chrome::modal_window("team_fragment_editor", theme, ctx)
        .open(&mut open)
        .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
        .movable(true)
        .resizable(false)
        .fixed_size(modal_sz)
        .show(ctx, |ui| {
            crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                ui.push_id("team_fragment_editor_form", |ui| {
                    modal_header_title(ui, theme, title);

                    let form_w = layout_util::finite_content_width_inset(ui, 4.0, 300.0, 460.0);
                    ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);

                    chrome::form_field_label(ui, theme, i18n::tr(ctx, "Title", "标题"));
                    chrome::form_singleline_field(
                        ui,
                        theme,
                        egui::Id::new("team_frag_title"),
                        &mut editor.title,
                        "",
                        form_w,
                        false,
                    );
                    chrome::form_field_label(ui, theme, i18n::tr(ctx, "Command", "命令"));
                    chrome::form_multiline_field(
                        ui,
                        theme,
                        egui::Id::new("team_frag_command"),
                        &mut editor.command,
                        form_w,
                        4,
                        false,
                    );
                    chrome::form_field_label(ui, theme, i18n::tr(ctx, "Category", "分类"));
                    chrome::form_singleline_field(
                        ui,
                        theme,
                        egui::Id::new("team_frag_cat"),
                        &mut editor.category,
                        "",
                        form_w,
                        false,
                    );

                    if !editor.error.is_empty() {
                        ui.label(
                            chrome::rich_caption(theme, &editor.error).color(theme.red_color()),
                        );
                    }

                    ui.add_space(theme.spacing_sm());
                    crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                        if crate::ui::chrome::modal_secondary_icon_button(
                            ui,
                            th,
                            crate::ui::icons::IconId::Close,
                            i18n::tr(ctx, "Cancel", "取消"),
                        )
                        .clicked()
                        {
                            should_close = true;
                        }
                        if crate::ui::chrome::modal_primary_icon_button(
                            ui,
                            th,
                            crate::ui::icons::IconId::Check,
                            i18n::tr(ctx, "Save", "保存"),
                        )
                        .clicked()
                        {
                            let title = editor.title.trim().to_string();
                            let command = editor.command.trim().to_string();
                            if title.is_empty() || command.is_empty() {
                                editor.error = i18n::tr(
                                    ctx,
                                    "Title and command are required",
                                    "标题与命令不能为空",
                                )
                                .to_string();
                                return;
                            }
                            let cat = editor.category.trim();
                            let cat_opt = if cat.is_empty() {
                                None
                            } else {
                                Some(cat)
                            };
                            if let Some(ref existing) = editor.editing {
                                match update_team_fragment_blocking(
                                    service,
                                    existing,
                                    &title,
                                    &command,
                                ) {
                                    Ok(updated) => {
                                        audit.record(
                                            AuditEvent::new(
                                                AuditCategory::Fragment,
                                                "fragment.update",
                                                AuditOutcome::Success,
                                            )
                                            .with_resource(&updated.id),
                                        );
                                        should_close = true;
                                    }
                                    Err(e) if e.status == 409 => {
                                        if let Some(server) = e.conflict_fragment {
                                            editor.error.clear();
                                            should_close = true;
                                            *conflict = Some(TeamFragmentConflictState {
                                                local: existing.clone(),
                                                server,
                                                pending_title: title,
                                                pending_command: command,
                                                error: String::new(),
                                            });
                                        } else {
                                            editor.error = e.message;
                                        }
                                    }
                                    Err(e) => {
                                        editor.error = e.message;
                                        if e.status == 401 {
                                            service.logout();
                                        }
                                    }
                                }
                            } else {
                                match create_team_fragment_blocking(
                                    service,
                                    &title,
                                    &command,
                                    cat_opt,
                                ) {
                                    Ok(created) => {
                                        audit.record(
                                            AuditEvent::new(
                                                AuditCategory::Fragment,
                                                "fragment.create",
                                                AuditOutcome::Success,
                                            )
                                            .with_resource(&created.id),
                                        );
                                        should_close = true;
                                    }
                                    Err(e) => {
                                        editor.error = e;
                                        if editor.error.contains("401") {
                                            service.logout();
                                        }
                                    }
                                }
                            }
                        }
                    });
                });
            });
        });
    editor.open = open && !should_close;
}

pub fn show_team_fragment_conflict_modal(
    ctx: &egui::Context,
    theme: &Theme,
    service: &mut TeamService,
    conflict: &mut Option<TeamFragmentConflictState>,
    audit: &AuditLogger,
) {
    let Some(state) = conflict.as_mut() else {
        return;
    };
    let mut open = true;
    let mut should_close = false;

    let modal_sz = layout_util::modal_edit_size(ctx);
    crate::ui::chrome::modal_window("team_fragment_conflict", theme, ctx)
        .open(&mut open)
        .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
        .movable(true)
        .resizable(false)
        .fixed_size(modal_sz)
        .show(ctx, |ui| {
            crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                ui.push_id("team_fragment_conflict_form", |ui| {
                    modal_header_title(
                        ui,
                        theme,
                        i18n::tr(ctx, "Revision conflict", "版本冲突"),
                    );
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                    ui.label(i18n::tr(
                        ctx,
                        "Someone else updated this snippet. Choose how to proceed.",
                        "该片段已在服务端被他人更新，请选择处理方式。",
                    ));
                    ui.add_space(theme.spacing_sm());
                    ui.label(
                        chrome::rich_caption(theme, i18n::tr(ctx, "Server version", "服务端版本"))
                            .strong(),
                    );
                    ui.monospace(format!(
                        "{}: {}",
                        state.server.title, state.server.command
                    ));
                    ui.add_space(theme.spacing_xs());
                    ui.label(
                        chrome::rich_caption(theme, i18n::tr(ctx, "Your edit", "你的编辑"))
                            .strong(),
                    );
                    ui.monospace(format!(
                        "{}: {}",
                        state.pending_title, state.pending_command
                    ));
                    if !state.error.is_empty() {
                        ui.label(
                            chrome::rich_caption(theme, &state.error).color(theme.red_color()),
                        );
                    }
                    ui.add_space(theme.spacing_sm());
                    crate::ui::chrome::modal_footer_actions(ui, theme, |ui, _th| {
                        if ui.button(i18n::tr(ctx, "Cancel", "取消")).clicked() {
                            should_close = true;
                        }
                        if ui.button(i18n::tr(ctx, "Merge", "合并")).clicked() {
                            let mut base = state.server.clone();
                            base.title = state.pending_title.clone();
                            base.command = format!(
                                "{}\n# --- merged ---\n{}",
                                state.server.command.trim_end(),
                                state.pending_command.trim()
                            );
                            match apply_conflict_resolution(service, &base, audit) {
                                Ok(()) => should_close = true,
                                Err(e) => state.error = e,
                            }
                        }
                        if ui
                            .button(i18n::tr(ctx, "Keep mine", "保留本地编辑"))
                            .clicked()
                        {
                            let mut base = state.server.clone();
                            base.title = state.pending_title.clone();
                            base.command = state.pending_command.clone();
                            match apply_conflict_resolution(service, &base, audit) {
                                Ok(()) => should_close = true,
                                Err(e) => state.error = e,
                            }
                        }
                        if ui
                            .button(i18n::tr(ctx, "Use server", "以服务端为准"))
                            .clicked()
                        {
                            let base = state.server.clone();
                            match apply_conflict_resolution(service, &base, audit) {
                                Ok(()) => should_close = true,
                                Err(e) => state.error = e,
                            }
                        }
                    });
                });
            });
        });
    if should_close || !open {
        *conflict = None;
    }
}

fn apply_conflict_resolution(
    service: &mut TeamService,
    base: &TeamFragment,
    audit: &AuditLogger,
) -> Result<(), String> {
    let updated =
        update_team_fragment_blocking(service, base, &base.title, &base.command)
            .map_err(|e| e.message)?;
    audit.record(
        AuditEvent::new(
            AuditCategory::Fragment,
            "fragment.update",
            AuditOutcome::Success,
        )
        .with_resource(&updated.id)
        .with_detail(serde_json::json!({ "conflict_resolved": true })),
    );
    Ok(())
}
