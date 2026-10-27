//! SFTP 目录对比视图（#163）：列表、勾选、同步动作。扫描与入队在 `sftp_panel`。

use super::sftp_xfer::format_bytes;
use crate::core::sftp_diff::{rel_under_root, DiffKind, DiffRow, DiffSideMeta, WalkOutcome};
use crate::i18n::UiLanguage;
use crate::ssh::SftpClient;
use crate::ui::theme::Theme;
use eframe::egui::{self, Color32, Sense};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiffPhase {
    Idle,
    Scanning,
    Ready,
    Syncing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiffFilter {
    All,
    OnlyLocal,
    OnlyRemote,
    Conflict,
}

pub(super) enum DiffUiAction {
    Close,
    Rescan,
    Cancel,
    Sync {
        direction: crate::core::sftp_diff::SyncDirection,
        all: bool,
    },
}

pub(super) enum DiffScanOutcome {
    Done {
        rows: Vec<DiffRow>,
        remote_root: PathBuf,
    },
    Cancelled,
    Failed(String),
}

pub(super) enum DiffPrepOutcome {
    Ready {
        upload: bool,
        copies: Vec<(PathBuf, PathBuf, u64, String)>,
        skipped_existing: Vec<String>,
    },
    Cancelled,
    Failed(String),
}

pub(super) struct DirDiffUi {
    pub(super) open: bool,
    pub(super) phase: DiffPhase,
    pub(super) local_root: PathBuf,
    pub(super) remote_root: PathBuf,
    pub(super) rows: Vec<DiffRow>,
    pub(super) checked: HashSet<String>,
    pub(super) filter: DiffFilter,
    pub(super) error: Option<String>,
    pub(super) skip_note: Option<String>,
    pub(super) scanned: Arc<AtomicU64>,
    pub(super) cancel: Option<Arc<AtomicBool>>,
    pub(super) batch_ids: Vec<u64>,
    pub(super) scan_gen: u64,
    pub(super) prep_gen: u64,
    pub(super) prep_inflight: bool,
}

impl Default for DirDiffUi {
    fn default() -> Self {
        Self {
            open: false,
            phase: DiffPhase::Idle,
            local_root: PathBuf::new(),
            remote_root: PathBuf::new(),
            rows: Vec::new(),
            checked: HashSet::new(),
            filter: DiffFilter::All,
            error: None,
            skip_note: None,
            scanned: Arc::new(AtomicU64::new(0)),
            cancel: None,
            batch_ids: Vec::new(),
            scan_gen: 0,
            prep_gen: 0,
            prep_inflight: false,
        }
    }
}

impl DirDiffUi {
    pub(super) fn request_cancel(&self) {
        if let Some(flag) = &self.cancel {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

pub(super) fn default_checks(rows: &[DiffRow]) -> HashSet<String> {
    rows.iter()
        .filter(|row| row.kind != DiffKind::BothDiffer)
        .map(|row| row.rel.clone())
        .collect()
}

pub(super) fn format_skip_note(lang: UiLanguage, rels: &[String]) -> String {
    let shown = rels.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
    let extra = rels.len().saturating_sub(8);
    let tail = if extra > 0 {
        format!(" (+{extra})")
    } else {
        String::new()
    };
    match lang {
        UiLanguage::En => format!(
            "Skipped {} conflict(s); not overwritten: {shown}{tail}",
            rels.len()
        ),
        UiLanguage::Zh => format!("已跳过 {} 个冲突, 未覆盖: {shown}{tail}", rels.len()),
    }
}

pub(super) fn format_existing_note(lang: UiLanguage, rels: &[String]) -> String {
    let shown = rels.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
    let extra = rels.len().saturating_sub(8);
    let tail = if extra > 0 {
        format!(" (+{extra})")
    } else {
        String::new()
    };
    match lang {
        UiLanguage::En => format!(
            "Skipped {} existing path(s); not overwritten: {shown}{tail}",
            rels.len()
        ),
        UiLanguage::Zh => format!(
            "已跳过 {} 个已存在的路径, 未覆盖: {shown}{tail}",
            rels.len()
        ),
    }
}

/// 远端树扫描。`root` 应为已经 realpath + 规范化的目录。取消在每次列目录之后生效。
pub(super) fn collect_remote_diff(
    client: &SftpClient,
    root: &Path,
    cancel: &AtomicBool,
    scanned: &AtomicU64,
    mut on_tick: impl FnMut(),
) -> Result<WalkOutcome, String> {
    let root_norm = super::SftpPanel::normalize_remote_path(root.to_path_buf());
    let root_str = root_norm.to_string_lossy().replace('\\', "/");
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(WalkOutcome::Cancelled);
        }
        let entries = client.list_dir(&dir)?;
        let mut kept = 0usize;
        for entry in entries {
            if cancel.load(Ordering::Relaxed) {
                return Ok(WalkOutcome::Cancelled);
            }
            scanned.fetch_add(1, Ordering::Relaxed);
            on_tick();
            if entry.permissions.starts_with('l') {
                continue;
            }
            let shown = super::SftpPanel::normalize_remote_path(entry.path.clone());
            let full = shown.to_string_lossy().replace('\\', "/");
            let child_rel = rel_under_root(&root_str, &full)
                .ok_or_else(|| format!("Failed to place {full} under compare root {root_str}"))?;
            if child_rel.is_empty() {
                continue;
            }
            if entry.is_dir {
                kept += 1;
                stack.push((entry.path, child_rel));
            } else if entry.is_file {
                kept += 1;
                out.push(DiffSideMeta {
                    rel: child_rel,
                    size: entry.size,
                    mtime_secs: entry.modified.timestamp(),
                    is_dir: false,
                });
            }
        }
        if kept == 0 && !rel.is_empty() {
            out.push(DiffSideMeta {
                rel,
                size: 0,
                mtime_secs: 0,
                is_dir: true,
            });
        }
    }
    Ok(WalkOutcome::Done(out))
}

pub(super) fn paint_dir_diff(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    theme: &Theme,
    state: &mut DirDiffUi,
    busy: bool,
) -> Option<DiffUiAction> {
    if matches!(state.phase, DiffPhase::Scanning | DiffPhase::Syncing) {
        ctx.request_repaint_after(std::time::Duration::from_millis(150));
    }
    let mut action = None;
    ui.label(
        egui::RichText::new(crate::i18n::tr(ctx, "Folder compare", "目录对比"))
            .size(theme.font_size_body())
            .color(theme.text_primary()),
    );
    ui.label(
        egui::RichText::new(crate::i18n::tr(
            ctx,
            "Conflicts (same name, different size or time) are skipped and kept in this list. Sync does not overwrite, and does not delete extras on the target side.",
            "冲突(同名且大小或时间不同)会跳过并留在列表中, 同步不会覆盖, 也不会删除只在目标侧的文件.",
        ))
        .size(theme.font_size_caption())
        .color(theme.text_tertiary()),
    );
    let local = state.local_root.display().to_string();
    let remote = state.remote_root.display().to_string();
    ui.label(
        egui::RichText::new(format!(
            "{} {}",
            crate::i18n::tr(ctx, "Local", "本机"),
            local
        ))
        .size(theme.font_size_caption())
        .color(theme.text_secondary()),
    )
    .on_hover_text(&local);
    ui.label(
        egui::RichText::new(format!(
            "{} {}",
            crate::i18n::tr(ctx, "Remote", "远端"),
            remote
        ))
        .size(theme.font_size_caption())
        .color(theme.text_secondary()),
    )
    .on_hover_text(&remote);

    let (n_left, n_right, n_conflict) = count_kinds(&state.rows);
    ui.label(
        egui::RichText::new(format!(
            "{} {n_left} · {} {n_right} · {} {n_conflict}",
            crate::i18n::tr(ctx, "Local only", "仅本机"),
            crate::i18n::tr(ctx, "Remote only", "仅远端"),
            crate::i18n::tr(ctx, "Conflict", "冲突"),
        ))
        .size(theme.font_size_caption())
        .color(theme.text_secondary()),
    );

    match state.phase {
        DiffPhase::Scanning => {
            let n = state.scanned.load(Ordering::Relaxed);
            ui.label(
                egui::RichText::new(format!(
                    "{} {n}",
                    crate::i18n::tr(ctx, "Comparing… scanned", "正在对比… 已扫描")
                ))
                .size(theme.font_size_caption())
                .color(theme.color_status_warning_text()),
            );
        }
        DiffPhase::Syncing => {
            ui.label(
                egui::RichText::new(crate::i18n::tr(
                    ctx,
                    "Syncing… progress is in the transfer strip below.",
                    "正在同步… 进度在下方传输条.",
                ))
                .size(theme.font_size_caption())
                .color(theme.color_status_warning_text()),
            );
        }
        _ => {}
    }
    if let Some(err) = &state.error {
        ui.label(
            egui::RichText::new(err)
                .size(theme.font_size_caption())
                .color(theme.red_color()),
        );
    }
    if let Some(note) = &state.skip_note {
        ui.label(
            egui::RichText::new(note)
                .size(theme.font_size_caption())
                .color(theme.color_status_warning_text()),
        );
    }

    let ready = state.phase == DiffPhase::Ready && !busy;
    let has_checked = state
        .rows
        .iter()
        .any(|row| row.kind != DiffKind::BothDiffer && state.checked.contains(&row.rel));
    let has_rows = !state.rows.is_empty();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_panel_gap();
        if crate::ui::chrome::panel_action_button_ex(
            ui,
            theme,
            crate::i18n::tr(ctx, "Back", "返回"),
            true,
        )
        .clicked()
        {
            action = Some(DiffUiAction::Close);
        }
        if crate::ui::chrome::panel_action_button_ex(
            ui,
            theme,
            crate::i18n::tr(ctx, "Compare again", "重新对比"),
            ready,
        )
        .clicked()
            && ready
        {
            action = Some(DiffUiAction::Rescan);
        }
        if matches!(state.phase, DiffPhase::Scanning | DiffPhase::Syncing)
            && crate::ui::chrome::panel_action_button_ex(
                ui,
                theme,
                crate::i18n::tr(ctx, "Cancel", "取消"),
                true,
            )
            .clicked()
        {
            action = Some(DiffUiAction::Cancel);
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_panel_gap();
        if sync_button(
            ui,
            theme,
            crate::i18n::tr(ctx, "Checked to remote", "勾选到远端"),
            ready && has_checked,
        ) {
            action = Some(DiffUiAction::Sync {
                direction: crate::core::sftp_diff::SyncDirection::LocalToRemote,
                all: false,
            });
        }
        if sync_button(
            ui,
            theme,
            crate::i18n::tr(ctx, "Checked to local", "勾选到本机"),
            ready && has_checked,
        ) {
            action = Some(DiffUiAction::Sync {
                direction: crate::core::sftp_diff::SyncDirection::RemoteToLocal,
                all: false,
            });
        }
        if crate::ui::chrome::panel_action_button_ex(
            ui,
            theme,
            crate::i18n::tr(ctx, "All to remote", "全部到远端"),
            ready && has_rows,
        )
        .clicked()
            && ready
            && has_rows
        {
            action = Some(DiffUiAction::Sync {
                direction: crate::core::sftp_diff::SyncDirection::LocalToRemote,
                all: true,
            });
        }
        if crate::ui::chrome::panel_action_button_ex(
            ui,
            theme,
            crate::i18n::tr(ctx, "All to local", "全部到本机"),
            ready && has_rows,
        )
        .clicked()
            && ready
            && has_rows
        {
            action = Some(DiffUiAction::Sync {
                direction: crate::core::sftp_diff::SyncDirection::RemoteToLocal,
                all: true,
            });
        }
    });

    if ready && has_rows {
        ui.horizontal_wrapped(|ui| {
            if crate::ui::chrome::panel_action_button_ex(
                ui,
                theme,
                crate::i18n::tr(ctx, "Select syncable", "全选可同步"),
                true,
            )
            .clicked()
            {
                state.checked = default_checks(&state.rows);
            }
            if crate::ui::chrome::panel_action_button_ex(
                ui,
                theme,
                crate::i18n::tr(ctx, "Select none", "全不选"),
                true,
            )
            .clicked()
            {
                state.checked.clear();
            }
            filter_chip(
                ui,
                state,
                DiffFilter::All,
                crate::i18n::tr(ctx, "All", "全部"),
            );
            filter_chip(
                ui,
                state,
                DiffFilter::OnlyLocal,
                &format!("{} {n_left}", crate::i18n::tr(ctx, "Local only", "仅本机")),
            );
            filter_chip(
                ui,
                state,
                DiffFilter::OnlyRemote,
                &format!(
                    "{} {n_right}",
                    crate::i18n::tr(ctx, "Remote only", "仅远端")
                ),
            );
            filter_chip(
                ui,
                state,
                DiffFilter::Conflict,
                &format!("{} {n_conflict}", crate::i18n::tr(ctx, "Conflict", "冲突")),
            );
        });
    }

    let filtered: Vec<usize> = state
        .rows
        .iter()
        .enumerate()
        .filter(|(_, row)| state.filter.matches(row.kind))
        .map(|(idx, _)| idx)
        .collect();
    if state.phase == DiffPhase::Ready && state.rows.is_empty() && state.error.is_none() {
        ui.label(
            egui::RichText::new(crate::i18n::tr(ctx, "No differences.", "没有差异."))
                .size(theme.font_size_caption())
                .color(theme.text_secondary()),
        );
    }

    let row_h = theme
        .size_file_list_row_h()
        .max(ui.spacing().interact_size.y);
    let list_h = (ui.available_height() - super::FLUX_XFER_STRIP_RESERVE).max(80.0);
    let interactive = state.phase == DiffPhase::Ready && !busy;
    egui::ScrollArea::vertical()
        .id_source("sftp_dir_diff_rows")
        .auto_shrink([false, false])
        .max_height(list_h)
        .show_rows(ui, row_h, filtered.len(), |ui, range| {
            for slot in range {
                let idx = filtered[slot];
                let (rel, kind, detail, conflict) = {
                    let row = &state.rows[idx];
                    (
                        row.rel.clone(),
                        row.kind,
                        row_detail(ctx, row),
                        row.kind == DiffKind::BothDiffer,
                    )
                };
                let on = state.checked.contains(&rel);
                ui.horizontal(|ui| {
                    if conflict {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::hover());
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "!",
                            egui::FontId::proportional(theme.font_size_caption()),
                            theme.color_status_warning_text(),
                        );
                    } else {
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(16.0, 16.0), Sense::click());
                        let stroke = if interactive {
                            theme.text_primary()
                        } else {
                            theme.text_tertiary()
                        };
                        ui.painter()
                            .rect_stroke(rect, 2.0, egui::Stroke::new(1.0_f32, stroke));
                        if on {
                            ui.painter().rect_filled(
                                rect.shrink(3.0),
                                1.0,
                                if interactive {
                                    theme.accent_color()
                                } else {
                                    theme.text_tertiary()
                                },
                            );
                        }
                        if interactive && resp.clicked() {
                            toggle_check(&mut state.checked, &rel, on);
                        }
                    }
                    ui.label(
                        egui::RichText::new(kind_label(ctx, kind))
                            .size(theme.font_size_caption())
                            .color(kind_color(theme, kind)),
                    );
                    let tip = format!("{rel}\n{detail}");
                    let path_w = (ui.available_width() - 96.0).max(24.0);
                    let path_resp = ui
                        .add_sized(
                            egui::vec2(path_w, row_h),
                            egui::Label::new(
                                egui::RichText::new(&rel)
                                    .size(theme.font_size_caption())
                                    .color(theme.text_primary()),
                            )
                            .sense(Sense::click())
                            .truncate(true),
                        )
                        .on_hover_text(tip);
                    if interactive && !conflict && path_resp.clicked() {
                        let now = state.checked.contains(&rel);
                        toggle_check(&mut state.checked, &rel, now);
                    }
                    ui.label(
                        egui::RichText::new(detail)
                            .size(theme.font_size_caption())
                            .color(theme.text_tertiary()),
                    );
                });
            }
        });
    action
}

