//! SFTP 传输队列：进度、取消、覆盖策略、断点续传决策。

use crate::ssh::WriteMode;
use crate::ui::theme::Theme;
use eframe::egui::{self, RichText};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum XferKind {
    Upload,
    Download,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum XferState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
    Skipped,
}

/// 用户对「目标已存在」的默认策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum OverwritePolicy {
    #[default]
    Ask,
    OverwriteAll,
    SkipAll,
    ResumeAll,
    /// 仅当源比目标新时覆盖（按 mtime；相等则比 size）。
    IfNewerAll,
}

#[derive(Debug, Clone)]
pub(super) struct XferItem {
    pub id: u64,
    pub kind: XferKind,
    pub local: PathBuf,
    pub remote: PathBuf,
    pub mode: WriteMode,
    pub state: XferState,
    pub done: u64,
    pub total: u64,
    pub error: Option<String>,
    pub started_at: Option<Instant>,
    pub speed_bps: f64,
    /// 同步下载时的本机对比根。用来拒绝跟随根下的符号链接。
    pub guard_local: Option<PathBuf>,
    /// 同步上传时的远端对比根。用来拒绝跟随根下的符号链接。
    pub guard_remote: Option<PathBuf>,
    last_sample_at: Option<Instant>,
    last_sample_done: u64,
}

impl XferItem {
    pub fn display_name(&self) -> String {
        match self.kind {
            XferKind::Upload => self
                .local
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.local.to_string_lossy().into_owned()),
            XferKind::Download => self
                .remote
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.remote.to_string_lossy().into_owned()),
        }
    }

    pub fn update_progress(&mut self, done: u64, total: u64) {
        let now = Instant::now();
        if let Some(at) = self.last_sample_at {
            let dt = now.saturating_duration_since(at).as_secs_f64();
            if dt >= 0.2 {
                let delta = done.saturating_sub(self.last_sample_done) as f64;
                self.speed_bps = delta / dt;
                self.last_sample_at = Some(now);
                self.last_sample_done = done;
            }
        } else {
            self.last_sample_at = Some(now);
            self.last_sample_done = done;
        }
        self.done = done;
        self.total = total;
    }
}

pub(super) struct XferQueue {
    pub items: VecDeque<XferItem>,
    next_id: u64,
    pub policy: OverwritePolicy,
    pub cancel: Option<Arc<AtomicBool>>,
    pub queue_expanded: bool,
}

impl Default for XferQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl XferQueue {
    pub fn new() -> Self {
        Self {
            items: VecDeque::new(),
            next_id: 1,
            policy: OverwritePolicy::Ask,
            cancel: None,
            queue_expanded: false,
        }
    }

    pub fn clear(&mut self) {
        if let Some(c) = &self.cancel {
            c.store(true, Ordering::Relaxed);
        }
        self.items.clear();
        self.cancel = None;
        self.policy = OverwritePolicy::Ask;
    }

    pub fn active_id(&self) -> Option<u64> {
        self.items
            .iter()
            .find(|i| i.state == XferState::Running)
            .map(|i| i.id)
    }

    pub fn queued_count(&self) -> usize {
        self.items
            .iter()
            .filter(|i| i.state == XferState::Queued)
            .count()
    }

    pub fn has_work(&self) -> bool {
        self.items
            .iter()
            .any(|i| matches!(i.state, XferState::Queued | XferState::Running))
    }

    /// 断连 / worker 挂掉：取消进行中，并把 Queued/Running 标为 Failed（#94 P0）。
    pub fn fail_all_in_flight(&mut self, error: String) -> bool {
        let had = self.has_work();
        if let Some(c) = &self.cancel {
            c.store(true, Ordering::Relaxed);
        }
        for item in self.items.iter_mut() {
            if matches!(item.state, XferState::Queued | XferState::Running) {
                item.state = XferState::Failed;
                item.error = Some(error.clone());
                item.speed_bps = 0.0;
            }
        }
        self.cancel = None;
        had
    }

