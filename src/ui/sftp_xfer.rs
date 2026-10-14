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

    #[allow(dead_code)] // 供后续关面板/断连清理与测试使用
    pub fn has_work(&self) -> bool {
        self.items
            .iter()
            .any(|i| matches!(i.state, XferState::Queued | XferState::Running))
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
        });
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

/// 底部传输条：当前进度 + 可选队列列表。
pub(super) fn paint_xfer_strip(
    ui: &mut egui::Ui,
    theme: &Theme,
    ctx: &egui::Context,
    queue: &mut XferQueue,
) -> XferStripAction {
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
    if running.is_none() && queued == 0 && failed == 0 && queue.items.is_empty() {
        return action;
    }

    ui.add_space(theme.spacing_panel_gap());
    ui.separator();
    ui.horizontal(|ui| {
        let title = crate::i18n::tr(ctx, "Transfers", "传输");
        ui.label(
            RichText::new(title)
                .size(theme.font_size_small())
                .color(theme.text_secondary()),
        );
        if queued > 0 || failed > 0 {
            ui.label(
                RichText::new(format!("· {queued} queued · {failed} failed"))
                    .size(theme.font_size_small())
                    .color(theme.text_tertiary()),
            );
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
        ui.add(
            egui::ProgressBar::new(pct)
                .text(label)
                .desired_width(ui.available_width()),
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
    fn format_bytes_scales() {
        assert_eq!(format_bytes(512), "512 B");
        assert!(format_bytes(2048).contains("KB"));
    }
}