fn toggle_check(checked: &mut HashSet<String>, rel: &str, was_on: bool) {
    if was_on {
        checked.remove(rel);
    } else {
        checked.insert(rel.to_string());
    }
}

fn sync_button(ui: &mut egui::Ui, theme: &Theme, label: &str, enabled: bool) -> bool {
    crate::ui::chrome::panel_action_primary_button_ex(ui, theme, label, enabled).clicked()
        && enabled
}

fn filter_chip(ui: &mut egui::Ui, state: &mut DirDiffUi, filter: DiffFilter, label: &str) {
    if ui.selectable_label(state.filter == filter, label).clicked() {
        state.filter = filter;
    }
}

impl DiffFilter {
    fn matches(self, kind: DiffKind) -> bool {
        match self {
            DiffFilter::All => true,
            DiffFilter::OnlyLocal => kind == DiffKind::OnlyLeft,
            DiffFilter::OnlyRemote => kind == DiffKind::OnlyRight,
            DiffFilter::Conflict => kind == DiffKind::BothDiffer,
        }
    }
}

fn count_kinds(rows: &[DiffRow]) -> (usize, usize, usize) {
    let mut left = 0;
    let mut right = 0;
    let mut conflict = 0;
    for row in rows {
        match row.kind {
            DiffKind::OnlyLeft => left += 1,
            DiffKind::OnlyRight => right += 1,
            DiffKind::BothDiffer => conflict += 1,
        }
    }
    (left, right, conflict)
}