    pub fn push(
        &mut self,
        kind: XferKind,
        local: PathBuf,
        remote: PathBuf,
        mode: WriteMode,
        total_hint: u64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.items.push_back(XferItem {
            id,
            kind,
            local,
            remote,
            mode,
            state: XferState::Queued,
            done: 0,
            total: total_hint,
            error: None,
            started_at: None,
            speed_bps: 0.0,
            last_sample_at: None,
            last_sample_done: 0,
            guard_local: None,
            guard_remote: None,
        });
        id
    }

    /// 目录同步入队。写模式是「目标必须不存在」，并带上对比根以便落盘时拒绝符号链接。
    pub fn push_sync(
        &mut self,
        kind: XferKind,
        local: PathBuf,
        remote: PathBuf,
        total_hint: u64,
        local_root: PathBuf,
        remote_root: PathBuf,
    ) -> u64 {
        let id = self.push(
            kind,
            local,
            remote,
            crate::ssh::WriteMode::Create,
            total_hint,
        );
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.guard_local = Some(local_root);
            item.guard_remote = Some(remote_root);
        }
        id
    }

    pub fn take_next_queued(&mut self) -> Option<XferItem> {
        let idx = self
            .items
            .iter()
            .position(|i| i.state == XferState::Queued)?;
        let mut item = self.items.remove(idx)?;
        item.state = XferState::Running;
        item.started_at = Some(Instant::now());
        // Keep a clone in the queue for UI, and return a working copy.
        self.items.insert(idx, item.clone());
        Some(item)
    }

    pub fn mark_running_progress(&mut self, id: u64, done: u64, total: u64) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.update_progress(done, total);
            item.state = XferState::Running;
        }
    }

    pub fn finish(
        &mut self,
        id: u64,
        state: XferState,
        done: u64,
        error: Option<String>,
    ) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.state = state;
            item.done = done;
            item.error = error;
            item.speed_bps = 0.0;
        }
        self.cancel = None;
    }

    pub fn request_cancel_active(&mut self) {
        if let Some(c) = &self.cancel {
            c.store(true, Ordering::Relaxed);
        }
    }

    /// 取消这一批里尚未开始的项，并请求取消其中正在跑的那一项。
    pub fn cancel_ids(&mut self, ids: &[u64]) {
        if ids.is_empty() {
            return;
        }
        let active = self.active_id();
        for item in &mut self.items {
            if item.state == XferState::Queued && ids.contains(&item.id) {
                item.state = XferState::Cancelled;
                item.speed_bps = 0.0;
            }
        }
        if active.is_some_and(|id| ids.contains(&id)) {
            self.request_cancel_active();
        }
    }

    pub fn retry_failed(&mut self, id: u64) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            if item.state == XferState::Failed || item.state == XferState::Cancelled {
                item.state = XferState::Queued;
                item.error = None;
                item.done = 0;
                item.speed_bps = 0.0;
                item.started_at = None;
                item.last_sample_at = None;
                item.last_sample_done = 0;
            }
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.items.retain(|i| i.id != id);
    }

    pub fn prune_finished(&mut self) {
        self.items.retain(|i| {
            !matches!(
                i.state,
                XferState::Done | XferState::Skipped | XferState::Cancelled
            )
        });
    }
}

/// 根据源/目标大小决定默认写模式（策略非 Ask 时）。
pub(super) fn resolve_write_mode(
    policy: OverwritePolicy,
    dest_exists: bool,
    dest_size: u64,
    source_size: u64,
) -> Option<WriteMode> {
    resolve_write_mode_mtime(policy, dest_exists, dest_size, source_size, None, None)
}

