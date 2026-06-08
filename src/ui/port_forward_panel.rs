//! 端口转发侧栏：本地 `-L`、远程 `-R`、动态 SOCKS `-D`。

use std::collections::{HashMap, HashSet};

use eframe::egui;

use crate::core::{
    parse_dynamic_forwards_text, parse_local_forwards_text, parse_remote_forwards_text,
    parse_forward_form, ForwardFormInput, ForwardFormKind, PortForwardKind, SessionConfig,
};
use crate::ssh::{ForwardControl, SshManager, SshSessionId};
use crate::ui::layout_util;
use crate::ui::terminal::TerminalView;
use crate::ui::theme::Theme;

pub use crate::core::PortForwardKind as ActiveForwardKind;

pub struct PortForwardSaveRequest {
    pub session_profile_id: String,
    pub kind: PortForwardKind,
}

pub struct PortForwardAuditRequest {
    pub session_profile_id: Option<String>,
    pub host: Option<String>,
    pub kind: PortForwardKind,
    pub started: bool,
}

enum ForwardEntry {
    /// 连接时按会话配置启动，停止需重连。
    Profile { label: String },
    /// 面板内运行时添加，可单独停止。
    Runtime {
        label: String,
        kind: PortForwardKind,
        control: ForwardControl,
    },
}

pub struct PortForwardPanel {
    by_ssh_session: HashMap<SshSessionId, Vec<ForwardEntry>>,
    profile_registered: HashSet<SshSessionId>,
    form_kind: ForwardFormKind,
    form: ForwardFormInput,
    save_to_session: bool,
    last_error: Option<String>,
    pending_save: Option<PortForwardSaveRequest>,
    pending_audits: Vec<PortForwardAuditRequest>,
    last_panel_slot_rect: Option<egui::Rect>,
}

impl Default for PortForwardPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl PortForwardPanel {
    pub fn new() -> Self {
        Self {
            by_ssh_session: HashMap::new(),
            profile_registered: HashSet::new(),
            form_kind: ForwardFormKind::Local,
            form: ForwardFormInput::default(),
            save_to_session: true,
            last_error: None,
            pending_save: None,
            pending_audits: Vec::new(),
            last_panel_slot_rect: None,
        }
    }

    pub fn take_pending_save(&mut self) -> Option<PortForwardSaveRequest> {
        self.pending_save.take()
    }

    pub fn take_pending_audits(&mut self) -> Vec<PortForwardAuditRequest> {
        std::mem::take(&mut self.pending_audits)
    }

    pub fn active_count_for(&self, ssh_session_id: SshSessionId) -> usize {
        self.by_ssh_session
            .get(&ssh_session_id)
            .map(|v| v.len())
            .unwrap_or(0)
    }

    pub fn has_any_active(&self) -> bool {
        self.by_ssh_session.values().any(|v| !v.is_empty())
    }

    /// 会话连接成功后登记配置中的转发（仅一次/ssh 会话）。
    pub fn register_profile_forwards(
        &mut self,
        ssh_session_id: SshSessionId,
        profile: &SessionConfig,
    ) {
        if !self.profile_registered.insert(ssh_session_id) {
            return;
        }
        let mut kinds: Vec<PortForwardKind> = parse_local_forwards_text(&profile.local_forwards_text)
            .into_iter()
            .map(PortForwardKind::Local)
            .collect();
        kinds.extend(
            parse_remote_forwards_text(&profile.remote_forwards_text)
                .into_iter()
                .map(PortForwardKind::Remote),
        );
        kinds.extend(
            parse_dynamic_forwards_text(&profile.dynamic_forwards_text)
                .into_iter()
                .map(PortForwardKind::Dynamic),
        );
        for kind in kinds {
            let label = kind.display_label();
            self.push_profile(ssh_session_id, label);
            self.pending_audits.push(PortForwardAuditRequest {
                session_profile_id: Some(profile.id.clone()),
                host: Some(profile.host.clone()),
                kind,
                started: true,
            });
        }
    }

    pub fn clear_ssh_session(&mut self, ssh_session_id: SshSessionId) {
        self.profile_registered.remove(&ssh_session_id);
        if let Some(list) = self.by_ssh_session.remove(&ssh_session_id) {
            for entry in list {
                if let ForwardEntry::Runtime { kind, control, .. } = entry {
                    control.stop();
                    self.pending_audits.push(PortForwardAuditRequest {
                        session_profile_id: None,
                        host: None,
                        kind,
                        started: false,
                    });
                }
            }
        }
    }

