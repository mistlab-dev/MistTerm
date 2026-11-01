//! 端口转发侧栏：本地 `-L`、远程 `-R`、动态 SOCKS `-D`。

use std::collections::{HashMap, HashSet};

use eframe::egui;

use crate::core::{
    parse_forward_form, ForwardFormInput, ForwardFormKind, PortForwardKind, SessionConfig,
};
use crate::ssh::{ForwardControl, ProfileForward, SshManager, SshSessionId};
use crate::ui::layout_util;
use crate::ui::terminal::TerminalView;
use crate::ui::theme::Theme;

pub use crate::core::PortForwardKind as ActiveForwardKind;

pub struct PortForwardSaveRequest {
    pub session_profile_id: String,
    pub kind: PortForwardKind,
}

/// 删除转发配置：从 profile 的转发文本里删掉该行（#268 Delete；承接 #91 重连复活问题）。
/// 「停止」只关监听，不发本请求。
pub struct PortForwardRemoveRequest {
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
    /// 连接时按会话配置启动（带控制柄，可停止/删除，#268）。
    Profile {
        label: String,
        kind: PortForwardKind,
        control: ForwardControl,
    },
    /// 面板内运行时添加。
    Runtime {
        label: String,
        kind: PortForwardKind,
        control: ForwardControl,
    },
}

impl ForwardEntry {
    fn label(&self) -> &str {
        match self {
            Self::Profile { label, .. } | Self::Runtime { label, .. } => label.as_str(),
        }
    }

    fn kind(&self) -> &PortForwardKind {
        match self {
            Self::Profile { kind, .. } | Self::Runtime { kind, .. } => kind,
        }
    }

    fn control(&self) -> &ForwardControl {
        match self {
            Self::Profile { control, .. } | Self::Runtime { control, .. } => control,
        }
    }

    fn from_profile(&self) -> bool {
        matches!(self, Self::Profile { .. })
    }
}