fn kind_label(ctx: &egui::Context, kind: DiffKind) -> &'static str {
    match kind {
        DiffKind::OnlyLeft => crate::i18n::tr(ctx, "Local only", "仅本机"),
        DiffKind::OnlyRight => crate::i18n::tr(ctx, "Remote only", "仅远端"),
        DiffKind::BothDiffer => crate::i18n::tr(ctx, "Conflict", "冲突"),
    }
}

fn kind_color(theme: &Theme, kind: DiffKind) -> Color32 {
    match kind {
        DiffKind::OnlyLeft => theme.accent_color(),
        DiffKind::OnlyRight => theme.color_status_positive_text(),
        DiffKind::BothDiffer => theme.color_status_warning_text(),
    }
}

fn row_detail(ctx: &egui::Context, row: &DiffRow) -> String {
    if row.kind == DiffKind::BothDiffer && row.left_is_dir != row.right_is_dir {
        return crate::i18n::tr(ctx, "File and folder", "文件和目录").to_string();
    }
    if row.left_is_dir || row.right_is_dir {
        return crate::i18n::tr(ctx, "Empty folder", "空目录").to_string();
    }
    match row.kind {
        DiffKind::BothDiffer => format!(
            "{} / {}",
            format_bytes(row.left_size.unwrap_or(0)),
            format_bytes(row.right_size.unwrap_or(0))
        ),
        DiffKind::OnlyLeft => format_bytes(row.left_size.unwrap_or(0)),
        DiffKind::OnlyRight => format_bytes(row.right_size.unwrap_or(0)),
    }
}