    pub fn show_side_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        right_dock_outer_left: &mut Option<f32>,
        dock_col_w: f32,
    ) {
        let (def_w, min_w, max_w) = layout_util::right_dock_resize_bounds(dock_col_w);
        let panel = egui::SidePanel::right("port_forward_panel")
            .default_width(def_w)
            .min_width(min_w)
            .max_width(max_w)
            .resizable(true)
            .frame(crate::ui::chrome::right_dock_placeholder_frame(theme))
            .show(ctx, |ui| {
                crate::ui::chrome::paint_right_dock_left_gap(ui, theme);
                self.last_panel_slot_rect = Some(ui.max_rect());
                let h = ui.available_height().max(1.0);
                let w = ui.available_width().max(1.0);
                ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
            });
        if let Some(slot) = self.last_panel_slot_rect {
            layout_util::record_right_dock_panel_rect(&slot, right_dock_outer_left);
        } else {
            layout_util::record_right_dock_panel(&panel.response, right_dock_outer_left);
        }
    }

    pub fn show_foreground_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        terminal: Option<&TerminalView>,
        session_profile: Option<&SessionConfig>,
        close_panel: &mut bool,
    ) {
        let Some(slot) = layout_util::right_dock_foreground_slot(
            self.last_panel_slot_rect,
            ctx,
            "port_forward_panel",
            layout_util::SidePanelProfile::Standard,
            None,
            theme.spacing_right_dock_screen_inset(),
        ) else {
            return;
        };
        let screen = ctx.screen_rect();
        let geom = crate::ui::chrome::prepare_right_dock_foreground_geom(slot, screen, theme);
        let layer_id = crate::ui::chrome::right_dock_foreground_layer_id("mistterm_port_fwd_fg");
        crate::ui::chrome::paint_right_dock_foreground_shell(ctx, layer_id, geom.paint, theme);
        crate::ui::chrome::show_right_dock_foreground_body(
            "mistterm_port_fwd_fg",
            ctx,
            &geom,
            layout_util::SidePanelProfile::Standard,
            |ui, _body_w| {
                self.show_content(ui, ctx, theme, terminal, session_profile, close_panel);
            },
        );
    }

    fn show_content(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        terminal: Option<&TerminalView>,
        session_profile: Option<&SessionConfig>,
        close_panel: &mut bool,
    ) {
        let mut header_closed = false;
        let prev_gap = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;
        theme.frame_right_dock_header_band().show(ui, |ui| {
            header_closed = crate::ui::chrome::dock_panel_title_close_only(
                ui,
                theme,
                crate::ui::icons::IconId::Network,
                crate::i18n::tr(ctx, "Port Forward", "端口转发"),
                crate::i18n::tr(
                    ctx,
                    "Hide sidebar · or use bottom toggle",
                    "隐藏侧栏 · 也可用底栏切换",
                ),
            );
        });
        if header_closed {
            *close_panel = true;
        }
        crate::ui::chrome::right_dock_header_divider(ui, theme);
        ui.spacing_mut().item_spacing.y = prev_gap;
        ui.add_space(theme.spacing_dock_section_gap());

        let Some(t) = terminal else {
            ui.label(
                egui::RichText::new(crate::i18n::tr(
                    ctx,
                    "Connect a session before using port forwarding.",
                    "请打开会话并连接后可使用端口转发。",
                ))
                .color(theme.text_tertiary()),
            );
            return;
        };
        if !t.is_connected() {
            crate::ui::chrome::busy_row(
                ui,
                theme,
                crate::i18n::tr(ctx, "Connecting…", "连接建立中…"),
            );
            return;
        }
        let Some(ssh_session_id) = t.ssh_session_id() else {
            ui.label(
                egui::RichText::new(crate::i18n::tr(ctx, "Session unavailable", "会话不可用"))
                    .color(theme.red_color()),
            );
            return;
        };
        let Some(mgr) = t.ssh_manager_clone() else {
            ui.label(
                egui::RichText::new(crate::i18n::tr(ctx, "Session unavailable", "会话不可用"))
                    .color(theme.red_color()),
            );
            return;
        };

        if let Some(profile) = session_profile {
            self.register_profile_forwards(ssh_session_id, profile);
        }
        self.prune_stopped(ssh_session_id);

        self.show_active_list(ui, ctx, theme, ssh_session_id);
        ui.add_space(theme.spacing_dock_section_gap());
        self.show_add_form(
            ui,
            ctx,
            theme,
            ssh_session_id,
            &mgr,
            session_profile,
        );

        if let Some(err) = &self.last_error {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(err).color(theme.red_color()).small());
        }
    }

    fn show_active_list(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        ssh_session_id: SshSessionId,
    ) {
        section_title(
            ui,
            theme,
            crate::i18n::tr(ctx, "Active forwards", "运行中的转发"),
        );
        ui.add_space(4.0);

        let Some(list) = self.by_ssh_session.get(&ssh_session_id) else {
            ui.label(
                egui::RichText::new(crate::i18n::tr(ctx, "None yet", "暂无"))
                    .small()
                    .color(theme.text_tertiary()),
            );
            return;
        };
        if list.is_empty() {
            ui.label(
                egui::RichText::new(crate::i18n::tr(ctx, "None yet", "暂无"))
                    .small()
                    .color(theme.text_tertiary()),
            );
            return;
        }

        let mut stop_idx: Option<usize> = None;
        for (i, entry) in list.iter().enumerate() {
            ui.horizontal(|ui| {
                let (label, stoppable) = match entry {
                    ForwardEntry::Profile { label, .. } => (label.as_str(), false),
                    ForwardEntry::Runtime { label, .. } => (label.as_str(), true),
                };
                ui.label(
                    egui::RichText::new(label)
                        .small()
                        .color(theme.text_primary()),
                );
                if !stoppable {
                    ui.label(
                        egui::RichText::new(crate::i18n::tr(ctx, "profile", "配置"))
                            .small()
                            .color(theme.text_tertiary()),
                    );
                }
                if stoppable {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(crate::i18n::tr(ctx, "Stop", "停止"))
                            .clicked()
                        {
                            stop_idx = Some(i);
                        }
                    });
                }
            });
        }

        if let Some(i) = stop_idx {
            if let Some(list) = self.by_ssh_session.get_mut(&ssh_session_id) {
                if let Some(ForwardEntry::Runtime { kind, control, .. }) = list.get(i) {
                    control.stop();
                    self.pending_audits.push(PortForwardAuditRequest {
                        session_profile_id: None,
                        host: None,
                        kind: kind.clone(),
                        started: false,
                    });
                }
                list.remove(i);
            }
        }
    }

    fn show_add_form(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        ssh_session_id: SshSessionId,
        mgr: &SshManager,
        session_profile: Option<&SessionConfig>,
    ) {
        section_title(
            ui,
            theme,
            crate::i18n::tr(ctx, "Add forward", "添加转发"),
        );
        ui.add_space(4.0);

        let prev_kind = self.form_kind;
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.form_kind,
                ForwardFormKind::Local,
                crate::i18n::tr(ctx, "Local (-L)", "本地 (-L)"),
            );
            ui.selectable_value(
                &mut self.form_kind,
                ForwardFormKind::Remote,
                crate::i18n::tr(ctx, "Remote (-R)", "远程 (-R)"),
            );
            ui.selectable_value(
                &mut self.form_kind,
                ForwardFormKind::Dynamic,
                crate::i18n::tr(ctx, "SOCKS (-D)", "SOCKS (-D)"),
            );
        });
        if self.form_kind != prev_kind {
            self.form.fill_defaults_for(self.form_kind);
        }
        ui.add_space(6.0);
        self.draw_form_fields(ui, ctx);

        ui.checkbox(
            &mut self.save_to_session,
            crate::i18n::tr(ctx, "Save to session profile", "保存到会话配置"),
        );

        if ui
            .button(crate::i18n::tr(ctx, "Start forward", "启动转发"))
            .clicked()
        {
            self.last_error = None;
            match self.start_runtime_forward(ssh_session_id, mgr, session_profile) {
                Ok(()) => {
                    self.form = ForwardFormInput::defaults_for(self.form_kind);
                }
                Err(e) => self.last_error = Some(e),
            }
        }
    }

    fn draw_form_fields(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        match self.form_kind {
            ForwardFormKind::Local | ForwardFormKind::Dynamic => {
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr(ctx, "Bind", "绑定"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.form.bind_address)
                            .desired_width(100.0)
                            .hint_text("127.0.0.1"),
                    );
                    ui.label(crate::i18n::tr(ctx, "Port", "端口"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.form.local_port)
                            .desired_width(64.0)
                            .hint_text("8080"),
                    );
                });
            }
            ForwardFormKind::Remote => {
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr(ctx, "Remote port", "远端端口"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.form.local_port)
                            .desired_width(64.0)
                            .hint_text("8080"),
                    );
                });
            }
        }

        if self.form_kind == ForwardFormKind::Local {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr(ctx, "Target", "目标"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.form.remote_host)
                        .desired_width(120.0)
                        .hint_text("127.0.0.1"),
                );
                ui.label(":");
                ui.add(
                    egui::TextEdit::singleline(&mut self.form.remote_port)
                        .desired_width(64.0)
                        .hint_text("80"),
                );
            });
        } else if self.form_kind == ForwardFormKind::Remote {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr(ctx, "Target", "目标"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.form.remote_host)
                        .desired_width(120.0)
                        .hint_text("127.0.0.1"),
                );
                ui.label(":");
                ui.add(
                    egui::TextEdit::singleline(&mut self.form.remote_port)
                        .desired_width(64.0)
                        .hint_text("3000"),
                );
            });
        }
    }

    fn start_runtime_forward(
        &mut self,
        ssh_session_id: SshSessionId,
        mgr: &SshManager,
        session_profile: Option<&SessionConfig>,
    ) -> Result<(), String> {
        self.form.fill_defaults_for(self.form_kind);
        let kind = parse_forward_form(self.form_kind, &self.form)?;
        let control = match &kind {
            PortForwardKind::Local(f) => mgr.spawn_local_forward(ssh_session_id, f.clone())?,
            PortForwardKind::Remote(f) => mgr.spawn_remote_forward(ssh_session_id, f.clone())?,
            PortForwardKind::Dynamic(f) => mgr.spawn_dynamic_forward(ssh_session_id, f.clone())?,
        };
        let label = kind.display_label();
        self.push_runtime(ssh_session_id, label, kind.clone(), control);
        self.pending_audits.push(PortForwardAuditRequest {
            session_profile_id: session_profile.map(|p| p.id.clone()),
            host: session_profile.map(|p| p.host.clone()),
            kind: kind.clone(),
            started: true,
        });
        if self.save_to_session {
            if let Some(profile) = session_profile {
                self.pending_save = Some(PortForwardSaveRequest {
                    session_profile_id: profile.id.clone(),
                    kind,
                });
            }
        }
        Ok(())
    }

    fn push_profile(&mut self, ssh_session_id: SshSessionId, label: String) {
        self.by_ssh_session
            .entry(ssh_session_id)
            .or_default()
            .push(ForwardEntry::Profile { label });
    }

    fn push_runtime(
        &mut self,
        ssh_session_id: SshSessionId,
        label: String,
        kind: PortForwardKind,
        control: ForwardControl,
    ) {
        self.by_ssh_session
            .entry(ssh_session_id)
            .or_default()
            .push(ForwardEntry::Runtime {
                label,
                kind,
                control,
            });
    }

    fn prune_stopped(&mut self, ssh_session_id: SshSessionId) {
        if let Some(list) = self.by_ssh_session.get_mut(&ssh_session_id) {
            list.retain(|entry| match entry {
                ForwardEntry::Profile { .. } => true,
                ForwardEntry::Runtime { control, .. } => !control.is_stopped(),
            });
        }
    }
}

fn section_title(ui: &mut egui::Ui, theme: &Theme, text: impl AsRef<str>) {
    ui.label(
        egui::RichText::new(text.as_ref())
            .strong()
            .color(theme.text_secondary()),
    );
}

#[cfg(test)]
mod panel_tests {
    use super::*;

    #[test]
    fn register_profile_once() {
        let mut panel = PortForwardPanel::new();
        let mut profile = SessionConfig::default();
        profile.local_forwards_text = "8080:127.0.0.1:80\n".into();
        panel.register_profile_forwards(1, &profile);
        panel.register_profile_forwards(1, &profile);
        assert_eq!(panel.active_count_for(1), 1);
        assert_eq!(panel.take_pending_audits().len(), 1);
    }

    #[test]
    fn clear_session_drops_profile_flag() {
        let mut panel = PortForwardPanel::new();
        let profile = SessionConfig::default();
        panel.register_profile_forwards(2, &profile);
        panel.clear_ssh_session(2);
        panel.register_profile_forwards(2, &profile);
        assert!(panel.profile_registered.contains(&2));
    }
}