pub struct PortForwardPanel {
    by_ssh_session: HashMap<SshSessionId, Vec<ForwardEntry>>,
    profile_registered: HashSet<SshSessionId>,
    form_kind: ForwardFormKind,
    form: ForwardFormInput,
    save_to_session: bool,
    last_error: Option<String>,
    pending_save: Option<PortForwardSaveRequest>,
    pending_remove: Option<PortForwardRemoveRequest>,
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
            pending_remove: None,
            pending_audits: Vec::new(),
            last_panel_slot_rect: None,
        }
    }

    pub fn take_pending_save(&mut self) -> Option<PortForwardSaveRequest> {
        self.pending_save.take()
    }

    pub fn take_pending_remove(&mut self) -> Option<PortForwardRemoveRequest> {
        self.pending_remove.take()
    }

    pub fn take_pending_audits(&mut self) -> Vec<PortForwardAuditRequest> {
        std::mem::take(&mut self.pending_audits)
    }

    /// 当前仍运行的转发数(不含已停止的运行时转发)。
    pub fn active_count_for(&self, ssh_session_id: SshSessionId) -> usize {
        self.by_ssh_session
            .get(&ssh_session_id)
            .map(|list| {
                list.iter()
                    .filter(|entry| !entry.control().is_stopped())
                    .count()
            })
            .unwrap_or(0)
    }

    /// 移除已无对应 SSH 连接的会话条目(断开时 `ssh_session_id` 可能已不可用)。
    pub fn retain_live_sessions(&mut self, live: &std::collections::HashSet<SshSessionId>) {
        let stale: Vec<SshSessionId> = self
            .by_ssh_session
            .keys()
            .copied()
            .filter(|id| !live.contains(id))
            .collect();
        for id in stale {
            self.clear_ssh_session(id);
        }
    }

    /// 每帧：登记会话配置转发并剔除已停止的转发。
    pub fn tick_active_session(
        &mut self,
        ssh_session_id: SshSessionId,
        profile: &SessionConfig,
        mgr: &SshManager,
    ) {
        self.register_profile_forwards(ssh_session_id, profile, mgr);
        self.prune_stopped(ssh_session_id);
    }

    /// 会话连接成功后登记配置中的转发(仅一次/ssh 会话)。
    /// 控制柄来自 `SshManager::take_profile_forwards`（连接时保留，#268）。
    /// 会话尚未进入管理器时不置位，下一帧再试。
    pub fn register_profile_forwards(
        &mut self,
        ssh_session_id: SshSessionId,
        profile: &SessionConfig,
        mgr: &SshManager,
    ) {
        if self.profile_registered.contains(&ssh_session_id) {
            return;
        }
        let Some(spawned) = mgr.take_profile_forwards(ssh_session_id) else {
            return;
        };
        self.profile_registered.insert(ssh_session_id);
        for pf in spawned {
            let (kind, control) = match pf {
                ProfileForward::Local { fwd, control } => (PortForwardKind::Local(fwd), control),
                ProfileForward::Remote { fwd, control } => (PortForwardKind::Remote(fwd), control),
                ProfileForward::Dynamic { fwd, control } => {
                    (PortForwardKind::Dynamic(fwd), control)
                }
            };
            let label = kind.display_label();
            self.push_profile(ssh_session_id, label, kind.clone(), control);
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
                let kind = entry.kind().clone();
                entry.control().stop();
                self.pending_audits.push(PortForwardAuditRequest {
                    session_profile_id: None,
                    host: None,
                    kind,
                    started: false,
                });
            }
        }
    }

    #[inline]
    pub(crate) fn last_panel_slot_rect(&self) -> Option<egui::Rect> {
        self.last_panel_slot_rect
    }

    pub fn show_side_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        right_dock_outer_left: &mut Option<f32>,
        dock_col_w: f32,
    ) {
        let (def_w, min_w, max_w) = layout_util::right_dock_resize_bounds(dock_col_w);
        ctx.root_ui(|root| {
            egui::Panel::right("port_forward_panel")
                .default_size(def_w)
                .min_size(min_w)
                .max_size(max_w)
                .resizable(true)
                .frame(crate::ui::chrome::right_dock_placeholder_frame(theme))
                .show(root, |ui| {
                    let h = ui.available_height().max(1.0);
                    let w = ui.available_width().max(1.0);
                    ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
                });
        });
        let dock_inset = theme.spacing_right_dock_screen_inset();
        // #206 S1：Panel::show 不再返回 response；槽位改由 PanelState 落盘矩形推得。
        let slot = layout_util::side_panel_state_rect(ctx, "port_forward_panel")
            .map(|r| {
                let screen = ctx.content_rect();
                let mut s = r;
                s.min.x = screen.max.x - s.width();
                s.max.x = screen.max.x - dock_inset;
                s
            })
            .unwrap_or(egui::Rect::NOTHING);
        crate::ui::chrome::paint_right_dock_slot_gap(ctx, theme, slot);
        self.last_panel_slot_rect = Some(slot);
        if let Some(slot) = self.last_panel_slot_rect {
            layout_util::record_right_dock_panel_rect(&slot, right_dock_outer_left);
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
        let screen = ctx.content_rect();
        let geom = crate::ui::chrome::prepare_right_dock_foreground_geom(slot, screen, theme);
        let layer_id = crate::ui::chrome::right_dock_foreground_layer_id("mistterm_port_fwd_fg");
        crate::ui::chrome::paint_right_dock_foreground_shell(ctx, layer_id, geom.paint, theme);
        crate::ui::chrome::show_right_dock_foreground_body(
            "mistterm_port_fwd_fg",
            ctx,
            theme,
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
                    "Hide panel · reopen from Activity Rail or View menu",
                    "隐藏面板 · 可从活动栏或「视图」菜单再打开",
                ),
            );
        });
        if header_closed {
            *close_panel = true;
        }
        crate::ui::chrome::right_dock_header_divider(ui, theme);
        ui.spacing_mut().item_spacing.y = prev_gap;
        ui.add_space(theme.spacing_dock_section_gap());

        if !crate::ui::chrome::show_right_dock_ssh_gate(
            ui,
            theme,
            ctx,
            terminal,
            "Connect a session before using port forwarding.",
            "请打开会话并连接后可使用端口转发。",
        ) {
            return;
        }
        let Some(t) = terminal else {
            return;
        };
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
            self.register_profile_forwards(ssh_session_id, profile, &mgr);
        }
        self.prune_stopped(ssh_session_id);

        self.show_active_list(ui, ctx, theme, ssh_session_id, session_profile);
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
        session_profile: Option<&SessionConfig>,
    ) {
        section_title(
            ui,
            theme,
            crate::i18n::tr(ctx, "Active forwards", "运行中的转发"),
        );
        ui.add_space(4.0);

        let empty_hint = crate::i18n::tr(
            ctx,
            "No active forwards yet. Pick a type below, fill in the ports, then Start forward.",
            "还没有运行中的转发。在下方选类型并填写端口后点「启动转发」。",
        );
        let Some(list) = self.by_ssh_session.get(&ssh_session_id) else {
            ui.label(
                egui::RichText::new(empty_hint)
                    .small()
                    .color(theme.text_tertiary()),
            );
            return;
        };
        if list.is_empty() {
            ui.label(
                egui::RichText::new(empty_hint)
                    .small()
                    .color(theme.text_tertiary()),
            );
            return;
        }

        let mut stop_idx: Option<usize> = None;
        let mut delete_idx: Option<usize> = None;
        for (i, entry) in list.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(entry.label())
                        .small()
                        .color(theme.text_primary()),
                );
                if entry.from_profile() {
                    ui.label(
                        egui::RichText::new(crate::i18n::tr(ctx, "profile", "配置"))
                            .small()
                            .color(theme.text_tertiary()),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // RTL: Delete then Stop → visual Stop | Delete
                    if crate::ui::chrome::chrome_small_button(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Delete", "删除"),
                    )
                    .on_hover_text(crate::i18n::tr(
                        ctx,
                        "Remove this forward from the session profile (stops it first if running)",
                        "从会话配置中去掉该条（若在运行会先停止）",
                    ))
                    .clicked()
                    {
                        delete_idx = Some(i);
                    }
                    if crate::ui::chrome::chrome_small_button(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Stop", "停止"),
                    )
                    .on_hover_text(crate::i18n::tr(
                        ctx,
                        "Stop this forward now; profile line is kept (reconnect may restore it)",
                        "停止当前转发；保留配置（重连可能再拉起）",
                    ))
                    .clicked()
                    {
                        stop_idx = Some(i);
                    }
                });
            });
        }

        // Prefer Delete if both somehow set (same row click races); Delete includes stop.
        if let Some(i) = delete_idx.or(stop_idx) {
            let remove_config = delete_idx.is_some();
            self.apply_row_action(ssh_session_id, i, session_profile, remove_config);
        }
    }

    /// `remove_config`: false = 停止；true = 删除（先停再删配置）。
    fn apply_row_action(
        &mut self,
        ssh_session_id: SshSessionId,
        index: usize,
        session_profile: Option<&SessionConfig>,
        remove_config: bool,
    ) {
        let Some(list) = self.by_ssh_session.get_mut(&ssh_session_id) else {
            return;
        };
        if index >= list.len() {
            return;
        }
        let entry = list.remove(index);
        let kind = entry.kind().clone();
        entry.control().stop();
        if remove_config {
            if let Some(profile) = session_profile {
                self.pending_remove = Some(PortForwardRemoveRequest {
                    session_profile_id: profile.id.clone(),
                    kind: kind.clone(),
                });
            }
        }
        self.pending_audits.push(PortForwardAuditRequest {
            session_profile_id: session_profile.map(|p| p.id.clone()),
            host: session_profile.map(|p| p.host.clone()),
            kind,
            started: false,
        });
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
        let tabs = [
            (
                "local",
                crate::i18n::tr(ctx, "Local forward", "本机转发"),
            ),
            (
                "remote",
                crate::i18n::tr(ctx, "Remote forward", "远端转发"),
            ),
            (
                "dynamic",
                crate::i18n::tr(ctx, "SOCKS proxy", "SOCKS 代理"),
            ),
        ];
        let active = match self.form_kind {
            ForwardFormKind::Local => "local",
            ForwardFormKind::Remote => "remote",
            ForwardFormKind::Dynamic => "dynamic",
        };
        if let Some(picked) = crate::ui::chrome::segmented_control_row(
            ui,
            theme,
            &tabs,
            active,
            Some(ui.available_width()),
        ) {
            self.form_kind = match picked.as_str() {
                "remote" => ForwardFormKind::Remote,
                "dynamic" => ForwardFormKind::Dynamic,
                _ => ForwardFormKind::Local,
            };
        }
        ui.label(
            egui::RichText::new(crate::i18n::tr(
                ctx,
                "Same as OpenSSH -L / -R / -D.",
                "对应 OpenSSH 的 -L / -R / -D。",
            ))
            .small()
            .color(theme.text_tertiary()),
        );
        if self.form_kind != prev_kind {
            self.form.fill_defaults_for(self.form_kind);
        }
        ui.add_space(6.0);
        theme.frame_inset_section().show(ui, |ui| {
            self.draw_form_fields(ui, ctx, theme);
        });

        crate::ui::chrome::form_checkbox(
            ui,
            theme,
            &mut self.save_to_session,
            crate::i18n::tr(ctx, "Save to session profile", "保存到会话配置"),
        );

        if crate::ui::chrome::panel_action_primary_button(
            ui,
            theme,
            crate::i18n::tr(ctx, "Start forward", "启动转发"),
        )
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

    fn draw_form_fields(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, theme: &Theme) {
        match self.form_kind {
            ForwardFormKind::Local | ForwardFormKind::Dynamic => {
                crate::ui::chrome::form_control_row(ui, theme, |ui| {
                    crate::ui::chrome::form_inline_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Listen on", "本机监听"),
                    );
                    crate::ui::chrome::form_inline_singleline(
                        ui,
                        theme,
                        egui::Id::new("pf_bind_address"),
                        &mut self.form.bind_address,
                        "127.0.0.1",
                        100.0,
                        false,
                    );
                    crate::ui::chrome::form_inline_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Port", "端口"),
                    );
                    crate::ui::chrome::form_inline_singleline(
                        ui,
                        theme,
                        egui::Id::new("pf_local_port_ld"),
                        &mut self.form.local_port,
                        "8080",
                        64.0,
                        false,
                    );
                });
            }
            ForwardFormKind::Remote => {
                crate::ui::chrome::form_control_row(ui, theme, |ui| {
                    crate::ui::chrome::form_inline_label(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Remote port", "远端端口"),
                    );
                    crate::ui::chrome::form_inline_singleline(
                        ui,
                        theme,
                        egui::Id::new("pf_local_port_r"),
                        &mut self.form.local_port,
                        "8080",
                        64.0,
                        false,
                    );
                });
            }
        }

        if self.form_kind == ForwardFormKind::Local {
            crate::ui::chrome::form_control_row(ui, theme, |ui| {
                crate::ui::chrome::form_inline_label(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Forward to", "转发到"),
                );
                crate::ui::chrome::form_inline_singleline(
                    ui,
                    theme,
                    egui::Id::new("pf_remote_host_l"),
                    &mut self.form.remote_host,
                    "127.0.0.1",
                    120.0,
                    false,
                );
                crate::ui::chrome::form_inline_label(ui, theme, ":");
                crate::ui::chrome::form_inline_singleline(
                    ui,
                    theme,
                    egui::Id::new("pf_remote_port_l"),
                    &mut self.form.remote_port,
                    "80",
                    64.0,
                    false,
                );
            });
        } else if self.form_kind == ForwardFormKind::Remote {
            crate::ui::chrome::form_control_row(ui, theme, |ui| {
                crate::ui::chrome::form_inline_label(
                    ui,
                    theme,
                    crate::i18n::tr(ctx, "Forward to", "转发到"),
                );
                crate::ui::chrome::form_inline_singleline(
                    ui,
                    theme,
                    egui::Id::new("pf_remote_host_r"),
                    &mut self.form.remote_host,
                    "127.0.0.1",
                    120.0,
                    false,
                );
                crate::ui::chrome::form_inline_label(ui, theme, ":");
                crate::ui::chrome::form_inline_singleline(
                    ui,
                    theme,
                    egui::Id::new("pf_remote_port_r"),
                    &mut self.form.remote_port,
                    "3000",
                    64.0,
                    false,
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

    fn push_profile(
        &mut self,
        ssh_session_id: SshSessionId,
        label: String,
        kind: PortForwardKind,
        control: ForwardControl,
    ) {
        self.by_ssh_session
            .entry(ssh_session_id)
            .or_default()
            .push(ForwardEntry::Profile {
                label,
                kind,
                control,
            });
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
            list.retain(|entry| !entry.control().is_stopped());
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
    use crate::ssh::{DynamicPortForward, LocalPortForward, RemotePortForward};

    fn sample_local_kind() -> PortForwardKind {
        PortForwardKind::Local(LocalPortForward {
            local_port: 8080,
            remote_host: "127.0.0.1".into(),
            remote_port: 80,
            bind_address: "127.0.0.1".into(),
        })
    }

    fn push_sample_profile(panel: &mut PortForwardPanel, ssh_id: SshSessionId) {
        let kind = sample_local_kind();
        let label = kind.display_label();
        panel
            .profile_registered
            .insert(ssh_id);
        panel.push_profile(ssh_id, label, kind, ForwardControl::new());
    }

    #[test]
    fn profile_row_stop_does_not_pending_remove() {
        let mut panel = PortForwardPanel::new();
        push_sample_profile(&mut panel, 1);
        let mut profile = SessionConfig::default();
        profile.id = "sess-1".into();
        profile.local_forwards_text = "127.0.0.1:8080:127.0.0.1:80\n".into();
        assert_eq!(panel.active_count_for(1), 1);
        panel.apply_row_action(1, 0, Some(&profile), false);
        assert_eq!(panel.active_count_for(1), 0);
        assert!(panel.take_pending_remove().is_none());
        assert_eq!(panel.take_pending_audits().len(), 1);
    }

    #[test]
    fn profile_row_delete_pending_removes_config() {
        let mut panel = PortForwardPanel::new();
        push_sample_profile(&mut panel, 1);
        let mut profile = SessionConfig::default();
        profile.id = "sess-1".into();
        panel.apply_row_action(1, 0, Some(&profile), true);
        let remove = panel.take_pending_remove().expect("delete should queue remove");
        assert_eq!(remove.session_profile_id, "sess-1");
        assert!(matches!(remove.kind, PortForwardKind::Local(_)));
        assert_eq!(panel.active_count_for(1), 0);
    }

    #[test]
    fn runtime_stop_keeps_config_delete_removes() {
        let mut panel = PortForwardPanel::new();
        let kind = sample_local_kind();
        let label = kind.display_label();
        panel.push_runtime(1, label, kind, ForwardControl::new());
        let mut profile = SessionConfig::default();
        profile.id = "sess-2".into();

        panel.apply_row_action(1, 0, Some(&profile), false);
        assert!(panel.take_pending_remove().is_none());

        let kind = sample_local_kind();
        let label = kind.display_label();
        panel.push_runtime(1, label, kind, ForwardControl::new());
        panel.apply_row_action(1, 0, Some(&profile), true);
        assert!(panel.take_pending_remove().is_some());
    }

    #[test]
    fn clear_session_stops_profile_and_drops_flag() {
        let mut panel = PortForwardPanel::new();
        push_sample_profile(&mut panel, 2);
        assert!(panel.profile_registered.contains(&2));
        panel.clear_ssh_session(2);
        assert!(!panel.profile_registered.contains(&2));
        assert_eq!(panel.active_count_for(2), 0);
        // start + stop audits: push had no audit; clear emits stop
        assert_eq!(panel.take_pending_audits().len(), 1);
    }

    #[test]
    fn retain_live_sessions_drops_stale() {
        use std::collections::HashSet;
        let mut panel = PortForwardPanel::new();
        push_sample_profile(&mut panel, 1);
        push_sample_profile(&mut panel, 2);
        assert_eq!(panel.active_count_for(1), 1);
        assert_eq!(panel.active_count_for(2), 1);
        let mut live = HashSet::new();
        live.insert(2);
        panel.retain_live_sessions(&live);
        assert_eq!(panel.active_count_for(1), 0);
        assert_eq!(panel.active_count_for(2), 1);
    }

    #[test]
    fn prune_stopped_drops_profile_and_runtime() {
        let mut panel = PortForwardPanel::new();
        let kind = sample_local_kind();
        let control = ForwardControl::new();
        control.stop();
        panel.push_profile(3, kind.display_label(), kind, control);
        let kind = PortForwardKind::Dynamic(DynamicPortForward {
            local_port: 1080,
            bind_address: "127.0.0.1".into(),
        });
        let control = ForwardControl::new();
        control.stop();
        panel.push_runtime(3, kind.display_label(), kind, control);
        // also one still running
        let kind = PortForwardKind::Remote(RemotePortForward {
            remote_port: 9000,
            target_host: "127.0.0.1".into(),
            target_port: 3000,
            remote_bind_address: None,
        });
        panel.push_profile(3, kind.display_label(), kind, ForwardControl::new());
        panel.prune_stopped(3);
        assert_eq!(panel.active_count_for(3), 1);
    }
}