/// `source_mtime` / `dest_mtime` 为 Unix 秒；缺省则 `IfNewerAll` 退化为比 size。
pub(super) fn resolve_write_mode_mtime(
    policy: OverwritePolicy,
    dest_exists: bool,
    dest_size: u64,
    source_size: u64,
    source_mtime: Option<u64>,
    dest_mtime: Option<u64>,
) -> Option<WriteMode> {
    if !dest_exists || dest_size == 0 {
        return Some(WriteMode::Overwrite);
    }
    match policy {
        OverwritePolicy::Ask => None,
        OverwritePolicy::OverwriteAll => Some(WriteMode::Overwrite),
        OverwritePolicy::SkipAll => None, // caller treats as skip
        OverwritePolicy::ResumeAll => {
            if dest_size < source_size {
                Some(WriteMode::Resume)
            } else if dest_size == source_size {
                None // skip complete
            } else {
                Some(WriteMode::Overwrite) // dest larger → restart
            }
        }
        OverwritePolicy::IfNewerAll => {
            let newer = match (source_mtime, dest_mtime) {
                (Some(s), Some(d)) if s != d => s > d,
                _ => source_size != dest_size,
            };
            if newer {
                Some(WriteMode::Overwrite)
            } else if dest_size == source_size {
                None
            } else {
                Some(WriteMode::Overwrite)
            }
        }
    }
}

pub(super) fn format_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let v = n as f64;
    if v >= GB {
        format!("{:.1} GB", v / GB)
    } else if v >= MB {
        format!("{:.1} MB", v / MB)
    } else if v >= KB {
        format!("{:.0} KB", v / KB)
    } else {
        format!("{n} B")
    }
}

pub(super) fn format_speed(bps: f64) -> String {
    if bps <= 0.0 {
        return "—".into();
    }
    format!("{}/s", format_bytes(bps as u64))
}

pub(super) fn format_eta(done: u64, total: u64, bps: f64) -> String {
    if bps <= 1.0 || total <= done {
        return "—".into();
    }
    let secs = ((total - done) as f64 / bps).round() as u64;
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// 底部传输条：当前进度 + 可选队列列表。
pub(super) fn paint_xfer_strip(
    ui: &mut egui::Ui,
    theme: &Theme,
    ctx: &egui::Context,
    queue: &mut XferQueue,
    host_line: &str,
    remote_cwd: &str,
    local_cwd: &str,
    details_open: &mut bool,
) -> XferStripAction {
    if theme.uses_modern_palette() {
        return paint_flux_xfer_strip(
            ui,
            theme,
            ctx,
            queue,
            host_line,
            remote_cwd,
            local_cwd,
            details_open,
        );
    }
    let _ = (host_line, remote_cwd, local_cwd, details_open);
    let mut action = XferStripAction::None;
    let running = queue
        .items
        .iter()
        .find(|i| i.state == XferState::Running)
        .cloned();
    let queued = queue.queued_count();
    let failed = queue
        .items
        .iter()
        .filter(|i| i.state == XferState::Failed)
        .count();
    ui.add_space(theme.spacing_sm());
    ui.separator();
    let policy_label = match queue.policy {
        OverwritePolicy::Ask => crate::i18n::tr(ctx, "Ask", "同名先问"),
        OverwritePolicy::OverwriteAll => crate::i18n::tr(ctx, "Overwrite all", "同名都覆盖"),
        OverwritePolicy::SkipAll => crate::i18n::tr(ctx, "Skip all", "同名都跳过"),
        OverwritePolicy::ResumeAll => crate::i18n::tr(ctx, "Resume all", "同名接着传"),
        OverwritePolicy::IfNewerAll => crate::i18n::tr(ctx, "If newer", "只覆盖更新的"),
    };
    ui.horizontal(|ui| {
        let status = if running.is_some() {
            crate::i18n::tr(ctx, "Transferring", "正在传输")
        } else if queued > 0 {
            crate::i18n::tr(ctx, "Queued", "排队中")
        } else if failed > 0 {
            crate::i18n::tr(ctx, "Needs attention", "有失败")
        } else {
            crate::i18n::tr(ctx, "Idle", "空闲")
        };
        ui.label(
            RichText::new(crate::i18n::tr(ctx, "Transfers", "传输"))
                .size(theme.font_size_small())
                .color(theme.text_secondary()),
        );
        ui.label(
            RichText::new(format!("· {status}"))
                .size(theme.font_size_small())
                .color(theme.text_tertiary()),
        );
        if queued > 0 || failed > 0 {
            ui.label(
                RichText::new(format!("· {queued} · {failed}"))
                    .size(theme.font_size_small())
                    .color(theme.text_tertiary()),
            );
        }
        if ui
            .small_button(policy_label)
            .on_hover_text(crate::i18n::tr(
                ctx,
                "What to do when the other side already has this file. Click to change.",
                "对方已有同名文件时怎么处理。点一下切换。",
            ))
            .clicked()
        {
            queue.policy = match queue.policy {
                OverwritePolicy::Ask => OverwritePolicy::OverwriteAll,
                OverwritePolicy::OverwriteAll => OverwritePolicy::SkipAll,
                OverwritePolicy::SkipAll => OverwritePolicy::ResumeAll,
                OverwritePolicy::ResumeAll => OverwritePolicy::IfNewerAll,
                OverwritePolicy::IfNewerAll => OverwritePolicy::Ask,
            };
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let expand = if queue.queue_expanded {
                crate::i18n::tr(ctx, "Hide queue", "收起队列")
            } else {
                crate::i18n::tr(ctx, "Show queue", "展开队列")
            };
            if ui
                .add(egui::Button::new(RichText::new(expand).size(theme.font_size_small())))
                .clicked()
            {
                queue.queue_expanded = !queue.queue_expanded;
            }
        });
    });

    if let Some(item) = running {
        let pct = if item.total > 0 {
            (item.done as f64 / item.total as f64).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let label = format!(
            "{}  {} / {}  {}",
            item.display_name(),
            format_bytes(item.done),
            if item.total > 0 {
                format_bytes(item.total)
            } else {
                "?".into()
            },
            format_speed(item.speed_bps),
        );
        crate::ui::chrome::progress_bar_labeled(
            ui,
            theme,
            pct,
            &label,
            Some(ui.available_width()),
        );
        ui.horizontal(|ui| {
            if crate::ui::chrome::panel_action_icon_button(
                ui,
                theme,
                crate::ui::icons::IconId::Cross,
                crate::i18n::tr(ctx, "Cancel", "取消"),
            )
            .clicked()
            {
                action = XferStripAction::CancelActive;
            }
        });
    } else if queued > 0 {
        ui.label(
            RichText::new(crate::i18n::tr(ctx, "Waiting to start…", "等待开始…"))
                .size(theme.font_size_small())
                .color(theme.text_tertiary()),
        );
    }

    if queue.queue_expanded {
        egui::ScrollArea::vertical()
            .max_height(theme.size_file_list_row_h() * 4.5)
            .show(ui, |ui| {
                let snapshot: Vec<_> = queue.items.iter().cloned().collect();
                for item in snapshot {
                    ui.horizontal(|ui| {
                        let state = match item.state {
                            XferState::Queued => crate::i18n::tr(ctx, "queued", "排队"),
                            XferState::Running => crate::i18n::tr(ctx, "running", "进行中"),
                            XferState::Done => crate::i18n::tr(ctx, "done", "完成"),
                            XferState::Failed => crate::i18n::tr(ctx, "failed", "失败"),
                            XferState::Cancelled => crate::i18n::tr(ctx, "cancelled", "已取消"),
                            XferState::Skipped => crate::i18n::tr(ctx, "skipped", "已跳过"),
                        };
                        ui.label(
                            RichText::new(format!("[{state}] {}", item.display_name()))
                                .size(theme.font_size_small()),
                        );
                        if item.state == XferState::Failed || item.state == XferState::Cancelled {
                            if ui
                                .small_button(crate::i18n::tr(ctx, "Retry", "重试"))
                                .clicked()
                            {
                                action = XferStripAction::Retry(item.id);
                            }
                            if ui
                                .small_button(crate::i18n::tr(ctx, "Remove", "移除"))
                                .clicked()
                            {
                                action = XferStripAction::Remove(item.id);
                            }
                        }
                    });
                    if let Some(err) = &item.error {
                        ui.label(
                            RichText::new(err)
                                .size(theme.font_size_small())
                                .color(theme.red_color()),
                        );
                    }
                }
            });
        ui.horizontal(|ui| {
            if ui
                .small_button(crate::i18n::tr(ctx, "Clear finished", "清除已完成"))
                .clicked()
            {
                action = XferStripAction::PruneFinished;
            }
        });
    }

    action
}

fn paint_flux_xfer_strip(
    ui: &mut egui::Ui,
    theme: &Theme,
    ctx: &egui::Context,
    queue: &mut XferQueue,
    host_line: &str,
    remote_cwd: &str,
    local_cwd: &str,
    details_open: &mut bool,
) -> XferStripAction {
    let mut action = XferStripAction::None;
    let running = queue
        .items
        .iter()
        .find(|i| i.state == XferState::Running)
        .cloned();
    let queued: Vec<_> = queue
        .items
        .iter()
        .filter(|i| i.state == XferState::Queued)
        .take(5)
        .cloned()
        .collect();
    let failed = queue
        .items
        .iter()
        .filter(|i| i.state == XferState::Failed)
        .count();

    ui.add_space(4.0);
    let bar_w = ui.available_width().max(1.0);
    let (bar_hit, _) = ui.allocate_exact_size(egui::vec2(bar_w, 8.0), egui::Sense::hover());
    let track = egui::Rect::from_center_size(bar_hit.center(), egui::vec2(bar_w, 2.0));
    ui.painter().rect_filled(
        track,
        1.0,
        theme.border_divider_color(),
    );
    let pct = running.as_ref().map(|item| {
        if item.total > 0 {
            (item.done as f64 / item.total as f64).clamp(0.0, 1.0) as f32
        } else {
            0.0
        }
    });
    if let Some(p) = pct {
        let mut fill = track;
        fill.set_width((track.width() * p).max(0.0));
        ui.painter().rect_filled(fill, 1.0, theme.accent_color());
    }
    let bar_resp = ui.interact(bar_hit, ui.id().with("flux_xfer_bar"), egui::Sense::hover());
    bar_resp.on_hover_ui(|ui| {
        ui.set_min_width(220.0);
        ui.label(
            RichText::new(crate::i18n::tr(ctx, "Up next", "接下来"))
                .size(theme.font_size_caption())
                .color(theme.text_tertiary()),
        );
        if queued.is_empty() && running.is_none() {
            ui.label(
                RichText::new(crate::i18n::tr(ctx, "Nothing queued", "没有排队的文件"))
                    .size(theme.font_size_small())
                    .color(theme.text_secondary()),
            );
        }
        if let Some(item) = &running {
            ui.label(
                RichText::new(format!("● {}", item.display_name()))
                    .font(egui::FontId::monospace(theme.font_size_small()))
                    .color(theme.text_primary()),
            );
        }
        for item in &queued {
            ui.label(
                RichText::new(item.display_name())
                    .font(egui::FontId::monospace(theme.font_size_small()))
                    .color(theme.text_secondary()),
            );
        }
    });

    let status = if let Some(item) = &running {
        let p = if item.total > 0 {
            format!("{:.0}%", (item.done as f64 / item.total as f64) * 100.0)
        } else {
            "…".into()
        };
        let arrow = match item.kind {
            XferKind::Download => "↓",
            XferKind::Upload => "↑",
        };
        format!(
            "{arrow} {}   {p}   {}   {}",
            item.display_name(),
            format_speed(item.speed_bps),
            format_eta(item.done, item.total, item.speed_bps)
        )
    } else if !queued.is_empty() {
        crate::i18n::tr(ctx, "Waiting to start", "等待开始").to_string()
    } else if failed > 0 {
        crate::i18n::tr(ctx, "A transfer failed", "有传输失败").to_string()
    } else {
        crate::i18n::tr(ctx, "Idle", "空闲").to_string()
    };

    ui.horizontal(|ui| {
        ui.set_min_height(22.0);
        ui.label(
            RichText::new(status)
                .font(egui::FontId::monospace(theme.font_size_small()))
                .color(theme.text_secondary()),
        );
        if running.is_some()
            && crate::ui::chrome::icon_button(
                ui,
                theme,
                crate::ui::icons::IconId::Cross,
                theme.text_secondary(),
            )
            .on_hover_text(crate::i18n::tr(ctx, "Cancel", "取消"))
            .clicked()
        {
            action = XferStripAction::CancelActive;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let gear = crate::ui::chrome::icon_button(
                ui,
                theme,
                crate::ui::icons::IconId::Settings,
                theme.text_secondary(),
            )
            .on_hover_text(crate::i18n::tr(
                ctx,
                "When a file already exists",
                "遇到同名文件时",
            ));
            let popup_id = ui.make_persistent_id("sftp_flux_policy");
            let just_toggled = gear.clicked();
            if just_toggled {
                egui::Popup::toggle_id(ui.ctx(), popup_id);
            }
            if egui::Popup::is_id_open(ui.ctx(), popup_id) {
                let popup_w = 196.0;
                let screen = ui.ctx().content_rect();
                let mut pos = egui::pos2(
                    gear.rect.right() - popup_w,
                    gear.rect.top() - 176.0,
                );
                pos.x = pos.x.max(screen.left() + 8.0);
                if pos.y < screen.top() + 8.0 {
                    pos.y = gear.rect.bottom() + 4.0;
                }
                let area = egui::Area::new(popup_id)
                    .order(egui::Order::Foreground)
                    .fixed_pos(pos)
                    .constrain(false)
                    .show(ui.ctx(), |ui| {
                        ui.set_width(popup_w);
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            crate::ui::chrome::apply_context_menu_style(ui, theme);
                            ui.set_max_width(popup_w - 16.0);
                            ui.label(
                                RichText::new(crate::i18n::tr(ctx, "Same name", "同名文件"))
                                    .size(theme.font_size_caption())
                                    .color(theme.text_tertiary()),
                            );
                            let choices = [
                                (OverwritePolicy::Ask, crate::i18n::tr(ctx, "Ask", "先问我")),
                                (
                                    OverwritePolicy::OverwriteAll,
                                    crate::i18n::tr(ctx, "Overwrite", "直接覆盖"),
                                ),
                                (
                                    OverwritePolicy::SkipAll,
                                    crate::i18n::tr(ctx, "Skip", "跳过"),
                                ),
                                (
                                    OverwritePolicy::IfNewerAll,
                                    crate::i18n::tr(ctx, "Only if newer", "只覆盖较新的"),
                                ),
                                (
                                    OverwritePolicy::ResumeAll,
                                    crate::i18n::tr(ctx, "Resume", "接着传"),
                                ),
                            ];
                            for (policy, label) in choices {
                                let mark = if queue.policy == policy { "● " } else { "  " };
                                if crate::ui::chrome::popup_menu_button(
                                    ui,
                                    theme,
                                    &format!("{mark}{label}"),
                                )
                                .clicked()
                                {
                                    queue.policy = policy;
                                    egui::Popup::close_id(ui.ctx(), popup_id);
                                }
                            }
                        });
                    });
                if !just_toggled && area.response.clicked_elsewhere() {
                    egui::Popup::close_id(ui.ctx(), popup_id);
                }
            }
        });
    });

    ui.horizontal(|ui| {
        let dot = if failed > 0 {
            theme.red_color()
        } else if running.is_some() || !queued.is_empty() {
            theme.amber_color()
        } else {
            theme.green_color()
        };
        let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(dot_rect.center(), 3.5, dot);
        let host = if host_line.is_empty() {
            crate::i18n::tr(ctx, "SFTP", "SFTP").to_string()
        } else {
            format!("SFTP  ·  {host_line}")
        };
        if ui
            .add(
                egui::Label::new(
                    RichText::new(host)
                        .font(egui::FontId::monospace(theme.font_size_caption()))
                        .color(theme.text_tertiary()),
                )
                .sense(egui::Sense::click()),
            )
            .on_hover_text(crate::i18n::tr(ctx, "Connection details", "连接详情"))
            .clicked()
        {
            *details_open = !*details_open;
        }
    });

    if *details_open {
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "{}  {local_cwd}",
                crate::i18n::tr(ctx, "Local", "本机")
            ))
            .font(egui::FontId::monospace(theme.font_size_caption()))
            .color(theme.text_secondary()),
        );
        ui.label(
            RichText::new(format!(
                "{}  {remote_cwd}",
                crate::i18n::tr(ctx, "Remote", "远端")
            ))
            .font(egui::FontId::monospace(theme.font_size_caption()))
            .color(theme.text_secondary()),
        );
        let snapshot: Vec<_> = queue.items.iter().rev().take(6).cloned().collect();
        for item in snapshot.into_iter().rev() {
            let state = match item.state {
                XferState::Queued => crate::i18n::tr(ctx, "queued", "排队"),
                XferState::Running => crate::i18n::tr(ctx, "running", "进行中"),
                XferState::Done => crate::i18n::tr(ctx, "done", "完成"),
                XferState::Failed => crate::i18n::tr(ctx, "failed", "失败"),
                XferState::Cancelled => crate::i18n::tr(ctx, "cancelled", "已取消"),
                XferState::Skipped => crate::i18n::tr(ctx, "skipped", "已跳过"),
            };
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{state}  {}", item.display_name()))
                        .font(egui::FontId::monospace(theme.font_size_caption()))
                        .color(theme.text_secondary()),
                );
                if item.state == XferState::Failed || item.state == XferState::Cancelled {
                    if ui
                        .small_button(crate::i18n::tr(ctx, "Retry", "重试"))
                        .clicked()
                    {
                        action = XferStripAction::Retry(item.id);
                    }
                }
            });
        }
    }

    action
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum XferStripAction {
    None,
    CancelActive,
    Retry(u64),
    Remove(u64),
    PruneFinished,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_write_mode_ask_needs_prompt() {
        assert_eq!(
            resolve_write_mode(OverwritePolicy::Ask, true, 100, 200),
            None
        );
        assert_eq!(
            resolve_write_mode(OverwritePolicy::Ask, false, 0, 200),
            Some(WriteMode::Overwrite)
        );
    }

    #[test]
    fn resolve_write_mode_resume_all() {
        assert_eq!(
            resolve_write_mode(OverwritePolicy::ResumeAll, true, 50, 200),
            Some(WriteMode::Resume)
        );
        assert_eq!(
            resolve_write_mode(OverwritePolicy::ResumeAll, true, 200, 200),
            None
        );
        assert_eq!(
            resolve_write_mode(OverwritePolicy::ResumeAll, true, 300, 200),
            Some(WriteMode::Overwrite)
        );
    }

    #[test]
    fn queue_take_and_finish() {
        let mut q = XferQueue::new();
        let id = q.push(
            XferKind::Upload,
            PathBuf::from("a.bin"),
            PathBuf::from("/tmp/a.bin"),
            WriteMode::Overwrite,
            10,
        );
        let item = q.take_next_queued().unwrap();
        assert_eq!(item.id, id);
        assert_eq!(q.active_id(), Some(id));
        q.mark_running_progress(id, 5, 10);
        q.finish(id, XferState::Done, 10, None);
        assert!(q.active_id().is_none());
        assert!(!q.has_work());
    }

    #[test]
    fn fail_all_in_flight_marks_running_and_queued() {
        let mut q = XferQueue::new();
        let a = q.push(
            XferKind::Upload,
            PathBuf::from("a.bin"),
            PathBuf::from("/tmp/a.bin"),
            WriteMode::Overwrite,
            10,
        );
        let _b = q.push(
            XferKind::Download,
            PathBuf::from("b.bin"),
            PathBuf::from("/tmp/b.bin"),
            WriteMode::Overwrite,
            20,
        );
        let _ = q.take_next_queued();
        assert!(q.has_work());
        assert!(q.fail_all_in_flight("gone".into()));
        assert!(!q.has_work());
        assert_eq!(
            q.items.iter().find(|i| i.id == a).map(|i| i.state),
            Some(XferState::Failed)
        );
        assert!(q.items.iter().all(|i| i.state == XferState::Failed));
    }

    #[test]
    fn format_bytes_scales() {
        assert_eq!(format_bytes(512), "512 B");
        assert!(format_bytes(2048).contains("KB"));
    }
}
