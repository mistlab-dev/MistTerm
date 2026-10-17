//! SFTP 侧栏：本地 / 远端双栏文件浏览，表格式列表，经 shell 泵队列传输。

#[path = "sftp_file_table.rs"]
mod sftp_file_table;
#[path = "sftp_xfer.rs"]
mod sftp_xfer;
#[path = "sftp_bookmarks.rs"]
mod sftp_bookmarks;
#[path = "sftp_tree.rs"]
mod sftp_tree;
#[path = "sftp_select.rs"]
mod sftp_select;

use crate::core::{AuditCategory, AuditEvent, AuditLogger, AuditOutcome};
use crate::i18n::UiLanguage;
use crate::ssh::{
    SftpClient, SftpEntry, SshSessionHandle, TransferProgress, TransferResult, WriteMode,
};
use crate::ui::layout_util;
use crate::ui::terminal::TerminalView;
use crate::ui::theme::Theme;
use chrono::Utc;
use eframe::egui::{self, Sense};
use crate::ui::file_dialog::FileDialog;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use sftp_file_table::{
    FileSortState, FileTableCols, LocalEntry, classify_file_kind, format_file_mtime,
    paint_file_table_header, paint_file_table_row, sort_local_entries, sort_remote_entries,
    system_time_to_utc,
};
use sftp_bookmarks::SftpBookmarkStore;
use sftp_select::SelectionSet;
use sftp_tree::{collect_local_tree, looks_like_text_name, preview_needs_confirm};
use sftp_xfer::{
    format_bytes, paint_xfer_strip, resolve_write_mode_mtime, OverwritePolicy,
    XferKind, XferQueue, XferState, XferStripAction,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

enum SftpJobResult {
    Listed {
        dir: PathBuf,
        result: Result<Vec<SftpEntry>, String>,
    },
    Msg(Result<String, String>),
    TransferDone {
        id: u64,
        kind: XferKind,
        remote: PathBuf,
        result: Result<TransferResult, String>,
    },
    UploadTreeReady {
        files: Vec<(PathBuf, PathBuf)>,
    },
    DownloadTreeReady {
        files: Vec<(PathBuf, PathBuf)>,
    },
    TreeCollectErr(String),
    RemoveMany(Result<String, String>),
    ChmodDone(Result<String, String>),
    EditPrepared {
        remote: PathBuf,
        track_edits: bool,
        result: Result<(PathBuf, u64, u64), String>,
    },
    EditConflictCheck {
        remote: PathBuf,
        local: PathBuf,
        session_size: u64,
        session_mtime: u64,
        result: Result<(u64, u64), String>,
    },
}

#[derive(Clone)]
struct RemoteEditSession {
    remote: PathBuf,
    local: PathBuf,
    remote_size: u64,
    remote_mtime: u64,
    local_mtime_at_open: Option<std::time::SystemTime>,
}

#[derive(Clone)]
enum PendingDeletePrompt {
    One(PathBuf),
    Many(Vec<PathBuf>),
}

/// 传输目标已存在时的待确认操作。
enum PendingOverwrite {
    Upload {
        remote: PathBuf,
        local: PathBuf,
        source_size: u64,
        dest_size: u64,
    },
    Download {
        remote: PathBuf,
        local: PathBuf,
        source_size: u64,
        dest_size: u64,
    },
    UploadMany {
        cwd: PathBuf,
        locals: Vec<PathBuf>,
        conflict_names: Vec<String>,
    },
}

struct XferProgressEvent {
    id: u64,
    done: u64,
    total: u64,
}

pub struct SftpPanel {
    /// 远端当前目录
    cwd: PathBuf,
    entries: Vec<SftpEntry>,
    path_edit: String,
    remote_selected: SelectionSet,
    /// 本机当前目录
    local_cwd: PathBuf,
    local_entries: Vec<LocalEntry>,
    local_path_edit: String,
    local_selected: SelectionSet,
    local_filter: String,
    remote_filter: String,
    bookmarks: SftpBookmarkStore,
    host_key: String,
    remote_edit: Option<RemoteEditSession>,
    edit_dirty: bool,
    pending_edit_upload: Option<(PathBuf, PathBuf)>,
    pending_edit_conflict: Option<(PathBuf, PathBuf)>,
    show_chmod_dialog: bool,
    chmod_octal: String,
    chmod_recursive: bool,
    pending_chmod_recursive_confirm: bool,
    tree_cancel: Option<Arc<AtomicBool>>,
    tree_busy: bool,
    deferred_upload_files: Vec<(PathBuf, PathBuf)>,
    deferred_download_files: Vec<(PathBuf, PathBuf)>,
    local_list_err: Option<String>,
    list_err: Option<String>,
    /// 待写入底栏 `status_message` 的成功/失败提示(不在面板内渲染)。
    pending_status_ok: Option<String>,
    pending_status_err: Option<String>,
    busy: bool,
    rx: Option<Receiver<SftpJobResult>>,
    /// 传输进度（与 SessionJob 并行试投，UI 每帧 drain）。
    xfer_progress_rx: Option<Receiver<XferProgressEvent>>,
    xfer: XferQueue,
    mkdir_name: String,
    show_mkdir_dialog: bool,
    pending_delete: Option<PendingDeletePrompt>,
    /// 大文件打开前的确认（先下载到临时文件再显示）。
    pending_large_open: Option<SftpEntry>,
    pending_overwrite: Option<PendingOverwrite>,
    show_rename_dialog: bool,
    rename_from: Option<PathBuf>,
    rename_name: String,
    pending_refresh_after_op: bool,
    /// 面板打开后与切换标签时为 true，触发一次列表加载
    pending_auto_list: bool,
    /// 后台操作成功后待写入审计
    pending_audit: Option<(&'static str, String)>,
    /// GUI 自动化：busy 时延后的下载任务
    deferred_gui_download: Option<(PathBuf, PathBuf)>,
    /// 右 dock 槽位(用于 Central 之后前景重绘)
    last_panel_slot_rect: Option<egui::Rect>,
    local_sort: FileSortState,
    remote_sort: FileSortState,
    local_path_editing: bool,
    remote_path_editing: bool,
    pending_terminal_cmd: Option<String>,
    flux_details_open: bool,
}

impl Default for SftpPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl SftpPanel {
    /// 右 dock 正文区可用宽(与其它侧栏并排时须随槽位收缩)。
    fn dock_field_width(ui: &mut egui::Ui) -> f32 {
        layout_util::set_width_to_available(ui);
        layout_util::finite_content_width_inset(ui, 0.0, 64.0, ui.available_width())
    }

    fn begin_dock_row(ui: &mut egui::Ui) -> f32 {
        layout_util::set_width_to_available(ui);
        let w = ui.available_width();
        ui.set_max_width(w);
        w
    }

    /// 本机单选且为文件 → (远端目标路径, 本机源路径)。
    fn local_upload_job(&self) -> Option<(PathBuf, PathBuf)> {
        if self.local_selected.len() != 1 {
            return None;
        }
        let p = self.local_selected.primary()?.clone();
        self.local_entries
            .iter()
            .find(|e| e.path == p && !e.is_dir)
            .map(|e| (self.cwd.join(&e.name), e.path.clone()))
    }

    /// 远端单选且为文件 → (远端源路径, 本机目标路径)。
    fn remote_download_job(&self) -> Option<(PathBuf, PathBuf)> {
        if self.remote_selected.len() != 1 {
            return None;
        }
        let p = self.remote_selected.primary()?.clone();
        self.entries
            .iter()
            .find(|e| e.path == p && !e.is_dir)
            .map(|e| (e.path.clone(), self.local_cwd.join(&e.name)))
    }

    fn has_local_upload_selection(&self) -> bool {
        !self.busy && !self.tree_busy && !self.local_selected.is_empty()
    }

    fn has_remote_download_selection(&self) -> bool {
        !self.busy && !self.tree_busy && !self.remote_selected.is_empty()
    }

    fn filtered_local_entries(&self) -> Vec<&LocalEntry> {
        let f = self.local_filter.trim().to_lowercase();
        self.local_entries
            .iter()
            .filter(|e| f.is_empty() || e.name.to_lowercase().contains(&f))
            .collect()
    }

    fn filtered_remote_entries(&self) -> Vec<&SftpEntry> {
        let f = self.remote_filter.trim().to_lowercase();
        self.entries
            .iter()
            .filter(|e| f.is_empty() || e.name.to_lowercase().contains(&f))
            .collect()
    }

    #[allow(dead_code)] // 供后续快捷键/批量操作复用
    fn visible_local_paths(&self) -> Vec<PathBuf> {
        self.filtered_local_entries()
            .into_iter()
            .map(|e| e.path.clone())
            .collect()
    }

    #[allow(dead_code)]
    fn visible_remote_paths(&self) -> Vec<PathBuf> {
        self.filtered_remote_entries()
            .into_iter()
            .map(|e| e.path.clone())
            .collect()
    }

    fn sync_local_selection_after_list(&mut self) {
        let visible: std::collections::HashSet<_> =
            self.local_entries.iter().map(|e| e.path.clone()).collect();
        self.local_selected
            .items
            .retain(|p| visible.contains(p));
        if self.local_selected.anchor.as_ref().is_some_and(|a| !visible.contains(a)) {
            self.local_selected.anchor = self.local_selected.items.iter().next().cloned();
        }
    }

    fn sync_remote_selection_after_list(&mut self) {
        let visible: std::collections::HashSet<_> =
            self.entries.iter().map(|e| e.path.clone()).collect();
        self.remote_selected
            .items
            .retain(|p| visible.contains(p));
        if self.remote_selected.anchor.as_ref().is_some_and(|a| !visible.contains(a)) {
            self.remote_selected.anchor = self.remote_selected.items.iter().next().cloned();
        }
    }

    fn parse_octal_mode(input: &str) -> Option<u32> {
        let s = input.trim();
        if s.is_empty() {
            return None;
        }
        u32::from_str_radix(s, 8).ok().map(|m| m & 0o777)
    }

    fn permissions_display_to_octal(perm: &str) -> Option<String> {
        let triplet = if perm.len() >= 10 {
            &perm[1..10]
        } else if perm.len() == 9 {
            perm
        } else {
            return None;
        };
        if triplet.len() != 9 {
            return None;
        }
        let mut mode = 0u32;
        for (i, c) in triplet.chars().enumerate() {
            let bit = match c {
                'r' if i % 3 == 0 => 4,
                'w' if i % 3 == 1 => 2,
                'x' if i % 3 == 2 => 1,
                '-' => 0,
                _ => return None,
            };
            let shift = 6 - (i / 3) * 3 - (2 - i % 3);
            mode |= bit << shift;
        }
        Some(format!("{mode:o}"))
    }

    fn edit_temp_path(remote: &Path, name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("mistterm_sftp_edit");
        let _ = std::fs::create_dir_all(&dir);
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let stem = if safe.is_empty() { "edit.txt".into() } else { safe };
        let mut path = dir.join(&stem);
        if path.exists() {
            let hash = format!("{:x}", remote.to_string_lossy().len());
            path = dir.join(format!("{stem}.{hash}"));
        }
        path
    }

    fn poll_remote_edit(&mut self) {
        let Some(session) = self.remote_edit.as_ref() else {
            self.edit_dirty = false;
            return;
        };
        let Ok(meta) = std::fs::metadata(&session.local) else {
            return;
        };
        let Ok(m) = meta.modified() else {
            return;
        };
        self.edit_dirty = session
            .local_mtime_at_open
            .map(|open| m != open)
            .unwrap_or(true);
    }

    fn upload_selected_local(&mut self, handle: &SshSessionHandle, ctx: &egui::Context) {
        let roots: Vec<PathBuf> = self.local_selected.paths_vec();
        if roots.is_empty() {
            return;
        }
        let needs_tree = roots.len() > 1
            || roots.iter().any(|p| {
                self.local_entries
                    .iter()
                    .find(|e| e.path == *p)
                    .is_some_and(|e| e.is_dir)
                    || p.is_dir()
            });
        if !needs_tree {
            if roots.len() == 1 {
                if let Some((r, l)) = self.local_upload_job() {
                    self.request_upload(handle, r, l, ctx);
                    return;
                }
            }
            self.request_upload_many(handle, self.cwd.clone(), roots, ctx);
            return;
        }
        self.spawn_collect_local_upload(handle, roots, self.cwd.clone(), ctx);
    }

    fn download_selected_remote(&mut self, handle: &SshSessionHandle, ctx: &egui::Context) {
        let roots: Vec<PathBuf> = self.remote_selected.paths_vec();
        if roots.is_empty() {
            return;
        }
        let all_files = roots.iter().all(|p| {
            self.entries.iter().any(|e| e.path == *p && !e.is_dir)
        });
        if roots.len() == 1 && all_files {
            if let Some((r, l)) = self.remote_download_job() {
                self.request_download(handle, r, l, ctx);
                return;
            }
        }
        self.spawn_collect_remote_download(handle, roots, self.local_cwd.clone(), ctx);
    }

    fn spawn_collect_local_upload(
        &mut self,
        handle: &SshSessionHandle,
        roots: Vec<PathBuf>,
        remote_base: PathBuf,
        ctx: &egui::Context,
    ) {
        if self.busy || self.rx.is_some() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.tree_cancel = Some(cancel.clone());
        self.tree_busy = true;
        self.busy = true;
        self.enqueue(handle, ctx, move |session| {
            let outcome = (|| -> Result<Vec<(PathBuf, PathBuf)>, String> {
                let mut files = Vec::new();
                let mut mkdirs = Vec::new();
                for root in roots {
                    for (local, rel, empty) in collect_local_tree(&root, Some(&cancel))? {
                        let remote = remote_base.join(&rel);
                        if empty {
                            mkdirs.push(remote);
                        } else {
                            files.push((local, remote));
                        }
                    }
                }
                let client = SftpClient::new(session)?;
                for d in mkdirs {
                    if cancel.load(Ordering::Relaxed) {
                        return Ok(Vec::new());
                    }
                    client.mkdir_p(&d)?;
                }
                Ok(files)
            })();
            match outcome {
                Ok(files) => SftpJobResult::UploadTreeReady { files },
                Err(e) => SftpJobResult::TreeCollectErr(e),
            }
        });
    }

    fn spawn_collect_remote_download(
        &mut self,
        handle: &SshSessionHandle,
        roots: Vec<PathBuf>,
        local_base: PathBuf,
        ctx: &egui::Context,
    ) {
        if self.busy || self.rx.is_some() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.tree_cancel = Some(cancel.clone());
        self.tree_busy = true;
        self.busy = true;
        self.enqueue(handle, ctx, move |session| {
            let outcome = (|| -> Result<Vec<(PathBuf, PathBuf)>, String> {
                let client = SftpClient::new(session)?;
                let mut files = Vec::new();
                let mut local_mkdirs = Vec::new();
                for root in roots {
                    for (remote, rel, empty) in client.collect_tree(&root, Some(&cancel))? {
                        let local = local_base.join(&rel);
                        if empty {
                            local_mkdirs.push(local);
                        } else {
                            files.push((remote, local));
                        }
                    }
                }
                for d in local_mkdirs {
                    if cancel.load(Ordering::Relaxed) {
                        return Ok(Vec::new());
                    }
                    std::fs::create_dir_all(&d)
                        .map_err(|e| format!("mkdir {}: {e}", d.display()))?;
                }
                Ok(files)
            })();
            match outcome {
                Ok(files) => SftpJobResult::DownloadTreeReady { files },
                Err(e) => SftpJobResult::TreeCollectErr(e),
            }
        });
    }

    fn cancel_tree_collect(&mut self) {
        if let Some(c) = &self.tree_cancel {
            c.store(true, Ordering::Relaxed);
        }
    }

    /// 双击或「打开」：小文件直接下到临时目录再打开；大文件先问。
    fn request_open_remote_file(
        &mut self,
        handle: &SshSessionHandle,
        entry: &SftpEntry,
        ctx: &egui::Context,
    ) {
        if entry.is_dir {
            return;
        }
        if preview_needs_confirm(entry.size) {
            self.pending_large_open = Some(entry.clone());
            return;
        }
        self.start_remote_edit(handle, entry, ctx);
    }

    fn start_remote_edit(
        &mut self,
        handle: &SshSessionHandle,
        entry: &SftpEntry,
        ctx: &egui::Context,
    ) {
        if self.busy || self.rx.is_some() {
            return;
        }
        if entry.is_dir {
            return;
        }
        let remote = entry.path.clone();
        let local = Self::edit_temp_path(&remote, &entry.name);
        let track_edits = looks_like_text_name(&entry.name);
        self.busy = true;
        self.enqueue(handle, ctx, move |session| {
            let result = (|| -> Result<(PathBuf, u64, u64), String> {
                let client = SftpClient::new(session)?;
                let size = client.remote_size(&remote).unwrap_or(0);
                let mtime = client.remote_mtime(&remote).unwrap_or(0);
                client.download_ex(
                    &remote,
                    &local,
                    WriteMode::Overwrite,
                    None,
                    &mut |_| {},
                )?;
                Ok((local, size, mtime))
            })();
            SftpJobResult::EditPrepared {
                remote,
                track_edits,
                result,
            }
        });
    }

    fn request_edit_upload(&mut self, handle: &SshSessionHandle, ctx: &egui::Context) {
        let Some(session) = self.remote_edit.clone() else {
            return;
        };
        if self.busy || self.rx.is_some() {
            return;
        }
        self.busy = true;
        let remote = session.remote.clone();
        let local = session.local.clone();
        let session_size = session.remote_size;
        let session_mtime = session.remote_mtime;
        self.enqueue(handle, ctx, move |session| {
            let result = (|| -> Result<(u64, u64), String> {
                let client = SftpClient::new(session)?;
                let size = client.remote_size(&remote).unwrap_or(0);
                let mtime = client.remote_mtime(&remote).unwrap_or(0);
                Ok((size, mtime))
            })();
            SftpJobResult::EditConflictCheck {
                remote,
                local,
                session_size,
                session_mtime,
                result,
            }
        });
    }

    fn spawn_chmod(
        &mut self,
        handle: &SshSessionHandle,
        paths: Vec<PathBuf>,
        mode: u32,
        recursive: bool,
        ctx: &egui::Context,
    ) {
        if self.busy || paths.is_empty() {
            return;
        }
        self.busy = true;
        let cancel = Arc::new(AtomicBool::new(false));
        let lang = crate::i18n::language(ctx);
        self.enqueue(handle, ctx, move |session| {
            let msg = (|| -> Result<String, String> {
                let client = SftpClient::new(session)?;
                let mut n = 0usize;
                for p in &paths {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    if recursive {
                        n += client.chmod_recursive(p, mode, Some(&cancel))?;
                    } else {
                        client.chmod(p, mode)?;
                        n += 1;
                    }
                }
                Ok(match lang {
                    UiLanguage::En => format!("Updated permissions on {n} item(s)"),
                    UiLanguage::Zh => format!("已更新 {n} 项权限"),
                })
            })();
            SftpJobResult::ChmodDone(msg)
        });
    }

    fn spawn_remove_many(
        &mut self,
        handle: &SshSessionHandle,
        paths: Vec<PathBuf>,
        ctx: &egui::Context,
    ) {
        if self.busy || paths.is_empty() {
            return;
        }
        self.busy = true;
        self.pending_audit = Some(("sftp.delete", format!("{} items", paths.len())));
        let lang = crate::i18n::language(ctx);
        self.enqueue(handle, ctx, move |session| {
            let msg = (|| -> Result<String, String> {
                let client = SftpClient::new(session)?;
                let mut n = 0usize;
                for p in &paths {
                    client.remove(p)?;
                    n += 1;
                }
                Ok(match lang {
                    UiLanguage::En => format!("Deleted {n} remote item(s)"),
                    UiLanguage::Zh => format!("已删除 {n} 个远端项"),
                })
            })();
            SftpJobResult::RemoveMany(msg)
        });
    }

    fn paint_bookmark_row(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        side_local: bool,
        current_path: PathBuf,
        navigate: &mut Option<PathBuf>,
    ) {
        let hit = theme.size_panel_header_control_h();
        let is_bookmarked = if side_local {
            self.bookmarks
                .local
                .iter()
                .any(|p| PathBuf::from(p) == current_path)
        } else {
            self.bookmarks
                .remote_bookmarks(&self.host_key)
                .iter()
                .any(|p| PathBuf::from(p) == current_path)
        };
        let star = if is_bookmarked { "★" } else { "☆" };
        let tip = crate::i18n::tr(ctx, "Bookmark this path", "收藏此路径");
        let star_label = crate::i18n::tr(ctx, "Save", "收藏");
        let star_color = if is_bookmarked {
            theme.accent_color()
        } else {
            theme.text_secondary()
        };
        let star_font = egui::FontId::proportional(theme.font_size_caption());
        let star_galley = ui.fonts(|f| f.layout_no_wrap(star_label.to_string(), star_font, star_color));
        let star_w = hit + 2.0 + star_galley.size().x + 4.0;
        let (star_rect, star_resp) =
            ui.allocate_exact_size(egui::vec2(star_w, hit), egui::Sense::click());
        if star_resp.hovered() {
            ui.painter().rect_filled(
                star_rect,
                theme.radius_list_item(),
                theme.color_tab_bar_icon_btn_hover_fill(),
            );
        }
        ui.painter().text(
            egui::pos2(star_rect.left() + hit * 0.5, star_rect.center().y),
            egui::Align2::CENTER_CENTER,
            star,
            egui::FontId::proportional(theme.font_size_body()),
            star_color,
        );
        ui.painter().galley(
            egui::pos2(
                star_rect.left() + hit + 2.0,
                star_rect.center().y - star_galley.size().y * 0.5,
            ),
            star_galley,
        );
        if star_resp.on_hover_text(tip).clicked() {
            if side_local {
                if is_bookmarked {
                    self.bookmarks
                        .remove_local_bookmark(&current_path.to_string_lossy());
                } else {
                    self.bookmarks.add_local_bookmark(&current_path);
                }
            } else if is_bookmarked {
                self.bookmarks.remove_remote_bookmark(
                    &self.host_key,
                    &current_path.to_string_lossy(),
                );
            } else {
                self.bookmarks
                    .add_remote_bookmark(&self.host_key, &current_path);
            }
        }
        let jump_lbl = crate::i18n::tr(ctx, "Go to", "前往");
        let jump_tip = crate::i18n::tr(
            ctx,
            "Open a bookmarked or recent folder",
            "打开收藏或最近去过的目录",
        );
        let path_font = egui::FontId::proportional(theme.font_size_caption());
        let path_galley = ui.fonts(|f| {
            f.layout_no_wrap(jump_lbl.to_string(), path_font, theme.text_secondary())
        });
        let chevron = 8.0;
        let path_w = path_galley.size().x + 6.0 + chevron + 8.0;
        let (path_rect, path_resp) =
            ui.allocate_exact_size(egui::vec2(path_w, hit), egui::Sense::click());
        if path_resp.hovered() {
            ui.painter().rect_filled(
                path_rect,
                theme.radius_list_item(),
                theme.color_tab_bar_icon_btn_hover_fill(),
            );
        }
        let text_x = path_rect.left() + 4.0;
        ui.painter().galley(
            egui::pos2(text_x, path_rect.center().y - path_galley.size().y * 0.5),
            path_galley.clone(),
        );
        let tip = egui::pos2(text_x + path_galley.size().x + 6.0 + chevron * 0.5, path_rect.center().y + 1.0);
        let stroke = egui::Stroke::new(1.4, theme.text_secondary());
        ui.painter().line_segment(
            [tip + egui::vec2(-chevron * 0.45, -2.0), tip],
            stroke,
        );
        ui.painter().line_segment(
            [tip + egui::vec2(chevron * 0.45, -2.0), tip],
            stroke,
        );
        let popup_id = ui.make_persistent_id(if side_local {
            "sftp_local_paths"
        } else {
            "sftp_remote_paths"
        });
        let just_toggled = path_resp.on_hover_text(jump_tip).clicked();
        if just_toggled {
            ui.memory_mut(|m| m.toggle_popup(popup_id));
        }
        if ui.memory(|m| m.is_popup_open(popup_id)) {
            let tidy = |items: Vec<String>| -> Vec<String> {
                let mut out = Vec::new();
                let mut seen = std::collections::HashSet::new();
                for raw in items {
                    let trimmed = raw.trim();
                    if trimmed.is_empty() || trimmed == "." {
                        continue;
                    }
                    let key = if side_local {
                        trimmed.to_string()
                    } else {
                        Self::normalize_remote_path(PathBuf::from(trimmed))
                            .to_string_lossy()
                            .into_owned()
                    };
                    if key.is_empty() || key == "." || !seen.insert(key.clone()) {
                        continue;
                    }
                    out.push(key);
                }
                out
            };
            let bookmarks: Vec<String> = tidy(if side_local {
                self.bookmarks.local.clone()
            } else {
                self.bookmarks.remote_bookmarks(&self.host_key).to_vec()
            });
            let recent: Vec<String> = tidy(if side_local {
                self.bookmarks.recent_local.clone()
            } else {
                self.bookmarks.recent_remote(&self.host_key).to_vec()
            });
            let fav = crate::i18n::tr(ctx, "Bookmarks", "收藏").to_string();
            let recent_title = crate::i18n::tr(ctx, "Recent", "最近").to_string();
            let empty_hint = crate::i18n::tr(
                ctx,
                "No saved folders yet. Use the star to bookmark this one.",
                "还没有可前往的目录，点旁边的星可以记下当前目录",
            )
            .to_string();
            let row_label = |p: &str| -> String {
                let n = p.chars().count();
                if n <= 36 {
                    p.to_string()
                } else {
                    let head: String = p.chars().take(16).collect();
                    let tail: String = p.chars().skip(n - 16).collect();
                    format!("{head}…{tail}")
                }
            };
            let widest = ui.fonts(|f| {
                let measure = |text: &str, size: f32| {
                    f.layout_no_wrap(
                        text.to_string(),
                        egui::FontId::proportional(size),
                        egui::Color32::WHITE,
                    )
                    .size()
                    .x
                };
                let mut w = if bookmarks.is_empty() && recent.is_empty() {
                    measure(&empty_hint, theme.font_size_caption())
                } else {
                    0.0
                };
                if !bookmarks.is_empty() {
                    w = w.max(measure(&fav, theme.font_size_small()));
                }
                if !recent.is_empty() {
                    w = w.max(measure(&recent_title, theme.font_size_small()));
                }
                for p in bookmarks.iter().chain(recent.iter()) {
                    w = w.max(measure(&row_label(p), theme.font_size_body()));
                }
                w
            });
            let popup_w = (widest + 36.0).clamp(path_rect.width(), 320.0);
            let screen = ui.ctx().screen_rect();
            let mut pos = egui::pos2(path_rect.left(), path_rect.bottom() + 2.0);
            if pos.x + popup_w > screen.right() - 8.0 {
                pos.x = (screen.right() - 8.0 - popup_w).max(screen.left() + 8.0);
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
                        ui.style_mut().wrap = Some(false);
                        if bookmarks.is_empty() && recent.is_empty() {
                            ui.label(
                                egui::RichText::new(&empty_hint)
                                    .size(theme.font_size_caption())
                                    .color(theme.text_tertiary()),
                            );
                        }
                        let mut paint_section = |ui: &mut egui::Ui, title: &str, items: &[String]| {
                            if items.is_empty() {
                                return;
                            }
                            ui.label(
                                egui::RichText::new(title)
                                    .size(theme.font_size_small())
                                    .color(theme.text_tertiary()),
                            );
                            for p in items {
                                let shown = row_label(p);
                                if ui.selectable_label(false, shown).on_hover_text(p).clicked() {
                                    *navigate = Some(PathBuf::from(p));
                                    ui.memory_mut(|m| m.close_popup());
                                }
                            }
                        };
                        paint_section(ui, &fav, &bookmarks);
                        if !bookmarks.is_empty() && !recent.is_empty() {
                            ui.separator();
                        }
                        paint_section(ui, &recent_title, &recent);
                    });
                });
            if !just_toggled && area.response.clicked_elsewhere() {
                ui.memory_mut(|m| m.close_popup());
            }
        }
    }

    fn remote_basename_exists(&self, name: &str) -> bool {
        self.entries.iter().any(|e| e.name == name)
    }

    fn remote_entry_size(&self, name: &str) -> u64 {
        self.entries
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.size)
            .unwrap_or(0)
    }

    fn enqueue_xfer(
        &mut self,
        handle: &SshSessionHandle,
        kind: XferKind,
        local: PathBuf,
        remote: PathBuf,
        mode: WriteMode,
        ctx: &egui::Context,
    ) {
        let total_hint = match kind {
            XferKind::Upload => std::fs::metadata(&local).map(|m| m.len()).unwrap_or(0),
            XferKind::Download => self
                .entries
                .iter()
                .find(|e| e.path == remote)
                .map(|e| e.size)
                .unwrap_or(0),
        };
        self.xfer.push(kind, local, remote, mode, total_hint);
        self.kick_xfer_queue(handle, ctx);
    }

    /// 上传：按覆盖策略决定询问 / 覆盖 / 跳过 / 续传。
    fn request_upload(
        &mut self,
        handle: &SshSessionHandle,
        remote: PathBuf,
        local: PathBuf,
        ctx: &egui::Context,
    ) {
        let name = remote
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let source_size = std::fs::metadata(&local).map(|m| m.len()).unwrap_or(0);
        let dest_exists = !name.is_empty() && self.remote_basename_exists(&name);
        let dest_size = if dest_exists {
            self.remote_entry_size(&name)
        } else {
            0
        };
        self.resolve_and_enqueue_upload(handle, remote, local, source_size, dest_exists, dest_size, ctx);
    }

    fn resolve_and_enqueue_upload(
        &mut self,
        handle: &SshSessionHandle,
        remote: PathBuf,
        local: PathBuf,
        source_size: u64,
        dest_exists: bool,
        dest_size: u64,
        ctx: &egui::Context,
    ) {
        if !dest_exists || dest_size == 0 {
            self.enqueue_xfer(handle, XferKind::Upload, local, remote, WriteMode::Overwrite, ctx);
            return;
        }
        match self.xfer.policy {
            OverwritePolicy::Ask => {
                self.pending_overwrite = Some(PendingOverwrite::Upload {
                    remote,
                    local,
                    source_size,
                    dest_size,
                });
            }
            OverwritePolicy::OverwriteAll => {
                self.enqueue_xfer(handle, XferKind::Upload, local, remote, WriteMode::Overwrite, ctx);
            }
            OverwritePolicy::SkipAll => {
                self.pending_status_ok = Some(
                    crate::i18n::tr(ctx, "Skipped existing file", "已跳过同名文件").to_string(),
                );
            }
            OverwritePolicy::ResumeAll => {
                if dest_size < source_size {
                    self.enqueue_xfer(handle, XferKind::Upload, local, remote, WriteMode::Resume, ctx);
                } else if dest_size == source_size {
                    self.pending_status_ok = Some(
                        crate::i18n::tr(ctx, "Already complete — skipped", "已完整，已跳过")
                            .to_string(),
                    );
                } else {
                    self.enqueue_xfer(handle, XferKind::Upload, local, remote, WriteMode::Overwrite, ctx);
                }
            }
            OverwritePolicy::IfNewerAll => {
                let src_m = std::fs::metadata(&local)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());
                let entry = remote
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|name| self.entries.iter().find(|e| e.name == name));
                let dst_m = entry.map(|e| e.modified.timestamp().max(0) as u64);
                match resolve_write_mode_mtime(
                    OverwritePolicy::IfNewerAll,
                    dest_exists,
                    dest_size,
                    source_size,
                    src_m,
                    dst_m,
                ) {
                    Some(mode) => {
                        self.enqueue_xfer(handle, XferKind::Upload, local, remote, mode, ctx);
                    }
                    None => {
                        self.pending_status_ok = Some(
                            crate::i18n::tr(ctx, "Already up to date — skipped", "已是最新，已跳过")
                                .to_string(),
                        );
                    }
                }
            }
        }
    }

    /// 下载：按覆盖策略决定询问 / 覆盖 / 跳过 / 续传。
    fn request_download(
        &mut self,
        handle: &SshSessionHandle,
        remote: PathBuf,
        local: PathBuf,
        ctx: &egui::Context,
    ) {
        let source_size = self
            .entries
            .iter()
            .find(|e| e.path == remote)
            .map(|e| e.size)
            .unwrap_or(0);
        let dest_exists = local.exists();
        let dest_size = if dest_exists {
            std::fs::metadata(&local).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        self.resolve_and_enqueue_download(
            handle,
            remote,
            local,
            source_size,
            dest_exists,
            dest_size,
            ctx,
        );
    }

    fn resolve_and_enqueue_download(
        &mut self,
        handle: &SshSessionHandle,
        remote: PathBuf,
        local: PathBuf,
        source_size: u64,
        dest_exists: bool,
        dest_size: u64,
        ctx: &egui::Context,
    ) {
        if !dest_exists || dest_size == 0 {
            self.enqueue_xfer(handle, XferKind::Download, local, remote, WriteMode::Overwrite, ctx);
            return;
        }
        match self.xfer.policy {
            OverwritePolicy::Ask => {
                self.pending_overwrite = Some(PendingOverwrite::Download {
                    remote,
                    local,
                    source_size,
                    dest_size,
                });
            }
            OverwritePolicy::OverwriteAll => {
                self.enqueue_xfer(handle, XferKind::Download, local, remote, WriteMode::Overwrite, ctx);
            }
            OverwritePolicy::SkipAll => {
                self.pending_status_ok = Some(
                    crate::i18n::tr(ctx, "Skipped existing file", "已跳过同名文件").to_string(),
                );
            }
            OverwritePolicy::ResumeAll => {
                if dest_size < source_size {
                    self.enqueue_xfer(handle, XferKind::Download, local, remote, WriteMode::Resume, ctx);
                } else if dest_size == source_size {
                    self.pending_status_ok = Some(
                        crate::i18n::tr(ctx, "Already complete — skipped", "已完整，已跳过")
                            .to_string(),
                    );
                } else {
                    self.enqueue_xfer(handle, XferKind::Download, local, remote, WriteMode::Overwrite, ctx);
                }
            }
            OverwritePolicy::IfNewerAll => {
                let src_m = self
                    .entries
                    .iter()
                    .find(|e| e.path == remote)
                    .map(|e| e.modified.timestamp().max(0) as u64);
                let dst_m = std::fs::metadata(&local)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs());
                match resolve_write_mode_mtime(
                    OverwritePolicy::IfNewerAll,
                    dest_exists,
                    dest_size,
                    source_size,
                    src_m,
                    dst_m,
                ) {
                    Some(mode) => {
                        self.enqueue_xfer(handle, XferKind::Download, local, remote, mode, ctx);
                    }
                    None => {
                        self.pending_status_ok = Some(
                            crate::i18n::tr(ctx, "Already up to date — skipped", "已是最新，已跳过")
                                .to_string(),
                        );
                    }
                }
            }
        }
    }

    fn request_upload_many(
        &mut self,
        handle: &SshSessionHandle,
        cwd: PathBuf,
        locals: Vec<PathBuf>,
        ctx: &egui::Context,
    ) {
        let mut conflict_names = Vec::new();
        for local in &locals {
            if let Some(name) = local.file_name().and_then(|n| n.to_str()) {
                if self.remote_basename_exists(name) {
                    conflict_names.push(name.to_string());
                }
            }
        }
        if !conflict_names.is_empty() && self.xfer.policy == OverwritePolicy::Ask {
            self.pending_overwrite = Some(PendingOverwrite::UploadMany {
                cwd,
                locals,
                conflict_names,
            });
            return;
        }
        for local in locals {
            let fname = local
                .file_name()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("upload.bin"));
            let remote = cwd.join(&fname);
            let name = fname.to_string_lossy().into_owned();
            let source_size = std::fs::metadata(&local).map(|m| m.len()).unwrap_or(0);
            let src_m = std::fs::metadata(&local)
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            let dest_exists = self.remote_basename_exists(&name);
            let dest_size = self.remote_entry_size(&name);
            let dst_m = self
                .entries
                .iter()
                .find(|e| e.name == name)
                .map(|e| e.modified.timestamp().max(0) as u64);
            match resolve_write_mode_mtime(
                self.xfer.policy,
                dest_exists,
                dest_size,
                source_size,
                src_m,
                dst_m,
            ) {
                Some(mode) => self.enqueue_xfer(handle, XferKind::Upload, local, remote, mode, ctx),
                None if self.xfer.policy == OverwritePolicy::SkipAll
                    || (dest_exists && dest_size == source_size) => {}
                None => self.enqueue_xfer(
                    handle,
                    XferKind::Upload,
                    local,
                    remote,
                    WriteMode::Overwrite,
                    ctx,
                ),
            }
        }
    }

    fn confirm_overwrite_with_mode(
        &mut self,
        handle: &SshSessionHandle,
        ctx: &egui::Context,
        mode: WriteMode,
        set_policy: Option<OverwritePolicy>,
    ) {
        if let Some(p) = set_policy {
            self.xfer.policy = p;
        }
        match self.pending_overwrite.take() {
            Some(PendingOverwrite::Upload { remote, local, .. }) => {
                self.enqueue_xfer(handle, XferKind::Upload, local, remote, mode, ctx);
            }
            Some(PendingOverwrite::Download { remote, local, .. }) => {
                self.enqueue_xfer(handle, XferKind::Download, local, remote, mode, ctx);
            }
            Some(PendingOverwrite::UploadMany { cwd, locals, .. }) => {
                for local in locals {
                    let fname = local
                        .file_name()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("upload.bin"));
                    let remote = cwd.join(&fname);
                    self.enqueue_xfer(handle, XferKind::Upload, local, remote, mode, ctx);
                }
            }
            None => {}
        }
    }

    fn skip_pending_overwrite(
        &mut self,
        handle: &SshSessionHandle,
        ctx: &egui::Context,
        set_policy: Option<OverwritePolicy>,
    ) {
        if let Some(p) = set_policy {
            self.xfer.policy = p;
        }
        // UploadMany：跳过冲突项，仍上传无冲突文件。
        if let Some(PendingOverwrite::UploadMany { cwd, locals, conflict_names }) =
            self.pending_overwrite.take()
        {
            let conflict: std::collections::HashSet<&str> =
                conflict_names.iter().map(|s| s.as_str()).collect();
            for local in locals {
                let name = local
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                if conflict.contains(name.as_str()) {
                    continue;
                }
                let remote = cwd.join(&name);
                self.enqueue_xfer(handle, XferKind::Upload, local, remote, WriteMode::Overwrite, ctx);
            }
        } else {
            self.pending_overwrite = None;
        }
        self.pending_status_ok = Some(
            crate::i18n::tr(ctx, "Skipped existing file", "已跳过同名文件").to_string(),
        );
    }

    fn begin_rename_selected(&mut self) {
        let Some(path) = self.remote_selected.primary().cloned() else {
            return;
        };
        if self.remote_selected.len() != 1 {
            return;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() || name == "." || name == ".." {
            return;
        }
        self.rename_from = Some(path);
        self.rename_name = name;
        self.show_rename_dialog = true;
    }

    /// 本机文件行右键：打开 / 在文件管理器中显示，以及传输相关项。
    fn local_entry_context_menu(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        handle: &SshSessionHandle,
        entry: &LocalEntry,
    ) {
        crate::ui::chrome::apply_context_menu_style(ui, theme);
        if entry.is_dir {
            let lbl = crate::i18n::tr(ctx, "Show in folder", "在文件管理器中显示");
            if crate::ui::chrome::popup_menu_button(ui, theme, lbl).clicked() {
                if !crate::platform::reveal_directory(&entry.path) {
                    self.pending_status_err = Some(
                        crate::i18n::tr(
                            ctx,
                            "Could not open folder in file manager",
                            "无法在文件管理器中打开该目录",
                        )
                        .to_string(),
                    );
                }
                ui.close_menu();
            }
        } else {
            let lbl = crate::i18n::tr(ctx, "Open", "打开");
            if crate::ui::chrome::popup_menu_button(ui, theme, lbl).clicked() {
                if let Err(e) = crate::platform::open_file(&entry.path) {
                    let lang = crate::i18n::language(ctx);
                    self.pending_status_err = Some(crate::i18n::localize_backend_error(lang, &e));
                }
                ui.close_menu();
            }
        }
        self.xfer_context_menu(ui, theme, ctx, handle);
    }

    fn remote_row_context_menu(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        handle: &SshSessionHandle,
        entry: &crate::ssh::SftpEntry,
    ) {
        crate::ui::chrome::apply_context_menu_style(ui, theme);
        if !self.remote_selected.contains(&entry.path) {
            self.remote_selected.set_only(entry.path.clone());
        }
        if entry.is_dir {
            let lbl = crate::i18n::tr(ctx, "Open in this terminal", "在当前终端打开");
            if crate::ui::chrome::popup_menu_button(ui, theme, lbl).clicked() {
                self.queue_terminal_cd(&entry.path);
                ui.close_menu();
                return;
            }
        }
        self.xfer_context_menu(ui, theme, ctx, handle);
    }

    /// 上传 / 下载 / 删除远端 — 文件列表右键菜单(点行或空白均可用)。
    fn xfer_context_menu(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        handle: &SshSessionHandle,
    ) {
        crate::ui::chrome::apply_context_menu_style(ui, theme);
        let upload = self.local_upload_job();
        let download = self.remote_download_job();
        let delete_paths: Vec<PathBuf> = self.remote_selected.paths_vec();
        let open_entry = self.remote_selected.primary().and_then(|p| {
            self.entries
                .iter()
                .find(|e| &e.path == p && !e.is_dir)
                .cloned()
        });
        let upload_lbl = crate::i18n::tr(ctx, "Upload", "上传");
        let download_lbl = crate::i18n::tr(ctx, "Download", "下载");
        let delete_lbl = crate::i18n::tr(ctx, "Delete remote", "删除远端");
        if !self.local_selected.is_empty() {
            if crate::ui::chrome::popup_menu_button(ui, theme, upload_lbl).clicked() {
                self.upload_selected_local(handle, ctx);
                ui.close_menu();
            }
        } else if let Some((remote, local)) = upload {
            if crate::ui::chrome::popup_menu_button(ui, theme, upload_lbl).clicked() {
                self.request_upload(handle, remote, local, ctx);
                ui.close_menu();
            }
        }
        if !self.remote_selected.is_empty() {
            if crate::ui::chrome::popup_menu_button(ui, theme, download_lbl).clicked() {
                self.download_selected_remote(handle, ctx);
                ui.close_menu();
            }
        } else if let Some((remote, local)) = download {
            if crate::ui::chrome::popup_menu_button(ui, theme, download_lbl).clicked() {
                self.request_download(handle, remote, local, ctx);
                ui.close_menu();
            }
        }
        if let Some(entry) = open_entry {
            let edit_lbl = if looks_like_text_name(&entry.name) {
                crate::i18n::tr(ctx, "Quick edit", "快速编辑")
            } else {
                crate::i18n::tr(ctx, "Open", "打开")
            };
            if crate::ui::chrome::popup_menu_button(ui, theme, edit_lbl).clicked() {
                self.request_open_remote_file(handle, &entry, ctx);
                ui.close_menu();
            }
        }
        let rename_lbl = crate::i18n::tr(ctx, "Rename", "重命名");
        if self.remote_selected.len() == 1
            && crate::ui::chrome::popup_menu_button(ui, theme, rename_lbl).clicked()
        {
            self.begin_rename_selected();
            ui.close_menu();
        }
        let chmod_lbl = crate::i18n::tr(ctx, "Permissions…", "权限…");
        if !delete_paths.is_empty()
            && crate::ui::chrome::popup_menu_button(ui, theme, chmod_lbl).clicked()
        {
            if let Some(p) = self.remote_selected.primary() {
                if let Some(entry) = self.entries.iter().find(|e| &e.path == p) {
                    self.chmod_octal = Self::permissions_display_to_octal(&entry.permissions)
                        .unwrap_or_else(|| "644".into());
                }
            }
            self.chmod_recursive = false;
            self.pending_chmod_recursive_confirm = false;
            self.show_chmod_dialog = true;
            ui.close_menu();
        }
        if !delete_paths.is_empty() {
            if crate::ui::chrome::popup_menu_button(ui, theme, delete_lbl).clicked() {
                self.pending_delete = if delete_paths.len() == 1 {
                    Some(PendingDeletePrompt::One(delete_paths[0].clone()))
                } else {
                    Some(PendingDeletePrompt::Many(delete_paths))
                };
                ui.close_menu();
            }
        }
    }

    /// 列表空白区右键(传输菜单)。
    fn paint_list_blank_context(
        ui: &mut egui::Ui,
        width: f32,
        add_menu: impl FnOnce(&mut egui::Ui),
    ) {
        let h = ui.available_height();
        if h < 2.0 {
            return;
        }
        let (_, response) = ui.allocate_exact_size(
            egui::vec2(width.max(1.0), h),
            Sense::click(),
        );
        response.context_menu(add_menu);
    }

    /// 本机分区 chrome(标题/路径/操作/表头，不含列表滚动区)。
    fn estimate_local_section_chrome(theme: &Theme) -> f32 {
        let band_h = theme.size_sftp_toolbar_row_h();
        let caption = theme.font_size_caption() + theme.spacing_xs();
        theme.spacing_body_pad() * 2.0
            + caption
            + band_h
            + theme.spacing_xs()
            + theme.size_panel_header_control_h()
            + theme.size_file_list_row_h()
    }

    /// 远端分区 chrome。
    fn estimate_remote_section_chrome(theme: &Theme) -> f32 {
        Self::estimate_local_section_chrome(theme)
    }

    fn quiet_icon(
        ui: &mut egui::Ui,
        theme: &Theme,
        icon: crate::ui::icons::IconId,
        tip: &str,
        enabled: bool,
    ) -> bool {
        let color = if enabled {
            theme.text_primary()
        } else {
            theme.text_tertiary()
        };
        let resp = crate::ui::chrome::icon_button(ui, theme, icon, color).on_hover_text(tip);
        enabled && resp.clicked()
    }

    /// 工具条动作：图标旁边直接写动作，不必悬停才知道用途。
    fn flux_action(
        ui: &mut egui::Ui,
        theme: &Theme,
        icon: crate::ui::icons::IconId,
        label: &str,
        tip: &str,
        enabled: bool,
    ) -> bool {
        let hit = theme.size_panel_header_control_h();
        let color = if enabled {
            theme.text_primary()
        } else {
            theme.text_tertiary()
        };
        let font = egui::FontId::proportional(theme.font_size_caption());
        let galley = ui.fonts(|f| f.layout_no_wrap(label.to_string(), font, color));
        let gap = 2.0;
        let w = hit + gap + galley.size().x + 4.0;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, hit), egui::Sense::click());
        if resp.hovered() && enabled {
            ui.painter().rect_filled(
                rect,
                theme.radius_list_item(),
                theme.color_tab_bar_icon_btn_hover_fill(),
            );
        }
        let icon_rect = egui::Rect::from_min_size(rect.min, egui::vec2(hit, hit));
        crate::ui::icons::paint_icon(
            ui,
            icon_rect,
            icon,
            color,
            (hit - 2.0).min(theme.size_icon_glyph()),
        );
        ui.painter().galley(
            egui::pos2(
                icon_rect.right() + gap,
                rect.center().y - galley.size().y * 0.5,
            ),
            galley,
        );
        enabled && resp.on_hover_text(tip).clicked()
    }

    fn paint_selection_chip(ui: &mut egui::Ui, theme: &Theme, ctx: &egui::Context, n: usize) {
        if n == 0 {
            return;
        }
        ui.label(
            egui::RichText::new(format!(
                "{} {n}",
                crate::i18n::tr(ctx, "Selected", "已选")
            ))
            .size(theme.font_size_small())
            .color(theme.accent_color()),
        );
    }

    /// 在本机/远端分区绘制前，一次性拆分两侧文件列表可用高度(各约一半)。
    fn split_file_list_heights(ui: &egui::Ui, theme: &Theme) -> (f32, f32) {
        let row_h = theme.size_file_list_row_h();
        let min_list = row_h * 2.5;
        let local_chrome = Self::estimate_local_section_chrome(theme);
        let remote_chrome = Self::estimate_remote_section_chrome(theme);
        let gap = theme.spacing_sm() * 2.0;
        let slack = theme.spacing_lg() + row_h;
        let total = ui.available_height();
        let list_total =
            (total - local_chrome - remote_chrome - gap - slack).max(min_list * 2.0);
        let half = list_total * 0.5;
        (half.max(min_list), (list_total - half).max(min_list))
    }

    pub fn new() -> Self {
        let local_root = std::env::temp_dir().join("mistterm_downloads");
        let _ = std::fs::create_dir_all(&local_root);
        Self {
            cwd: PathBuf::from("."),
            entries: Vec::new(),
            path_edit: ".".to_string(),
            remote_selected: SelectionSet::default(),
            local_cwd: local_root.clone(),
            local_entries: Vec::new(),
            local_path_edit: local_root.to_string_lossy().into_owned(),
            local_selected: SelectionSet::default(),
            local_filter: String::new(),
            remote_filter: String::new(),
            bookmarks: SftpBookmarkStore::load(),
            host_key: String::new(),
            remote_edit: None,
            edit_dirty: false,
            pending_edit_upload: None,
            pending_edit_conflict: None,
            show_chmod_dialog: false,
            chmod_octal: "644".to_string(),
            chmod_recursive: false,
            pending_chmod_recursive_confirm: false,
            tree_cancel: None,
            tree_busy: false,
            deferred_upload_files: Vec::new(),
            deferred_download_files: Vec::new(),
            local_list_err: None,
            list_err: None,
            pending_status_ok: None,
            pending_status_err: None,
            busy: false,
            rx: None,
            xfer_progress_rx: None,
            xfer: XferQueue::new(),
            mkdir_name: String::new(),
            show_mkdir_dialog: false,
            pending_delete: None,
            pending_large_open: None,
            pending_overwrite: None,
            show_rename_dialog: false,
            rename_from: None,
            rename_name: String::new(),
            pending_refresh_after_op: false,
            pending_auto_list: false,
            pending_audit: None,
            deferred_gui_download: None,
            last_panel_slot_rect: None,
            local_sort: FileSortState::default(),
            remote_sort: FileSortState::default(),
            local_path_editing: false,
            remote_path_editing: false,
            pending_terminal_cmd: None,
            flux_details_open: false,
        }
    }

    pub fn request_list_on_open(&mut self) {
        self.pending_auto_list = true;
    }

    #[inline]
    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn reset(&mut self) {
        self.cwd = PathBuf::from(".");
        self.entries.clear();
        self.path_edit = ".".to_string();
        self.remote_selected.clear();
        let local_root = std::env::temp_dir().join("mistterm_downloads");
        let _ = std::fs::create_dir_all(&local_root);
        self.local_cwd = local_root.clone();
        self.local_entries.clear();
        self.local_path_edit = local_root.to_string_lossy().into_owned();
        self.local_selected.clear();
        self.local_filter.clear();
        self.remote_filter.clear();
        self.remote_edit = None;
        self.edit_dirty = false;
        self.pending_edit_upload = None;
        self.pending_edit_conflict = None;
        self.show_chmod_dialog = false;
        self.chmod_recursive = false;
        self.pending_chmod_recursive_confirm = false;
        self.tree_cancel = None;
        self.tree_busy = false;
        self.deferred_upload_files.clear();
        self.deferred_download_files.clear();
        self.local_list_err = None;
        self.list_err = None;
        self.pending_status_ok = None;
        self.pending_status_err = None;
        self.busy = false;
        self.rx = None;
        self.xfer_progress_rx = None;
        self.xfer.clear();
        self.mkdir_name.clear();
        self.show_mkdir_dialog = false;
        self.pending_delete = None;
        self.pending_overwrite = None;
        self.show_rename_dialog = false;
        self.rename_from = None;
        self.rename_name.clear();
        self.pending_large_open = None;
        self.pending_refresh_after_op = false;
        self.pending_auto_list = false;
        self.pending_audit = None;
        self.deferred_gui_download = None;
        self.last_panel_slot_rect = None;
        self.local_path_editing = false;
        self.remote_path_editing = false;
        self.pending_terminal_cmd = None;
        self.flux_details_open = false;
    }

    /// 轮询 SFTP 后台任务；面板未显示时也需调用以便 GUI 自动化下载完成。
    pub fn tick_sftp_jobs(
        &mut self,
        handle: &SshSessionHandle,
        ctx: &egui::Context,
        audit: &AuditLogger,
    ) {
        self.poll_xfer_progress();
        self.poll_rx(audit, crate::i18n::language(ctx));
        if !self.busy && self.rx.is_none() {
            self.flush_deferred_transfers(handle, ctx);
            self.kick_xfer_queue(handle, ctx);
            if let Some((remote, local)) = self.deferred_gui_download.take() {
                self.spawn_download(handle, remote, local, ctx);
            }
        }
    }

    fn flush_deferred_transfers(&mut self, handle: &SshSessionHandle, ctx: &egui::Context) {
        if self.busy || self.rx.is_some() {
            return;
        }
        let uploads = std::mem::take(&mut self.deferred_upload_files);
        for (local, remote) in uploads {
            self.request_upload(handle, remote, local, ctx);
        }
        let downloads = std::mem::take(&mut self.deferred_download_files);
        for (remote, local) in downloads {
            self.request_download(handle, remote, local, ctx);
        }
    }

    fn record_sftp_audit(&mut self, audit: &AuditLogger, ok: bool, detail: Option<&str>) {
        let Some((action, resource)) = self.pending_audit.take() else {
            return;
        };
        let outcome = if ok {
            AuditOutcome::Success
        } else {
            AuditOutcome::Failure
        };
        let mut ev = AuditEvent::new(AuditCategory::Session, action, outcome).with_resource(&resource);
        if let Some(d) = detail {
            ev = ev.with_detail(serde_json::json!({ "error": d }));
        }
        audit.record(ev);
        if let Some(scp_action) = action.strip_prefix("sftp.") {
            let mut ev2 = AuditEvent::new(
                AuditCategory::Session,
                format!("file.scp.{scp_action}"),
                outcome,
            )
            .with_resource(resource);
            if let Some(d) = detail {
                ev2 = ev2.with_detail(serde_json::json!({ "error": d }));
            }
            audit.record(ev2);
        }
    }

    fn poll_rx(&mut self, audit: &AuditLogger, lang: UiLanguage) {
        let Some(rx) = &self.rx else {
            return;
        };
        match rx.try_recv() {
            Ok(SftpJobResult::Listed { dir, result }) => {
                match result {
                    Ok(entries) => {
                        self.entries = entries;
                        self.apply_remote_sort();
                        self.cwd = dir.clone();
                        self.sync_remote_path_from_cwd();
                        self.list_err = None;
                        self.sync_remote_selection_after_list();
                        if !self.host_key.is_empty() {
                            self.bookmarks.touch_recent_remote(&self.host_key, &dir);
                        }
                    }
                    Err(e) => {
                        self.list_err = Some(e);
                    }
                }
                self.busy = false;
                self.rx = None;
            }
            Ok(SftpJobResult::Msg(result)) => {
                match result {
                    Ok(msg) => {
                        self.record_sftp_audit(audit, true, None);
                        self.pending_status_ok = Some(msg);
                        self.pending_refresh_after_op = true;
                        self.refresh_local_list();
                    }
                    Err(e) => {
                        self.record_sftp_audit(audit, false, Some(&e));
                        self.pending_status_err = Some(e);
                    }
                }
                self.busy = false;
                self.rx = None;
            }
            Ok(SftpJobResult::TransferDone {
                id,
                kind,
                remote,
                result,
            }) => {
                self.xfer_progress_rx = None;
                match result {
                    Ok(TransferResult::Completed { bytes }) => {
                        self.record_sftp_audit(audit, true, None);
                        self.xfer.finish(id, XferState::Done, bytes, None);
                        let name = remote.to_string_lossy();
                        let msg = match (kind, lang) {
                            (XferKind::Upload, UiLanguage::En) => {
                                format!("Uploaded {bytes} bytes → {name}")
                            }
                            (XferKind::Upload, UiLanguage::Zh) => {
                                format!("已上传 {bytes} bytes → {name}")
                            }
                            (XferKind::Download, UiLanguage::En) => {
                                format!("Downloaded {name} → {bytes} bytes")
                            }
                            (XferKind::Download, UiLanguage::Zh) => {
                                format!("已下载 {name} → {bytes} bytes")
                            }
                        };
                        self.pending_status_ok = Some(msg);
                        self.pending_refresh_after_op = true;
                        self.refresh_local_list();
                    }
                    Ok(TransferResult::Cancelled { bytes_done }) => {
                        self.pending_audit.take();
                        self.xfer.finish(
                            id,
                            XferState::Cancelled,
                            bytes_done,
                            Some(
                                crate::i18n::Locale::from(lang)
                                    .tr("Cancelled", "已取消")
                                    .to_string(),
                            ),
                        );
                        self.pending_status_ok = Some(
                            crate::i18n::Locale::from(lang)
                                .tr("Transfer cancelled", "传输已取消")
                                .to_string(),
                        );
                    }
                    Ok(TransferResult::Skipped) => {
                        self.record_sftp_audit(audit, true, None);
                        self.xfer.finish(id, XferState::Skipped, 0, None);
                        self.pending_status_ok = Some(
                            crate::i18n::Locale::from(lang)
                                .tr("Already complete — skipped", "已完整，已跳过")
                                .to_string(),
                        );
                    }
                    Err(e) => {
                        self.record_sftp_audit(audit, false, Some(&e));
                        self.xfer
                            .finish(id, XferState::Failed, 0, Some(e.clone()));
                        self.pending_status_err = Some(e);
                    }
                }
                self.busy = false;
                self.rx = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.busy = false;
                self.rx = None;
                self.xfer_progress_rx = None;
                self.tree_busy = false;
                self.tree_cancel = None;
                self.pending_status_err = Some(
                    crate::i18n::Locale::from(lang)
                        .tr(
                            "SFTP background worker stopped unexpectedly",
                            "SFTP 后台任务异常中断",
                        )
                        .to_string(),
                );
            }
            Ok(SftpJobResult::UploadTreeReady { files }) => {
                self.busy = false;
                self.rx = None;
                self.tree_busy = false;
                self.tree_cancel = None;
                self.deferred_upload_files.extend(files);
            }
            Ok(SftpJobResult::DownloadTreeReady { files }) => {
                self.busy = false;
                self.rx = None;
                self.tree_busy = false;
                self.tree_cancel = None;
                self.deferred_download_files.extend(files);
            }
            Ok(SftpJobResult::TreeCollectErr(e)) => {
                self.busy = false;
                self.rx = None;
                self.tree_busy = false;
                self.tree_cancel = None;
                self.pending_status_err = Some(e);
            }
            Ok(SftpJobResult::RemoveMany(result)) => {
                match result {
                    Ok(msg) => {
                        self.record_sftp_audit(audit, true, None);
                        self.pending_status_ok = Some(msg);
                        self.pending_refresh_after_op = true;
                    }
                    Err(e) => {
                        self.record_sftp_audit(audit, false, Some(&e));
                        self.pending_status_err = Some(e);
                    }
                }
                self.busy = false;
                self.rx = None;
            }
            Ok(SftpJobResult::ChmodDone(result)) => {
                match result {
                    Ok(msg) => {
                        self.record_sftp_audit(audit, true, None);
                        self.pending_status_ok = Some(msg);
                        self.pending_refresh_after_op = true;
                    }
                    Err(e) => {
                        self.pending_status_err = Some(e);
                    }
                }
                self.busy = false;
                self.rx = None;
            }
            Ok(SftpJobResult::EditPrepared {
                remote,
                track_edits,
                result,
            }) => {
                self.busy = false;
                self.rx = None;
                match result {
                    Ok((local, size, mtime)) => {
                        let open_mtime = std::fs::metadata(&local).ok().and_then(|m| m.modified().ok());
                        if let Err(e) = crate::platform::open_file(&local) {
                            self.pending_status_err =
                                Some(crate::i18n::localize_backend_error(lang, &e));
                        } else if track_edits {
                            self.remote_edit = Some(RemoteEditSession {
                                remote,
                                local,
                                remote_size: size,
                                remote_mtime: mtime,
                                local_mtime_at_open: open_mtime,
                            });
                            self.edit_dirty = false;
                        }
                    }
                    Err(e) => self.pending_status_err = Some(e),
                }
            }
            Ok(SftpJobResult::EditConflictCheck {
                remote,
                local,
                session_size,
                session_mtime,
                result,
            }) => {
                self.busy = false;
                self.rx = None;
                match result {
                    Ok((cur_size, cur_mtime)) => {
                        if cur_size != session_size || cur_mtime != session_mtime {
                            self.pending_edit_conflict = Some((remote, local));
                        } else {
                            self.deferred_upload_files.push((local.clone(), remote));
                            if let Some(session) = self.remote_edit.as_mut() {
                                session.remote_size = cur_size;
                                session.remote_mtime = cur_mtime;
                                session.local_mtime_at_open =
                                    std::fs::metadata(&local).ok().and_then(|m| m.modified().ok());
                                self.edit_dirty = false;
                            }
                        }
                    }
                    Err(e) => self.pending_status_err = Some(e),
                }
            }
        }
    }

    fn list_local_dir(path: &Path) -> Result<Vec<LocalEntry>, String> {
        let read = std::fs::read_dir(path)
            .map_err(|e| format!("Failed to read local directory {}: {}", path.display(), e))?;
        let mut result = Vec::new();
        for ent in read {
            let ent = ent.map_err(|e| format!("read_dir entry: {}", e))?;
            let name = ent.file_name().to_string_lossy().to_string();
            if name == "." || name == ".." {
                continue;
            }
            let full = ent.path();
            let meta = ent.metadata().ok();
            let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
            let size = if is_dir {
                0
            } else {
                meta.as_ref().map(|m| m.len()).unwrap_or(0)
            };
            let modified = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(system_time_to_utc)
                .unwrap_or_else(Utc::now);
            result.push(LocalEntry {
                name,
                is_dir,
                size,
                modified,
                path: full,
            });
        }
        Ok(result)
    }

    fn apply_local_sort(&mut self) {
        sort_local_entries(&mut self.local_entries, self.local_sort);
    }

    fn apply_remote_sort(&mut self) {
        sort_remote_entries(&mut self.entries, self.remote_sort);
    }

    fn expand_local_path(raw: &str) -> PathBuf {
        let s = raw.trim();
        if s == "~" {
            return std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(s));
        }
        if let Some(rest) = s.strip_prefix("~/") {
            if let Some(home) = std::env::var_os("HOME") {
                return PathBuf::from(home).join(rest);
            }
        }
        PathBuf::from(s)
    }

    fn localize_local_list_error(ctx: &egui::Context, msg: &str) -> String {
        if msg.contains("Permission denied") {
            return crate::i18n::tr(
                ctx,
                "No permission to read this folder. Pick another path or tap ↑ Parent.",
                "没有权限读取该目录，请换路径或点「上级」返回。",
            )
            .to_string();
        }
        if msg.contains("No such file") || msg.contains("not found") {
            return crate::i18n::tr(
                ctx,
                "Folder does not exist. Check the path and try again.",
                "目录不存在，请检查路径后重试。",
            )
            .to_string();
        }
        msg.to_string()
    }

    fn try_navigate_local_path(&mut self, ctx: &egui::Context) {
        let raw = self.local_path_edit.trim();
        if raw.is_empty() {
            self.local_list_err = Some(
                crate::i18n::tr(ctx, "Enter a folder path.", "请输入目录路径。").to_string(),
            );
            return;
        }
        let p = Self::expand_local_path(raw);
        if !p.exists() {
            self.local_list_err = Some(
                Self::localize_local_list_error(ctx, "No such file or directory"),
            );
            return;
        }
        if !p.is_dir() {
            self.local_list_err = Some(
                crate::i18n::tr(ctx, "Not a folder.", "不是文件夹。").to_string(),
            );
            return;
        }
        self.local_cwd = p.clone();
        self.sync_local_path_from_cwd();
        self.bookmarks.touch_recent_local(&p);
        self.refresh_local_list();
    }

    fn refresh_local_list(&mut self) {
        match Self::list_local_dir(&self.local_cwd) {
            Ok(entries) => {
                self.local_entries = entries;
                self.apply_local_sort();
                self.local_list_err = None;
                self.bookmarks.touch_recent_local(&self.local_cwd);
                self.sync_local_selection_after_list();
            }
            Err(e) => {
                self.local_entries.clear();
                self.local_selected.clear();
                self.local_list_err = Some(e);
            }
        }
    }

    fn sync_local_path_from_cwd(&mut self) {
        self.local_path_edit = self.local_cwd.to_string_lossy().into_owned();
    }

    fn sync_remote_path_from_cwd(&mut self) {
        self.path_edit = self.cwd.to_string_lossy().into_owned();
    }

    fn localize_list_error(ctx: &egui::Context, msg: &str) -> String {
        if crate::ssh::sftp::is_sftp_would_block_message(msg) {
            return crate::i18n::tr(
                ctx,
                "SFTP channel busy (shell is using the connection). Wait a moment and tap Refresh.",
                "SFTP 通道繁忙(终端正在占用连接)，请稍候再点「刷新」重试。",
            )
            .to_string();
        }
        msg.to_string()
    }

    /// 通过 shell 泵命令队列下发 SFTP 任务，避免与 PTY 读循环并发占用 libssh2 session。
    fn enqueue<F>(&mut self, handle: &SshSessionHandle, ctx: &egui::Context, job: F)
    where
        F: FnOnce(&::ssh2::Session) -> SftpJobResult + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx2 = ctx.clone();
        let result = handle.enqueue_session_job(move |session| {
            let outcome = job(session);
            let _ = tx.send(outcome);
            ctx2.request_repaint();
        });
        if let Err(e) = result {
            self.busy = false;
            self.rx = None;
            self.pending_status_err = Some(e);
        }
    }

    /// OpenSSH 在 Windows 上会把盘符解析成 `/C:/Users/...`，界面上收成 `C:/Users/...`。
    fn normalize_remote_path(path: PathBuf) -> PathBuf {
        let s = path.to_string_lossy().replace('\\', "/");
        let bytes = s.as_bytes();
        if bytes.len() >= 3
            && bytes[0] == b'/'
            && bytes[1].is_ascii_alphabetic()
            && bytes[2] == b':'
        {
            return PathBuf::from(&s[1..]);
        }
        PathBuf::from(s)
    }

    /// `.` 和空路径的上一层是 `..`，不能用 `Path::parent()`（会变成空路径，列表被清空）。
    fn remote_parent_dir(cwd: &Path) -> PathBuf {
        let text = cwd.to_string_lossy();
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed == "." {
            return PathBuf::from("..");
        }
        if trimmed == "/" || trimmed == "\\" {
            return PathBuf::from("/");
        }
        match cwd.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => PathBuf::from("/"),
        }
    }

    fn spawn_list(&mut self, handle: &SshSessionHandle, dir: PathBuf, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.enqueue(handle, ctx, move |session| {
            let client = match SftpClient::new(session) {
                Ok(client) => client,
                Err(e) => {
                    return SftpJobResult::Listed {
                        dir,
                        result: Err(e),
                    };
                }
            };
            let result = client.list_dir(&dir);
            let dir = if result.is_ok() {
                Self::normalize_remote_path(client.realpath(&dir).unwrap_or(dir))
            } else {
                dir
            };
            SftpJobResult::Listed { dir, result }
        });
    }

    /// GUI 自动化 / 兼容入口：直接入队覆盖上传。
    fn spawn_upload(
        &mut self,
        handle: &SshSessionHandle,
        remote: PathBuf,
        local: PathBuf,
        ctx: &egui::Context,
    ) {
        self.enqueue_xfer(handle, XferKind::Upload, local, remote, WriteMode::Overwrite, ctx);
    }

    /// GUI 自动化 / 兼容入口：直接入队覆盖下载。
    fn spawn_download(
        &mut self,
        handle: &SshSessionHandle,
        remote: PathBuf,
        local: PathBuf,
        ctx: &egui::Context,
    ) {
        self.enqueue_xfer(handle, XferKind::Download, local, remote, WriteMode::Overwrite, ctx);
    }

    fn kick_xfer_queue(&mut self, handle: &SshSessionHandle, ctx: &egui::Context) {
        if self.busy || self.rx.is_some() {
            return;
        }
        let Some(item) = self.xfer.take_next_queued() else {
            return;
        };
        let id = item.id;
        let kind = item.kind;
        let local = item.local.clone();
        let remote = item.remote.clone();
        let mode = item.mode;
        let cancel = Arc::new(AtomicBool::new(false));
        self.xfer.cancel = Some(cancel.clone());
        let (prog_tx, prog_rx) = mpsc::sync_channel::<XferProgressEvent>(16);
        self.xfer_progress_rx = Some(prog_rx);
        self.busy = true;
        let audit_action = match kind {
            XferKind::Upload => "sftp.upload",
            XferKind::Download => "sftp.download",
        };
        self.pending_audit = Some((audit_action, remote.to_string_lossy().into_owned()));
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx2 = ctx.clone();
        let result = handle.enqueue_session_job(move |session| {
            let outcome = (|| -> Result<TransferResult, String> {
                let client = SftpClient::new(session)?;
                let mut last_report = Instant::now();
                let mut on_progress = |p: TransferProgress| {
                    let now = Instant::now();
                    if now.duration_since(last_report).as_millis() >= 80 || p.done == p.total {
                        let _ = prog_tx.try_send(XferProgressEvent {
                            id,
                            done: p.done,
                            total: p.total,
                        });
                        last_report = now;
                        ctx2.request_repaint();
                    }
                };
                match kind {
                    XferKind::Upload => {
                        client.upload_ex(&local, &remote, mode, Some(&cancel), &mut on_progress)
                    }
                    XferKind::Download => {
                        client.download_ex(&remote, &local, mode, Some(&cancel), &mut on_progress)
                    }
                }
            })();
            let _ = tx.send(SftpJobResult::TransferDone {
                id,
                kind,
                remote,
                result: outcome,
            });
            ctx2.request_repaint();
        });
        if let Err(e) = result {
            self.busy = false;
            self.rx = None;
            self.xfer_progress_rx = None;
            self.xfer.finish(id, XferState::Failed, 0, Some(e.clone()));
            self.pending_status_err = Some(e);
        }
    }

    fn poll_xfer_progress(&mut self) {
        let Some(rx) = &self.xfer_progress_rx else {
            return;
        };
        while let Ok(ev) = rx.try_recv() {
            self.xfer.mark_running_progress(ev.id, ev.done, ev.total);
        }
    }

    fn handle_xfer_strip_action(
        &mut self,
        action: XferStripAction,
        handle: &SshSessionHandle,
        ctx: &egui::Context,
    ) {
        match action {
            XferStripAction::None => {}
            XferStripAction::CancelActive => self.xfer.request_cancel_active(),
            XferStripAction::Retry(id) => {
                self.xfer.retry_failed(id);
                self.kick_xfer_queue(handle, ctx);
            }
            XferStripAction::Remove(id) => self.xfer.remove(id),
            XferStripAction::PruneFinished => self.xfer.prune_finished(),
        }
    }

    /// Windows/macOS GUI E2E：由 `MISTTERM_GUI_AUTOMATION=1` 启用，配合 Ctrl+Shift+F9/F10。
    pub fn gui_automation_enabled() -> bool {
        matches!(
            std::env::var("MISTTERM_GUI_AUTOMATION").as_deref(),
            Ok("1") | Ok("true") | Ok("TRUE")
        )
    }

    /// GUI 自动化：本机 E2E / ZMODEM 上传文件路径。
    pub fn gui_automation_zmodem_local_path() -> Option<PathBuf> {
        if !Self::gui_automation_enabled() {
            return None;
        }
        if let Ok(p) = std::env::var("MISTTERM_ZMODEM_E2E_LOCAL") {
            let pb = PathBuf::from(p.trim());
            if pb.is_file() {
                return Some(pb);
            }
        }
        Self::gui_automation_e2e_local_path()
    }

    /// GUI 自动化：本机 E2E 文件路径(`%TEMP%/mistterm_downloads/<MISTTERM_E2E_FILE>`)。
    pub fn gui_automation_e2e_local_path() -> Option<PathBuf> {
        if !Self::gui_automation_enabled() {
            return None;
        }
        let name = Self::env_e2e_filename()?;
        let dir = std::env::temp_dir().join("mistterm_downloads");
        let path = dir.join(&name);
        path.exists().then_some(path)
    }

    fn env_e2e_filename() -> Option<String> {
        std::env::var("MISTTERM_E2E_FILE")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn pick_local_by_name(entries: &[LocalEntry], name: &str) -> Option<PathBuf> {
        entries
            .iter()
            .find(|e| e.name == name && !e.is_dir)
            .map(|e| e.path.clone())
    }

    fn pick_remote_by_name(entries: &[SftpEntry], name: &str) -> Option<PathBuf> {
        entries
            .iter()
            .find(|e| e.name == name && !e.is_dir)
            .map(|e| e.path.clone())
    }

    /// GUI 自动化：上传 `MISTTERM_E2E_FILE`(或首个本机文件)。
    pub fn run_gui_automation_upload(
        &mut self,
        ctx: &egui::Context,
        handle: &SshSessionHandle,
    ) {
        if self.busy {
            return;
        }
        self.refresh_local_list();
        if let Some(name) = Self::env_e2e_filename() {
            if let Some(path) = Self::pick_local_by_name(&self.local_entries, &name) {
                self.local_selected.set_only(path);
            }
        } else if self.local_selected.is_empty() {
            if let Some(e) = self.local_entries.iter().find(|e| !e.is_dir) {
                self.local_selected.set_only(e.path.clone());
            }
        }
        if let Some((remote, local)) = self.local_upload_job() {
            self.spawn_upload(handle, remote, local, ctx);
        } else {
            let lang = crate::i18n::language(ctx);
            self.pending_status_err = Some(match lang {
                UiLanguage::En => "GUI automation: no local file selected for upload".into(),
                UiLanguage::Zh => "GUI 自动化：未找到可上传的本机文件".into(),
            });
        }
    }

    /// GUI 自动化：下载 `MISTTERM_E2E_FILE`(或首个远端文件)。
    pub fn run_gui_automation_download(
        &mut self,
        ctx: &egui::Context,
        handle: &SshSessionHandle,
    ) {
        let filename = Self::env_e2e_filename();
        let remote = if let Some(name) = filename.as_deref().filter(|s| !s.is_empty()) {
            if let Some(path) = Self::pick_remote_by_name(&self.entries, name) {
                path
            } else {
                self.cwd.join(name)
            }
        } else if let Some(path) = self.remote_selected.primary().cloned() {
            path
        } else {
            self.entries
                .iter()
                .find(|e| !e.is_dir)
                .map(|e| e.path.clone())
                .unwrap_or_else(|| self.cwd.join("download.bin"))
        };
        let local = self.local_cwd.join(
            remote
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("download.bin"),
        );
        if self.busy {
            self.deferred_gui_download = Some((remote, local));
            return;
        }
        self.spawn_download(handle, remote, local, ctx);
    }

    fn show_mkdir_dialog_ui(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        handle: &SshSessionHandle,
    ) {
        if !self.show_mkdir_dialog {
            return;
        }
        let mut open = true;
        let mut should_close = false;
        let mut do_create = false;
        let mkdir_id = egui::Id::new("sftp_mkdir_modal_name");
        let mkdir_hint = crate::i18n::tr(ctx, "New folder name", "新建目录名");
        let can_create = !self.busy && !self.mkdir_name.trim().is_empty();
        let modal_sz = layout_util::modal_confirm_size(ctx);
        let cwd_label = self.cwd.to_string_lossy();
        let resp = crate::ui::chrome::modal_window("sftp_mkdir_modal", theme, ctx)
            .open(&mut open)
            .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
            .movable(true)
            .resizable(false)
            .fixed_size(modal_sz)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    if crate::ui::chrome::modal_header(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "New folder", "新建目录"),
                        crate::ui::chrome::modal_title_font_size(theme),
                    ) {
                        should_close = true;
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {}",
                            crate::i18n::tr(ctx, "Location:", "位置："),
                            cwd_label,
                        ))
                        .size(theme.font_size_small())
                        .color(theme.text_tertiary()),
                    );
                    ui.add_space(theme.spacing_md());
                    crate::ui::chrome::form_singleline_field(
                        ui,
                        theme,
                        mkdir_id,
                        &mut self.mkdir_name,
                        mkdir_hint,
                        ui.available_width().max(160.0),
                        false,
                    );
                    ui.add_space(theme.spacing_lg());
                    crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                        if crate::ui::chrome::modal_primary_icon_button(
                            ui,
                            th,
                            crate::ui::icons::IconId::Plus,
                            crate::i18n::tr(ctx, "Create", "创建"),
                        )
                        .clicked()
                            && can_create
                        {
                            do_create = true;
                            should_close = true;
                        }
                        if crate::ui::chrome::modal_secondary_icon_button(
                            ui,
                            th,
                            crate::ui::icons::IconId::Cross,
                            crate::i18n::tr(ctx, "Cancel", "取消"),
                        )
                        .clicked()
                        {
                            should_close = true;
                        }
                    });
                })
            });
        if let Some(inner) = resp {
            crate::ui::chrome::raise_window_response(ctx, &inner.response);
        }
        ctx.memory_mut(|m| m.request_focus(mkdir_id));
        if do_create {
            let p = self.cwd.join(self.mkdir_name.trim());
            self.mkdir_name.clear();
            self.spawn_mkdir(handle, p, ctx);
        }
        if !open || should_close {
            self.show_mkdir_dialog = false;
            if !do_create {
                self.mkdir_name.clear();
            }
        }
    }

    fn show_rename_dialog_ui(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        handle: &SshSessionHandle,
    ) {
        if !self.show_rename_dialog {
            return;
        }
        let mut open = true;
        let mut should_close = false;
        let mut do_rename = false;
        let rename_id = egui::Id::new("sftp_rename_modal_name");
        let rename_hint = crate::i18n::tr(ctx, "New name", "新名称");
        let can_rename = !self.busy && !self.rename_name.trim().is_empty();
        let modal_sz = layout_util::modal_confirm_size(ctx);
        let from_label = self
            .rename_from
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let resp = crate::ui::chrome::modal_window("sftp_rename_modal", theme, ctx)
            .open(&mut open)
            .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
            .movable(true)
            .resizable(false)
            .fixed_size(modal_sz)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    if crate::ui::chrome::modal_header(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Rename", "重命名"),
                        crate::ui::chrome::modal_title_font_size(theme),
                    ) {
                        should_close = true;
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {}",
                            crate::i18n::tr(ctx, "Current:", "当前："),
                            from_label,
                        ))
                        .size(theme.font_size_small())
                        .color(theme.text_tertiary()),
                    );
                    ui.add_space(theme.spacing_md());
                    crate::ui::chrome::form_singleline_field(
                        ui,
                        theme,
                        rename_id,
                        &mut self.rename_name,
                        rename_hint,
                        ui.available_width().max(160.0),
                        false,
                    );
                    ui.add_space(theme.spacing_lg());
                    crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                        if crate::ui::chrome::modal_primary_icon_button(
                            ui,
                            th,
                            crate::ui::icons::IconId::Check,
                            crate::i18n::tr(ctx, "Rename", "重命名"),
                        )
                        .clicked()
                            && can_rename
                        {
                            do_rename = true;
                            should_close = true;
                        }
                        if crate::ui::chrome::modal_secondary_icon_button(
                            ui,
                            th,
                            crate::ui::icons::IconId::Cross,
                            crate::i18n::tr(ctx, "Cancel", "取消"),
                        )
                        .clicked()
                        {
                            should_close = true;
                        }
                    });
                })
            });
        if let Some(inner) = resp {
            crate::ui::chrome::raise_window_response(ctx, &inner.response);
        }
        ctx.memory_mut(|m| m.request_focus(rename_id));
        if do_rename {
            if let Some(from) = self.rename_from.take() {
                let new_name = self.rename_name.trim().to_string();
                self.rename_name.clear();
                let parent = from.parent().map(PathBuf::from).unwrap_or_else(|| self.cwd.clone());
                let to = parent.join(new_name);
                self.spawn_rename(handle, from, to, ctx);
            }
        }
        if !open || should_close {
            self.show_rename_dialog = false;
            if !do_rename {
                self.rename_from = None;
                self.rename_name.clear();
            }
        }
    }

    fn show_chmod_dialog_ui(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        handle: &SshSessionHandle,
    ) {
        if !self.show_chmod_dialog {
            return;
        }
        let paths: Vec<PathBuf> = self.remote_selected.paths_vec();
        if paths.is_empty() {
            self.show_chmod_dialog = false;
            return;
        }
        let perm_hint = self
            .remote_selected
            .primary()
            .and_then(|p| self.entries.iter().find(|e| &e.path == p))
            .map(|e| e.permissions.clone())
            .unwrap_or_default();
        let mut open = true;
        let mut should_close = false;
        let mut do_apply = false;
        let oct_id = egui::Id::new("sftp_chmod_octal");
        let modal_sz = layout_util::modal_confirm_size(ctx);
        let resp = crate::ui::chrome::modal_window("sftp_chmod_modal", theme, ctx)
            .open(&mut open)
            .default_pos(layout_util::modal_center_pos(ctx, modal_sz))
            .movable(true)
            .resizable(false)
            .fixed_size(modal_sz)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                    if crate::ui::chrome::modal_header(
                        ui,
                        theme,
                        crate::i18n::tr(ctx, "Permissions", "权限"),
                        crate::ui::chrome::modal_title_font_size(theme),
                    ) {
                        should_close = true;
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {}  ·  {} {}",
                            crate::i18n::tr(ctx, "Current:", "当前："),
                            perm_hint,
                            crate::i18n::tr(ctx, "Items:", "项数："),
                            paths.len(),
                        ))
                        .size(theme.font_size_small())
                        .color(theme.text_tertiary()),
                    );
                    ui.add_space(theme.spacing_md());
                    ui.label(crate::i18n::tr(ctx, "Octal (e.g. 644)", "八进制（如 644）"));
                    crate::ui::chrome::form_singleline_field(
                        ui,
                        theme,
                        oct_id,
                        &mut self.chmod_octal,
                        "644",
                        ui.available_width().max(120.0),
                        false,
                    );
                    ui.checkbox(
                        &mut self.chmod_recursive,
                        crate::i18n::tr(ctx, "Include subfolders", "包含子目录"),
                    );
                    if self.pending_chmod_recursive_confirm && self.chmod_recursive {
                        ui.label(
                            egui::RichText::new(crate::i18n::tr(
                                ctx,
                                "Recursive chmod affects many files. Confirm again.",
                                "递归改权限影响范围大，请再次确认。",
                            ))
                            .small()
                            .color(theme.amber_color()),
                        );
                    }
                    ui.add_space(theme.spacing_lg());
                    crate::ui::chrome::modal_footer_actions(ui, theme, |ui, th| {
                        if crate::ui::chrome::modal_primary_icon_button(
                            ui,
                            th,
                            crate::ui::icons::IconId::Check,
                            crate::i18n::tr(ctx, "Apply", "应用"),
                        )
                        .clicked()
                        {
                            do_apply = true;
                        }
                        if crate::ui::chrome::modal_secondary_icon_button(
                            ui,
                            th,
                            crate::ui::icons::IconId::Cross,
                            crate::i18n::tr(ctx, "Cancel", "取消"),
                        )
                        .clicked()
                        {
                            should_close = true;
                        }
                    });
                })
            });
        if let Some(inner) = resp {
            crate::ui::chrome::raise_window_response(ctx, &inner.response);
        }
        if do_apply {
            if self.chmod_recursive && !self.pending_chmod_recursive_confirm {
                self.pending_chmod_recursive_confirm = true;
            } else if let Some(mode) = Self::parse_octal_mode(&self.chmod_octal) {
                should_close = true;
                self.show_chmod_dialog = false;
                self.pending_chmod_recursive_confirm = false;
                self.spawn_chmod(handle, paths, mode, self.chmod_recursive, ctx);
            } else {
                self.pending_status_err = Some(
                    crate::i18n::tr(ctx, "Invalid octal mode", "八进制权限无效").to_string(),
                );
            }
        }
        if !open || should_close {
            self.show_chmod_dialog = false;
            self.pending_chmod_recursive_confirm = false;
        }
    }

    fn spawn_rename(
        &mut self,
        handle: &SshSessionHandle,
        from: PathBuf,
        to: PathBuf,
        ctx: &egui::Context,
    ) {
        if self.busy {
            return;
        }
        if from == to {
            return;
        }
        self.busy = true;
        self.pending_audit = Some(("sftp.rename", to.to_string_lossy().into_owned()));
        let lang = crate::i18n::language(ctx);
        self.enqueue(handle, ctx, move |session| {
            let msg = (|| -> Result<String, String> {
                let client = SftpClient::new(session)?;
                client.rename(&from, &to)?;
                Ok(match lang {
                    UiLanguage::En => format!(
                        "Renamed {} → {}",
                        from.to_string_lossy(),
                        to.to_string_lossy()
                    ),
                    UiLanguage::Zh => format!(
                        "已重命名 {} → {}",
                        from.to_string_lossy(),
                        to.to_string_lossy()
                    ),
                })
            })();
            SftpJobResult::Msg(msg)
        });
    }

    fn spawn_mkdir(&mut self, handle: &SshSessionHandle, path: PathBuf, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.pending_audit = Some(("sftp.mkdir", path.to_string_lossy().into_owned()));
        let lang = crate::i18n::language(ctx);
        self.enqueue(handle, ctx, move |session| {
            let msg = (|| -> Result<String, String> {
                let client = SftpClient::new(session)?;
                client.mkdir(&path)?;
                Ok(match lang {
                    UiLanguage::En => format!("Created directory {}", path.to_string_lossy()),
                    UiLanguage::Zh => format!("已创建目录 {}", path.to_string_lossy()),
                })
            })();
            SftpJobResult::Msg(msg)
        });
    }

    fn spawn_remove(&mut self, handle: &SshSessionHandle, path: PathBuf, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.pending_audit = Some(("sftp.delete", path.to_string_lossy().into_owned()));
        let lang = crate::i18n::language(ctx);
        self.enqueue(handle, ctx, move |session| {
            let msg = (|| -> Result<String, String> {
                let client = SftpClient::new(session)?;
                client.remove(&path)?;
                Ok(match lang {
                    UiLanguage::En => format!("Deleted {}", path.to_string_lossy()),
                    UiLanguage::Zh => format!("已删除 {}", path.to_string_lossy()),
                })
            })();
            SftpJobResult::Msg(msg)
        });
    }

    #[allow(dead_code)] // 兼容入口；拖放/工具栏走 request_upload_many
    fn spawn_upload_many(
        &mut self,
        handle: &SshSessionHandle,
        cwd: PathBuf,
        locals: Vec<PathBuf>,
        ctx: &egui::Context,
    ) {
        self.request_upload_many(handle, cwd, locals, ctx);
    }

    /// 右侧 SFTP 侧栏入口(`close_panel` 置为 true 时由宿主隐藏侧栏)
    #[inline]
    pub(crate) fn last_panel_slot_rect(&self) -> Option<egui::Rect> {
        self.last_panel_slot_rect
    }

    /// 取出待显示在底栏的状态文案(成功/失败)；失败项优先于成功项。
    pub fn take_pending_status(&mut self) -> (Option<String>, Option<String>) {
        (
            self.pending_status_ok.take(),
            self.pending_status_err.take(),
        )
    }

    pub fn take_pending_terminal_cmd(&mut self) -> Option<String> {
        self.pending_terminal_cmd.take()
    }

    fn shell_cd_command(path: &Path) -> String {
        let raw = path.to_string_lossy().replace('\\', "/");
        let quoted = raw.replace('\'', "'\\''");
        format!("cd '{quoted}'")
    }

    fn queue_terminal_cd(&mut self, path: &Path) {
        self.pending_terminal_cmd = Some(Self::shell_cd_command(path));
    }

    pub fn show_side_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        right_dock_outer_left: &mut Option<f32>,
        dock_col_w: f32,
    ) {
        let (def_w, min_w, max_w) = layout_util::right_dock_resize_bounds(dock_col_w);
        let panel = egui::SidePanel::right("sftp_browser_panel")
            .default_width(def_w)
            .min_width(min_w)
            .max_width(max_w)
            .resizable(true)
            .frame(crate::ui::chrome::right_dock_placeholder_frame(theme))
            .show(ctx, |ui| {
                let h = ui.available_height().max(1.0);
                let w = ui.available_width().max(1.0);
                ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
            });
        let dock_inset = theme.spacing_right_dock_screen_inset();
        let slot = layout_util::side_panel_place_slot(ctx, &panel.response, dock_col_w, dock_inset);
        crate::ui::chrome::paint_right_dock_slot_gap(ctx, theme, slot);
        self.last_panel_slot_rect = Some(slot);
        if let Some(slot) = self.last_panel_slot_rect {
            layout_util::record_right_dock_panel_rect(&slot, right_dock_outer_left);
        } else {
            layout_util::record_right_dock_panel(&panel.response, right_dock_outer_left);
        }
        let _ = theme;
    }

    /// Central 之后绘制 SFTP 前景正文(与 AI/监控一致，避免列壳层风格不一致)。
    pub fn show_foreground_panel(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        terminal: Option<&TerminalView>,
        audit: &AuditLogger,
        close_panel: &mut bool,
    ) {
        let screen = ctx.screen_rect();
        let dock_inset = theme.spacing_right_dock_screen_inset();
        let Some(slot) = layout_util::right_dock_foreground_slot(
            self.last_panel_slot_rect,
            ctx,
            "sftp_browser_panel",
            layout_util::SidePanelProfile::Standard,
            None,
            dock_inset,
        ) else {
            return;
        };
        let geom = crate::ui::chrome::prepare_right_dock_foreground_geom(slot, screen, theme);
        let layer_id = crate::ui::chrome::right_dock_foreground_layer_id("mistterm_sftp_fg");
        crate::ui::chrome::paint_right_dock_foreground_shell(ctx, layer_id, geom.paint, theme);
        crate::ui::chrome::show_right_dock_foreground_body(
            "mistterm_sftp_fg",
            ctx,
            theme,
            &geom,
            layout_util::SidePanelProfile::Standard,
            |ui, body_w| {
                self.show_content(ui, ctx, theme, terminal, audit, close_panel, body_w);
            },
        );
    }

    fn show_content(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        terminal: Option<&TerminalView>,
        audit: &AuditLogger,
        close_panel: &mut bool,
        body_w: f32,
    ) {
        self.poll_xfer_progress();
        self.poll_rx(audit, crate::i18n::language(ctx));
        self.poll_remote_edit();

        let content_w = layout_util::constrain_ui_to_right_dock_body(ui, body_w);

        let mut header_closed = false;
        let prev_gap_y = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;
        theme.frame_right_dock_header_band().show(ui, |ui| {
            ui.set_max_width(content_w);
            header_closed = crate::ui::chrome::dock_panel_title_close_only(
                ui,
                theme,
                crate::ui::icons::IconId::Folder,
                "SFTP",
                crate::i18n::tr(
                    ctx,
                    "Hide panel · reopen from Activity Rail or View menu",
                    "隐藏面板 · 可从活动栏或「视图」菜单再打开",
                ),
            );
        });
        if header_closed {
            if self.edit_dirty {
                self.pending_status_err = Some(
                    crate::i18n::tr(
                        ctx,
                        "You have unsaved remote edits. Upload or discard them before closing.",
                        "还有未回传的修改，请先上传或放弃后再关面板。",
                    )
                    .to_string(),
                );
            } else {
                *close_panel = true;
            }
        }
        crate::ui::chrome::right_dock_header_divider(ui, theme);
        ui.spacing_mut().item_spacing.y = prev_gap_y;
        ui.add_space(theme.spacing_dock_section_gap());

        if !crate::ui::chrome::show_right_dock_ssh_gate(
            ui,
            theme,
            ctx,
            terminal,
            "Connect a session before using SFTP.",
            "请打开会话并连接后可使用 SFTP。",
        ) {
            return;
        }
        let Some(t) = terminal else {
            return;
        };

        let Some(handle) = t.sftp_session_for_ops() else {
            ui.label(egui::RichText::new(crate::i18n::tr(ctx, "Session unavailable", "会话不可用")).color(theme.red_color()));
            return;
        };

        self.host_key = t.connection_server_text();
        if !self.busy && self.rx.is_none() {
            self.flush_deferred_transfers(&handle, ctx);
        }

        let download_dir_path = PathBuf::from(t.download_dir());

        // 可变操作成功后自动刷新；否则处理「打开面板时首次加载」
        if self.pending_refresh_after_op && !self.busy && self.rx.is_none() {
            self.pending_refresh_after_op = false;
            self.refresh_local_list();
            self.spawn_list(&handle, self.cwd.clone(), ctx);
        } else if self.pending_auto_list && !self.busy && self.rx.is_none() {
            self.pending_auto_list = false;
            self.local_cwd = download_dir_path.clone();
            self.sync_local_path_from_cwd();
            self.refresh_local_list();
            self.spawn_list(&handle, self.cwd.clone(), ctx);
        }

        if !theme.uses_modern_palette() {
        if let Some(err) = &self.list_err {
            let msg = Self::localize_list_error(ctx, err);
            egui::Frame::none()
                .fill(theme.color_subtle_inset_fill())
                .stroke(egui::Stroke::new(1.0, theme.red_a128()))
                .rounding(theme.radius_list_item())
                .inner_margin(egui::Margin::symmetric(
                    theme.spacing_search_input_x(),
                    theme.spacing_search_input_y(),
                ))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(msg).color(theme.red_color()));
                    ui.horizontal(|ui| {
                        if crate::ui::chrome::panel_action_primary_icon_button(
                            ui,
                            theme,
                            crate::ui::icons::IconId::Refresh,
                            crate::i18n::tr(ui.ctx(), "Retry", "重试"),
                        )
                        .clicked()
                        {
                            self.list_err = None;
                            self.spawn_list(&handle, self.cwd.clone(), ctx);
                        }
                    });
                });
        }
        }

        ui.add_space(theme.spacing_sm());

        let (local_list_h, remote_list_h) = Self::split_file_list_heights(ui, theme);

        if theme.uses_modern_palette() {
            self.paint_flux_workspace(ui, ctx, theme, &handle, content_w);
        } else {
        Self::paint_browser_section_frame(theme).show(ui, |ui| {
            layout_util::set_width_to_available(ui);
            ui.horizontal(|ui| {
                ui.label(crate::ui::chrome::rich_caption(
                    theme,
                    crate::i18n::tr(ui.ctx(), "Local", "本机"),
                ))
                .on_hover_text(crate::i18n::tr(
                    ctx,
                    "Click to select. Ctrl adds. Shift selects a range.",
                    "单击选择，Ctrl 加选，Shift 选一段。",
                ));
                ui.add_space(theme.spacing_sm());
                Self::paint_selection_chip(ui, theme, ctx, self.local_selected.len());
            });
            let local_path_id = egui::Id::new("sftp_local_path");
            let home_hint = crate::platform::home_dir_display_hint();
            let go_lbl = crate::i18n::tr(ui.ctx(), "Go", "前往");
            let up_lbl = crate::i18n::tr(ui.ctx(), "Up", "上级");
            let refresh_lbl = crate::i18n::tr(ui.ctx(), "Refresh", "刷新");
            let browse_short = crate::i18n::tr(ui.ctx(), "Browse", "浏览");
            let browse_tip = crate::i18n::tr(ui.ctx(), "Browse…", "浏览…");
            let upload_lbl = crate::i18n::tr(ui.ctx(), "Upload", "上传");
            let upload_tip = crate::i18n::tr(
                ui.ctx(),
                "Upload selected item(s)",
                "上传所选本机项",
            );
            let can_upload = self.has_local_upload_selection();
            let mut jump_local: Option<PathBuf> = None;
            let up_ok = self.local_cwd.parent().is_some();
            let local_nav = [
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::ArrowEnter,
                    label: go_lbl,
                    enabled: true,
                    tooltip: go_lbl,
                },
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::ChevronUp,
                    label: up_lbl,
                    enabled: up_ok,
                    tooltip: up_lbl,
                },
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::Refresh,
                    label: refresh_lbl,
                    enabled: true,
                    tooltip: refresh_lbl,
                },
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::Folder,
                    label: browse_short,
                    enabled: true,
                    tooltip: browse_tip,
                },
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::Upload,
                    label: upload_lbl,
                    enabled: can_upload,
                    tooltip: upload_tip,
                },
            ];
            if theme.uses_modern_palette() {
                let _ = &local_nav;
                layout_util::set_width_to_available(ui);
                ui.horizontal(|ui| {
                    let path_w = (ui.available_width() - 96.0).max(80.0);
                    let _path_resp = crate::ui::chrome::form_singleline_field(
                        ui,
                        theme,
                        local_path_id,
                        &mut self.local_path_edit,
                        &home_hint,
                        path_w,
                        false,
                    );
                    self.paint_bookmark_row(
                        ui,
                        theme,
                        ctx,
                        true,
                        self.local_cwd.clone(),
                        &mut jump_local,
                    );
                });
                if ui.ctx().input(|i| i.key_pressed(egui::Key::Enter))
                    && ui.memory(|m| m.has_focus(local_path_id))
                {
                    self.try_navigate_local_path(ctx);
                }
                ui.horizontal(|ui| {
                    if Self::quiet_icon(
                        ui,
                        theme,
                        crate::ui::icons::IconId::ChevronUp,
                        up_lbl,
                        up_ok,
                    ) {
                        if let Some(parent) = self.local_cwd.parent() {
                            self.local_cwd = parent.to_path_buf();
                            self.sync_local_path_from_cwd();
                            self.local_selected.clear();
                            self.refresh_local_list();
                        }
                    }
                    if Self::quiet_icon(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Refresh,
                        refresh_lbl,
                        true,
                    ) {
                        self.refresh_local_list();
                    }
                    if Self::quiet_icon(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Folder,
                        browse_tip,
                        true,
                    ) {
                        if let Some(dir) = FileDialog::new().pick_folder() {
                            self.local_cwd = dir;
                            self.sync_local_path_from_cwd();
                            self.local_selected.clear();
                            self.refresh_local_list();
                        }
                    }
                    ui.separator();
                    if Self::quiet_icon(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Upload,
                        upload_tip,
                        can_upload,
                    ) {
                        self.upload_selected_local(&handle, ctx);
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.local_filter)
                            .hint_text(crate::i18n::tr(ctx, "Filter by name", "按名字找"))
                            .desired_width(ui.available_width().max(48.0)),
                    );
                });
            } else {
                let field_w = Self::dock_field_width(ui);
                let path_resp = crate::ui::chrome::form_singleline_field(
                    ui,
                    theme,
                    local_path_id,
                    &mut self.local_path_edit,
                    &home_hint,
                    field_w,
                    false,
                );
                if ui.ctx().input(|i| i.key_pressed(egui::Key::Enter))
                    && ui.memory(|m| m.has_focus(local_path_id))
                {
                    self.try_navigate_local_path(ctx);
                }
                let _path_resp = path_resp;
                ui.horizontal_wrapped(|ui| {
                    Self::begin_dock_row(ui);
                    ui.spacing_mut().item_spacing.x = theme.spacing_panel_gap();
                    if crate::ui::chrome::panel_action_button_with_icon_ex(
                        ui,
                        theme,
                        crate::ui::icons::IconId::ArrowEnter,
                        go_lbl,
                        true,
                    )
                    .clicked()
                    {
                        self.try_navigate_local_path(ctx);
                    }
                    if crate::ui::chrome::panel_action_button_with_icon_ex(
                        ui,
                        theme,
                        crate::ui::icons::IconId::ChevronUp,
                        up_lbl,
                        up_ok,
                    )
                    .clicked()
                    {
                        if let Some(parent) = self.local_cwd.parent() {
                            self.local_cwd = parent.to_path_buf();
                            self.sync_local_path_from_cwd();
                            self.refresh_local_list();
                        }
                    }
                    if crate::ui::chrome::panel_action_icon_button_ex(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Refresh,
                        refresh_lbl,
                        true,
                    )
                    .clicked()
                    {
                        self.refresh_local_list();
                    }
                    if crate::ui::chrome::panel_action_icon_button_ex(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Folder,
                        browse_tip,
                        true,
                    )
                    .clicked()
                    {
                        if let Some(dir) = FileDialog::new().pick_folder() {
                            self.local_cwd = dir;
                            self.sync_local_path_from_cwd();
                            self.refresh_local_list();
                        }
                    }
                    if crate::ui::chrome::panel_action_button_with_icon_ex(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Upload,
                        upload_lbl,
                        can_upload,
                    )
                    .clicked()
                    {
                        self.upload_selected_local(&handle, ctx);
                    }
                });
                ui.add_space(theme.spacing_xs());
                self.paint_bookmark_row(
                    ui,
                    theme,
                    ctx,
                    true,
                    self.local_cwd.clone(),
                    &mut jump_local,
                );
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(crate::i18n::tr(ctx, "Filter", "筛选"))
                            .small()
                            .color(theme.text_tertiary()),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.local_filter)
                            .hint_text(crate::i18n::tr(ctx, "Name…", "文件名…"))
                            .desired_width(ui.available_width().max(80.0)),
                    );
                });
            }
            if let Some(p) = jump_local.take() {
                if p.is_dir() {
                    self.local_cwd = p;
                    self.sync_local_path_from_cwd();
                    self.local_selected.clear();
                    self.refresh_local_list();
                } else {
                    self.pending_status_err = Some(
                        crate::i18n::tr(ctx, "Path is not a folder", "路径不是文件夹").to_string(),
                    );
                }
            }
            if self.tree_busy {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(crate::i18n::tr(
                            ctx,
                            "Scanning files…",
                            "正在统计文件…",
                        ))
                        .small()
                        .color(theme.amber_color()),
                    );
                    if ui
                        .small_button(crate::i18n::tr(ctx, "Cancel", "取消"))
                        .clicked()
                    {
                        self.cancel_tree_collect();
                    }
                });
            }
            if let Some(err) = &self.local_list_err {
                let msg = Self::localize_local_list_error(ctx, err);
                ui.label(egui::RichText::new(msg).small().color(theme.red_color()));
            }
            let mut enter_local: Option<PathBuf> = None;
            let local_block_h = local_list_h + theme.size_file_list_row_h();
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width().max(1.0), local_block_h),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
            Self::paint_file_list_viewport_frame(theme).show(ui, |ui| {
                layout_util::set_width_to_available(ui);
                let table_cols = FileTableCols::for_list_ui(ui, content_w);
                if paint_file_table_header(ui, theme, ctx, table_cols, &mut self.local_sort) {
                    self.apply_local_sort();
                }
                egui::ScrollArea::vertical()
                    .id_source("sftp_local_list")
                    .auto_shrink([false, false])
                    .max_height(local_list_h)
                    .show(ui, |ui| {
                        ui.visuals_mut().extreme_bg_color = theme.color_file_list_bg();
                        ui.set_max_width(table_cols.total);
                        let local_rows: Vec<LocalEntry> = self
                            .filtered_local_entries()
                            .into_iter()
                            .cloned()
                            .collect();
                        let ordered = local_rows.iter().map(|e| e.path.clone()).collect::<Vec<_>>();
                        let (ctrl, shift) = ui.ctx().input(|i| {
                            (i.modifiers.ctrl || i.modifiers.command, i.modifiers.shift)
                        });
                        for e in &local_rows {
                            let sel = self.local_selected.contains(&e.path);
                            let size_lbl =
                                if e.is_dir { "—".to_string() } else { e.size_human() };
                            let time_lbl = format_file_mtime(e.modified);
                            let kind = classify_file_kind(&e.name, e.is_dir);
                            let resp = paint_file_table_row(
                                ui,
                                theme,
                                table_cols,
                                &e.name,
                                &size_lbl,
                                &time_lbl,
                                kind,
                                sel,
                                &e.path.display().to_string(),
                            );
                            if resp.double_clicked() && e.is_dir {
                                enter_local = Some(e.path.clone());
                            } else if resp.clicked() {
                                self.local_selected.apply_click(
                                    e.path.clone(),
                                    &ordered,
                                    ctrl,
                                    shift,
                                );
                            }
                            resp.context_menu(|ui| {
                                self.local_entry_context_menu(ui, theme, ctx, &handle, e);
                            });
                        }
                        Self::paint_list_blank_context(ui, table_cols.total, |ui| {
                            self.xfer_context_menu(ui, theme, ctx, &handle);
                        });
                    });
            });
            });
            if let Some(d) = enter_local {
                self.local_cwd = d;
                self.sync_local_path_from_cwd();
                self.local_selected.clear();
                self.refresh_local_list();
            }
        });

        ui.add_space(theme.spacing_sm());

        if self.edit_dirty {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ctx,
                        "Local copy changed — upload to server?",
                        "本地文件已修改，是否上传？",
                    ))
                    .small()
                    .color(theme.amber_color()),
                );
                if ui
                    .small_button(crate::i18n::tr(ctx, "Upload", "上传"))
                    .clicked()
                {
                    self.request_edit_upload(&handle, ctx);
                }
                if ui
                    .small_button(crate::i18n::tr(ctx, "Discard", "放弃"))
                    .clicked()
                {
                    self.edit_dirty = false;
                    if let Some(session) = self.remote_edit.as_ref() {
                        if let Some(m) = std::fs::metadata(&session.local)
                            .ok()
                            .and_then(|x| x.modified().ok())
                        {
                            if let Some(s) = self.remote_edit.as_mut() {
                                s.local_mtime_at_open = Some(m);
                            }
                        }
                    }
                }
            });
        }
        if let Some((remote, local)) = self.pending_edit_conflict.take() {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ctx,
                        "Remote file changed on server — overwrite anyway?",
                        "服务器上文件已变更，仍要覆盖？",
                    ))
                    .small()
                    .color(theme.red_color()),
                );
                if ui
                    .small_button(crate::i18n::tr(ctx, "Upload anyway", "仍上传"))
                    .clicked()
                {
                    self.deferred_upload_files.push((local, remote));
                }
            });
        }

        Self::paint_browser_section_frame(theme).show(ui, |ui| {
            layout_util::set_width_to_available(ui);
            ui.horizontal(|ui| {
                ui.label(crate::ui::chrome::rich_caption(
                    theme,
                    crate::i18n::tr(ui.ctx(), "Remote", "远端"),
                ))
                .on_hover_text(crate::i18n::tr(
                    ctx,
                    "Double-click a text file to edit it here, then send it back.",
                    "文本文件双击即可在本机改，改完再传回。",
                ));
                ui.add_space(theme.spacing_sm());
                Self::paint_selection_chip(ui, theme, ctx, self.remote_selected.len());
            });
            let remote_path_id = egui::Id::new("sftp_path_edit");
            let remote_path_hint = crate::i18n::tr(ui.ctx(), "/home/user", "/home/user");
            let go_lbl = crate::i18n::tr(ui.ctx(), "Go", "前往");
            let up_lbl = crate::i18n::tr(ui.ctx(), "Up", "上级");
            let refresh_lbl = crate::i18n::tr(ui.ctx(), "Refresh", "刷新");
            let new_short = crate::i18n::tr(ui.ctx(), "New", "新建");
            let new_tip = crate::i18n::tr(ui.ctx(), "New folder", "新建目录");
            let download_lbl = crate::i18n::tr(ui.ctx(), "Download", "下载");
            let download_tip = crate::i18n::tr(
                ui.ctx(),
                "Download selected remote item(s)",
                "下载所选远端项",
            );
            let rename_lbl = crate::i18n::tr(ui.ctx(), "Rename", "重命名");
            let can_download = self.has_remote_download_selection();
            let can_rename = !self.busy && self.remote_selected.len() == 1;
            let mut jump_remote: Option<PathBuf> = None;
            let busy = !self.busy;
            let remote_nav = [
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::ArrowEnter,
                    label: go_lbl,
                    enabled: busy,
                    tooltip: go_lbl,
                },
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::ChevronUp,
                    label: up_lbl,
                    enabled: busy,
                    tooltip: up_lbl,
                },
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::Refresh,
                    label: refresh_lbl,
                    enabled: busy,
                    tooltip: refresh_lbl,
                },
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::Plus,
                    label: new_short,
                    enabled: busy,
                    tooltip: new_tip,
                },
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::Download,
                    label: download_lbl,
                    enabled: can_download,
                    tooltip: download_tip,
                },
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::SortName,
                    label: rename_lbl,
                    enabled: can_rename,
                    tooltip: rename_lbl,
                },
            ];
            if theme.uses_modern_palette() {
                let _ = &remote_nav;
                layout_util::set_width_to_available(ui);
                ui.horizontal(|ui| {
                    let path_w = (ui.available_width() - 96.0).max(80.0);
                    crate::ui::chrome::form_singleline_field(
                        ui,
                        theme,
                        remote_path_id,
                        &mut self.path_edit,
                        remote_path_hint,
                        path_w,
                        false,
                    );
                    self.paint_bookmark_row(
                        ui,
                        theme,
                        ctx,
                        false,
                        self.cwd.clone(),
                        &mut jump_remote,
                    );
                });
                if ui.ctx().input(|i| i.key_pressed(egui::Key::Enter))
                    && ui.memory(|m| m.has_focus(remote_path_id))
                {
                    self.spawn_list(&handle, PathBuf::from(self.path_edit.trim()), ctx);
                }
                let can_edit = self.remote_selected.len() == 1
                    && self.remote_selected.primary().is_some_and(|p| {
                        self.entries.iter().any(|e| &e.path == p && !e.is_dir)
                    });
                ui.horizontal(|ui| {
                    if Self::quiet_icon(
                        ui,
                        theme,
                        crate::ui::icons::IconId::ChevronUp,
                        up_lbl,
                        busy,
                    ) {
                        self.spawn_list(&handle, Self::remote_parent_dir(&self.cwd), ctx);
                    }
                    if Self::quiet_icon(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Refresh,
                        refresh_lbl,
                        busy,
                    ) {
                        self.spawn_list(&handle, self.cwd.clone(), ctx);
                    }
                    if Self::quiet_icon(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Plus,
                        new_tip,
                        busy,
                    ) {
                        self.show_mkdir_dialog = true;
                    }
                    ui.separator();
                    if Self::quiet_icon(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Download,
                        download_tip,
                        can_download,
                    ) {
                        self.download_selected_remote(&handle, ctx);
                    }
                    if Self::quiet_icon(
                        ui,
                        theme,
                        crate::ui::icons::IconId::File,
                        crate::i18n::tr(
                            ctx,
                            "Download to a temp file and open",
                            "下载到临时文件并打开",
                        ),
                        can_edit && busy,
                    ) {
                        if let Some(p) = self.remote_selected.primary().cloned() {
                            if let Some(entry) = self.entries.iter().find(|e| e.path == p).cloned() {
                                self.request_open_remote_file(&handle, &entry, ctx);
                            }
                        }
                    }
                    if Self::quiet_icon(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Key,
                        crate::i18n::tr(ctx, "Change permissions", "改权限"),
                        self.remote_selected.len() == 1 && busy,
                    ) {
                        if let Some(p) = self.remote_selected.primary() {
                            if let Some(entry) = self.entries.iter().find(|e| &e.path == p) {
                                self.chmod_octal = Self::permissions_display_to_octal(
                                    &entry.permissions,
                                )
                                .unwrap_or_else(|| "644".into());
                            }
                        }
                        self.chmod_recursive = false;
                        self.pending_chmod_recursive_confirm = false;
                        self.show_chmod_dialog = true;
                    }
                    if Self::quiet_icon(
                        ui,
                        theme,
                        crate::ui::icons::IconId::SortName,
                        rename_lbl,
                        can_rename,
                    ) {
                        self.begin_rename_selected();
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.remote_filter)
                            .hint_text(crate::i18n::tr(ctx, "Filter by name", "按名字找"))
                            .desired_width(ui.available_width().max(48.0)),
                    );
                });
            } else {
                let remote_path_w = Self::dock_field_width(ui);
                crate::ui::chrome::form_singleline_field(
                    ui,
                    theme,
                    remote_path_id,
                    &mut self.path_edit,
                    remote_path_hint,
                    remote_path_w,
                    false,
                );
                ui.horizontal_wrapped(|ui| {
                    Self::begin_dock_row(ui);
                    ui.spacing_mut().item_spacing.x = theme.spacing_panel_gap();
                    if crate::ui::chrome::panel_action_button_with_icon_ex(
                        ui,
                        theme,
                        crate::ui::icons::IconId::ArrowEnter,
                        go_lbl,
                        busy,
                    )
                    .clicked()
                    {
                        self.spawn_list(&handle, PathBuf::from(self.path_edit.trim()), ctx);
                    }
                    if crate::ui::chrome::panel_action_button_with_icon_ex(
                        ui,
                        theme,
                        crate::ui::icons::IconId::ChevronUp,
                        up_lbl,
                        busy,
                    )
                    .clicked()
                    {
                        self.spawn_list(&handle, Self::remote_parent_dir(&self.cwd), ctx);
                    }
                    if crate::ui::chrome::panel_action_icon_button_ex(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Refresh,
                        refresh_lbl,
                        busy,
                    )
                    .clicked()
                    {
                        self.spawn_list(&handle, self.cwd.clone(), ctx);
                    }
                    if crate::ui::chrome::panel_action_button_with_icon_ex(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Plus,
                        new_tip,
                        busy,
                    )
                    .clicked()
                    {
                        self.show_mkdir_dialog = true;
                    }
                    if crate::ui::chrome::panel_action_button_with_icon_ex(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Download,
                        download_lbl,
                        can_download,
                    )
                    .clicked()
                    {
                        self.download_selected_remote(&handle, ctx);
                    }
                    if crate::ui::chrome::panel_action_button_with_icon_ex(
                        ui,
                        theme,
                        crate::ui::icons::IconId::SortName,
                        rename_lbl,
                        can_rename,
                    )
                    .clicked()
                    {
                        self.begin_rename_selected();
                    }
                });
                ui.add_space(theme.spacing_xs());
                self.paint_bookmark_row(
                    ui,
                    theme,
                    ctx,
                    false,
                    self.cwd.clone(),
                    &mut jump_remote,
                );
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(crate::i18n::tr(ctx, "Filter", "筛选"))
                            .small()
                            .color(theme.text_tertiary()),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.remote_filter)
                            .hint_text(crate::i18n::tr(ctx, "Name…", "文件名…"))
                            .desired_width(ui.available_width().max(80.0)),
                    );
                });
            }
            if let Some(p) = jump_remote.take() {
                self.spawn_list(&handle, p, ctx);
            }
            if self.tree_busy {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(crate::i18n::tr(
                            ctx,
                            "Scanning files…",
                            "正在统计文件…",
                        ))
                        .small()
                        .color(theme.amber_color()),
                    );
                    if ui
                        .small_button(crate::i18n::tr(ctx, "Cancel", "取消"))
                        .clicked()
                    {
                        self.cancel_tree_collect();
                    }
                });
            }

            if let Some(prompt) = self.pending_delete.clone() {
                ui.horizontal(|ui| {
                    let label = match &prompt {
                        PendingDeletePrompt::One(p) => format!(
                            "{} {}",
                            crate::i18n::tr(ui.ctx(), "Delete?", "删除？"),
                            p.to_string_lossy()
                        ),
                        PendingDeletePrompt::Many(v) => format!(
                            "{} {} {}",
                            crate::i18n::tr(ui.ctx(), "Delete", "删除"),
                            v.len(),
                            crate::i18n::tr(ui.ctx(), "remote items?", "个远端项？")
                        ),
                    };
                    ui.label(egui::RichText::new(label).small());
                    if crate::ui::chrome::panel_action_primary_icon_button(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Trash,
                        crate::i18n::tr(ui.ctx(), "Confirm", "确认"),
                    )
                    .clicked()
                    {
                        match self.pending_delete.take().unwrap() {
                            PendingDeletePrompt::One(path) => {
                                self.spawn_remove(&handle, path, ctx);
                            }
                            PendingDeletePrompt::Many(paths) => {
                                self.spawn_remove_many(&handle, paths, ctx);
                            }
                        }
                    }
                    if crate::ui::chrome::panel_action_icon_button(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Cross,
                        crate::i18n::tr(ui.ctx(), "Cancel", "取消"),
                    )
                    .clicked()
                    {
                        self.pending_delete = None;
                    }
                });
            }
            if let Some(ow) = self.pending_overwrite.as_ref() {
                let (preview, source_size, dest_size, can_resume) = match ow {
                    PendingOverwrite::Upload {
                        remote,
                        source_size,
                        dest_size,
                        ..
                    } => (
                        remote.to_string_lossy().into_owned(),
                        *source_size,
                        *dest_size,
                        *dest_size > 0 && *dest_size < *source_size,
                    ),
                    PendingOverwrite::Download {
                        local,
                        source_size,
                        dest_size,
                        ..
                    } => (
                        local.to_string_lossy().into_owned(),
                        *source_size,
                        *dest_size,
                        *dest_size > 0 && *dest_size < *source_size,
                    ),
                    PendingOverwrite::UploadMany { conflict_names, .. } => {
                        let n = conflict_names.len();
                        let sample = conflict_names
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ");
                        let preview = if n > 3 {
                            format!("{sample}… (+{})", n - 3)
                        } else {
                            sample
                        };
                        (preview, 0, 0, false)
                    }
                };
                ui.vertical(|ui| {
                    let label = if source_size > 0 || dest_size > 0 {
                        format!(
                            "{} {}  ({} → {})",
                            crate::i18n::tr(ui.ctx(), "Target exists:", "目标已存在:"),
                            preview,
                            format_bytes(dest_size),
                            format_bytes(source_size),
                        )
                    } else {
                        format!(
                            "{} {}",
                            crate::i18n::tr(ui.ctx(), "Target exists:", "目标已存在:"),
                            preview
                        )
                    };
                    ui.label(
                        egui::RichText::new(label)
                            .small()
                            .color(theme.amber_color()),
                    );
                    ui.horizontal_wrapped(|ui| {
                        if can_resume
                            && ui
                                .small_button(crate::i18n::tr(ui.ctx(), "Resume", "续传"))
                                .clicked()
                        {
                            self.confirm_overwrite_with_mode(
                                &handle,
                                ctx,
                                WriteMode::Resume,
                                None,
                            );
                        }
                        if can_resume
                            && ui
                                .small_button(crate::i18n::tr(ui.ctx(), "Resume all", "全部续传"))
                                .clicked()
                        {
                            self.confirm_overwrite_with_mode(
                                &handle,
                                ctx,
                                WriteMode::Resume,
                                Some(OverwritePolicy::ResumeAll),
                            );
                        }
                        if ui
                            .small_button(crate::i18n::tr(ui.ctx(), "Overwrite", "覆盖"))
                            .clicked()
                        {
                            self.confirm_overwrite_with_mode(
                                &handle,
                                ctx,
                                WriteMode::Overwrite,
                                None,
                            );
                        }
                        if ui
                            .small_button(crate::i18n::tr(ui.ctx(), "Overwrite all", "全部覆盖"))
                            .clicked()
                        {
                            self.confirm_overwrite_with_mode(
                                &handle,
                                ctx,
                                WriteMode::Overwrite,
                                Some(OverwritePolicy::OverwriteAll),
                            );
                        }
                        if ui
                            .small_button(crate::i18n::tr(
                                ui.ctx(),
                                "Overwrite if newer",
                                "仅当更新时覆盖",
                            ))
                            .clicked()
                        {
                            // 单次先覆盖；并把后续冲突策略设为「源更新才覆盖」。
                            self.confirm_overwrite_with_mode(
                                &handle,
                                ctx,
                                WriteMode::Overwrite,
                                Some(OverwritePolicy::IfNewerAll),
                            );
                        }
                        if ui
                            .small_button(crate::i18n::tr(ui.ctx(), "Skip", "跳过"))
                            .clicked()
                        {
                            self.skip_pending_overwrite(&handle, ctx, None);
                        }
                        if ui
                            .small_button(crate::i18n::tr(ui.ctx(), "Skip all", "全部跳过"))
                            .clicked()
                        {
                            self.skip_pending_overwrite(
                                &handle,
                                ctx,
                                Some(OverwritePolicy::SkipAll),
                            );
                        }
                        if ui
                            .small_button(crate::i18n::tr(ui.ctx(), "Cancel", "取消"))
                            .clicked()
                        {
                            self.pending_overwrite = None;
                        }
                    });
                });
            }
            let mut enter_remote: Option<PathBuf> = None;
            let remote_block_h = remote_list_h + theme.size_file_list_row_h();
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width().max(1.0), remote_block_h),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
            Self::paint_file_list_viewport_frame(theme).show(ui, |ui| {
                layout_util::set_width_to_available(ui);
                let table_cols = FileTableCols::for_list_ui(ui, content_w);
                if paint_file_table_header(ui, theme, ctx, table_cols, &mut self.remote_sort) {
                    self.apply_remote_sort();
                }
                egui::ScrollArea::vertical()
                    .id_source("sftp_remote_list")
                    .auto_shrink([false, false])
                    .max_height(remote_list_h)
                    .show(ui, |ui| {
                        ui.visuals_mut().extreme_bg_color = theme.color_file_list_bg();
                        ui.set_max_width(table_cols.total);
                        let has_dropped =
                            !ui.ctx().input(|i| i.raw.dropped_files.is_empty());
                        let is_hovering = ui.ctx().input(|i| {
                            i.pointer
                                .hover_pos()
                                .map_or(false, |pos| ui.clip_rect().contains(pos))
                        });
                        if has_dropped && is_hovering {
                            let files: Vec<PathBuf> = ui.ctx().input(|i| {
                                i.raw
                                    .dropped_files
                                    .iter()
                                    .filter_map(|f| f.path.clone())
                                    .collect()
                            });
                            if !files.is_empty() {
                                self.request_upload_many(&handle, self.cwd.clone(), files, ctx);
                            }
                        } else if ui.ctx().input(|i| {
                            i.raw.dropped_files.is_empty() && !i.raw.hovered_files.is_empty()
                        }) && is_hovering
                        {
                            ui.painter().rect_filled(
                                ui.clip_rect(),
                                0.0,
                                theme.color_sftp_row_hover(),
                            );
                            ui.painter().text(
                                ui.clip_rect().center(),
                                egui::Align2::CENTER_CENTER,
                                crate::i18n::tr(ui.ctx(), "Drop to upload", "拖入以上传"),
                                egui::FontId::proportional(theme.font_size_body()),
                                theme.text_primary(),
                            );
                        }
                        let remote_rows: Vec<SftpEntry> = self
                            .filtered_remote_entries()
                            .into_iter()
                            .cloned()
                            .collect();
                        let ordered =
                            remote_rows.iter().map(|e| e.path.clone()).collect::<Vec<_>>();
                        let (ctrl, shift) = ui.ctx().input(|i| {
                            (i.modifiers.ctrl || i.modifiers.command, i.modifiers.shift)
                        });
                        for e in &remote_rows {
                            let sel = self.remote_selected.contains(&e.path);
                            let size_lbl =
                                if e.is_dir { "—".to_string() } else { e.size_human() };
                            let time_lbl = format_file_mtime(e.modified);
                            let kind = classify_file_kind(&e.name, e.is_dir);
                            let tip = format!("{}  {}", e.path.to_string_lossy(), e.permissions);
                            let resp = paint_file_table_row(
                                ui,
                                theme,
                                table_cols,
                                &e.name,
                                &size_lbl,
                                &time_lbl,
                                kind,
                                sel,
                                &tip,
                            );
                            if resp.double_clicked() {
                                if e.is_dir {
                                    enter_remote = Some(e.path.clone());
                                } else {
                                    self.request_open_remote_file(&handle, e, ctx);
                                }
                            } else if resp.clicked() {
                                self.remote_selected.apply_click(
                                    e.path.clone(),
                                    &ordered,
                                    ctrl,
                                    shift,
                                );
                            }
                            resp.context_menu(|ui| {
                                self.remote_row_context_menu(ui, theme, ctx, &handle, e);
                            });
                        }
                        Self::paint_list_blank_context(ui, table_cols.total, |ui| {
                            self.xfer_context_menu(ui, theme, ctx, &handle);
                        });
                    });
            });
            });
            if let Some(d) = enter_remote {
                self.spawn_list(&handle, d, ctx);
            }
        });
        }

        self.paint_large_open_prompt(ui, ctx, theme, &handle);

        self.show_mkdir_dialog_ui(ctx, theme, &handle);
        self.show_rename_dialog_ui(ctx, theme, &handle);
        self.show_chmod_dialog_ui(ctx, theme, &handle);

        if !self.busy && self.rx.is_none() {
            self.kick_xfer_queue(&handle, ctx);
        }

        let strip_action = paint_xfer_strip(
            ui,
            theme,
            ctx,
            &mut self.xfer,
            &self.host_key,
            &self.cwd.display().to_string(),
            &self.local_cwd.display().to_string(),
            &mut self.flux_details_open,
        );
        self.handle_xfer_strip_action(strip_action, &handle, ctx);

        if self.busy && self.xfer.active_id().is_none() {
            ui.add_space(theme.spacing_panel_gap());
            ui.label(
                egui::RichText::new(crate::i18n::tr(
                    ui.ctx(),
                    "SFTP busy…",
                    "SFTP 处理中…",
                ))
                .small()
                .color(theme.text_tertiary()),
            );
        }
    }

    fn paint_flux_workspace(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        handle: &SshSessionHandle,
        content_w: f32,
    ) {
        let avail_h = ui.available_height().max(160.0);
        let reserve = if self.pending_overwrite.is_some()
            || self.pending_delete.is_some()
            || self.pending_large_open.is_some()
            || self.edit_dirty
            || self.pending_edit_conflict.is_some()
        {
            96.0
        } else {
            8.0
        };
        let panes_h = (avail_h - reserve - 52.0).max(140.0);
        let gap = 1.0;
        let pane_w = ((content_w - gap) * 0.5).max(96.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.allocate_ui_with_layout(
                egui::vec2(pane_w, panes_h),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.set_width(pane_w);
                    ui.push_id("sftp_flux_local", |ui| {
                        self.paint_flux_local_pane(ui, ctx, theme, handle, panes_h);
                    });
                },
            );
            let (div, _) = ui.allocate_exact_size(egui::vec2(gap, panes_h), egui::Sense::hover());
            ui.painter()
                .rect_filled(div, 0.0, theme.border_divider_color());
            ui.allocate_ui_with_layout(
                egui::vec2(pane_w, panes_h),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.set_width(pane_w);
                    ui.push_id("sftp_flux_remote", |ui| {
                        self.paint_flux_remote_pane(ui, ctx, theme, handle, panes_h);
                    });
                },
            );
        });
        self.paint_flux_prompts(ui, ctx, theme, handle);
    }

    fn clip_flux_pane(ui: &mut egui::Ui) -> f32 {
        let mut w = ui.available_width().min(ui.max_rect().width());
        if !w.is_finite() || w < 48.0 {
            w = 320.0;
        }
        w = w.clamp(96.0, 2000.0);
        let rect = egui::Rect::from_min_size(
            ui.max_rect().min,
            egui::vec2(w, ui.max_rect().height().max(1.0)),
        );
        ui.set_clip_rect(rect);
        ui.set_max_width(w);
        w
    }

    fn flux_tool_row(ui: &mut egui::Ui, theme: &Theme, add: impl FnOnce(&mut egui::Ui)) {
        let w = ui.max_rect().width().clamp(48.0, 2000.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 36.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 0.0, theme.color_sftp_bar_fill());
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            egui::Stroke::new(1.0, theme.border.to_color32()),
        );
        let mut child = ui.child_ui(rect, egui::Layout::left_to_right(egui::Align::Center));
        child.spacing_mut().item_spacing.x = 4.0;
        add(&mut child);
    }

    fn paint_flux_head_bar(
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        title: &str,
        selected_n: usize,
        path: &Path,
        scroll_id: &str,
        editing: &mut bool,
        path_edit: &mut String,
        edit_id: egui::Id,
        edit_hint: &str,
    ) -> Option<PathBuf> {
        let w = ui.max_rect().width().clamp(48.0, 2000.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 36.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 0.0, theme.color_sftp_bar_fill());
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            egui::Stroke::new(1.0, theme.border.to_color32()),
        );
        let title_font = egui::FontId::proportional(theme.font_size_caption());
        let title_galley = ui.fonts(|f| {
            f.layout_no_wrap(title.to_string(), title_font, theme.text_tertiary())
        });
        let title_y = rect.center().y - title_galley.size().y * 0.5;
        ui.painter()
            .galley(egui::pos2(rect.left() + 10.0, title_y), title_galley.clone());
        let mut path_left = rect.left() + 10.0 + title_galley.size().x + 12.0;
        if selected_n > 0 {
            let sel = format!(
                "{} {selected_n}",
                crate::i18n::tr(ctx, "Selected", "已选")
            );
            let sel_galley = ui.fonts(|f| {
                f.layout_no_wrap(sel, egui::FontId::proportional(theme.font_size_caption()), theme.accent_color())
            });
            let sel_y = rect.center().y - sel_galley.size().y * 0.5;
            ui.painter()
                .galley(egui::pos2(path_left, sel_y), sel_galley.clone());
            path_left += sel_galley.size().x + 10.0;
        }
        let path_rect = egui::Rect::from_min_max(
            egui::pos2(path_left, rect.top()),
            egui::pos2(rect.right() - 8.0, rect.bottom()),
        );
        if path_rect.width() < 24.0 {
            return None;
        }
        if *editing {
            let mut child =
                ui.child_ui(path_rect, egui::Layout::left_to_right(egui::Align::Center));
            crate::ui::chrome::form_singleline_field(
                &mut child,
                theme,
                edit_id,
                path_edit,
                edit_hint,
                path_rect.width(),
                false,
            );
            None
        } else {
            Self::paint_flux_crumbs(ui, theme, ctx, path, scroll_id, editing, path_rect)
        }
    }

    fn paint_flux_filter_bar(
        ui: &mut egui::Ui,
        theme: &Theme,
        id: egui::Id,
        text: &mut String,
        hint: &str,
        width: f32,
    ) {
        let w = width.clamp(48.0, 2000.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 36.0), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 0.0, theme.color_sftp_bar_fill());
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            egui::Stroke::new(1.0, theme.border.to_color32()),
        );
        let inner = rect.shrink2(egui::vec2(8.0, 4.0));
        let mut child = ui.child_ui(inner, egui::Layout::left_to_right(egui::Align::Center));
        crate::ui::chrome::form_singleline_field(&mut child, theme, id, text, hint, inner.width(), false);
    }

    fn paint_flux_crumbs(
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        path: &Path,
        scroll_id: &str,
        editing: &mut bool,
        rect: egui::Rect,
    ) -> Option<PathBuf> {
        let _ = ui.allocate_rect(rect, egui::Sense::hover());
        if ui.interact(rect, ui.id().with(scroll_id), egui::Sense::click()).double_clicked() {
            *editing = true;
        }
        let response = ui.interact(rect, ui.id().with((scroll_id, "path")), egui::Sense::click());
        let crumbs = sftp_file_table::path_crumbs(path);
        let mono = egui::FontId::monospace(theme.font_size_caption());
        let color = theme.text_secondary();
        let mut parts: Vec<(String, PathBuf, bool)> = Vec::new();
        let n = crumbs.len();
        for (i, (label, dest)) in crumbs.iter().enumerate() {
            if i > 0 {
                parts.push(("/".to_string(), dest.clone(), false));
            }
            parts.push((label.clone(), dest.clone(), i + 1 == n));
        }
        let mut start = 0usize;
        let width_of = |ui: &egui::Ui, from: usize| -> f32 {
            let text: String = parts.iter().skip(from).map(|(s, _, _)| s.as_str()).collect();
            let prefix = if from > 0 { "…/" } else { "" };
            ui.fonts(|f| {
                f.layout_no_wrap(
                    format!("{prefix}{text}"),
                    mono.clone(),
                    color,
                )
                .size()
                .x
            })
        };
        while start + 1 < parts.len() && width_of(ui, start) > rect.width() - 4.0 {
            start += 1;
        }
        let mut hits: Vec<(egui::Rect, PathBuf, bool)> = Vec::new();
        let mut x = rect.left() + 2.0;
        if start > 0 {
            let galley = ui.fonts(|f| {
                f.layout_no_wrap("…/".to_string(), mono.clone(), theme.text_tertiary())
            });
            let w = galley.size().x;
            ui.painter().galley(
                egui::pos2(x, rect.center().y - galley.size().y * 0.5),
                galley,
            );
            x += w;
        }
        for (label, dest, current) in parts.iter().skip(start) {
            let text_color = if *current {
                theme.text_primary()
            } else if label == "/" {
                theme.text_tertiary()
            } else {
                theme.text_secondary()
            };
            let galley = ui.fonts(|f| f.layout_no_wrap(label.clone(), mono.clone(), text_color));
            let w = galley.size().x;
            let top = rect.center().y - galley.size().y * 0.5;
            ui.painter().galley(egui::pos2(x, top), galley);
            if label != "/" {
                hits.push((
                    egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(w, rect.height())),
                    dest.clone(),
                    *current,
                ));
            }
            x += w;
        }
        let response = response.on_hover_text(crate::i18n::tr(
            ctx,
            "Double-click to type a path",
            "双击后输入路径",
        ));
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                for (hit, dest, current) in &hits {
                    if hit.contains(pos) && !current {
                        let _ = scroll_id;
                        return Some(dest.clone());
                    }
                }
            }
        }
        None
    }

    fn paint_flux_local_pane(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        handle: &SshSessionHandle,
        pane_h: f32,
    ) {
        let _ = pane_h;
        ui.spacing_mut().item_spacing.y = 0.0;
        let pane_w = Self::clip_flux_pane(ui);
        let local_cwd = self.local_cwd.clone();
        let local_hint = crate::platform::home_dir_display_hint();
        let local_edit_id = egui::Id::new("sftp_flux_local_path");
        let mut jump_local = Self::paint_flux_head_bar(
            ui,
            theme,
            ctx,
            crate::i18n::tr(ctx, "Local", "本机"),
            self.local_selected.len(),
            &local_cwd,
            "sftp_flux_local_crumbs",
            &mut self.local_path_editing,
            &mut self.local_path_edit,
            local_edit_id,
            &local_hint,
        );
        if self.local_path_editing {
            if ui.ctx().input(|i| i.key_pressed(egui::Key::Enter))
                && ui.memory(|m| m.has_focus(local_edit_id))
            {
                self.local_path_editing = false;
                self.try_navigate_local_path(ctx);
            }
            if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
                self.local_path_editing = false;
                self.sync_local_path_from_cwd();
            }
        }
        let up_ok = self.local_cwd.parent().is_some();
        let can_upload = self.has_local_upload_selection();
        Self::flux_tool_row(ui, theme, |ui| {
            if Self::flux_action(
                ui,
                theme,
                crate::ui::icons::IconId::ChevronUp,
                crate::i18n::tr(ctx, "Up", "上级"),
                crate::i18n::tr(ctx, "Up", "上级"),
                up_ok,
            ) {
                if let Some(parent) = self.local_cwd.parent() {
                    self.local_cwd = parent.to_path_buf();
                    self.sync_local_path_from_cwd();
                    self.local_selected.clear();
                    self.refresh_local_list();
                }
            }
            if Self::flux_action(
                ui,
                theme,
                crate::ui::icons::IconId::Refresh,
                crate::i18n::tr(ctx, "Refresh", "刷新"),
                crate::i18n::tr(ctx, "Refresh", "刷新"),
                true,
            ) {
                self.refresh_local_list();
            }
            if Self::flux_action(
                ui,
                theme,
                crate::ui::icons::IconId::Folder,
                crate::i18n::tr(ctx, "Browse", "浏览"),
                crate::i18n::tr(ctx, "Browse…", "浏览…"),
                true,
            ) {
                if let Some(dir) = FileDialog::new().pick_folder() {
                    self.local_cwd = dir;
                    self.sync_local_path_from_cwd();
                    self.local_selected.clear();
                    self.refresh_local_list();
                }
            }
            if Self::flux_action(
                ui,
                theme,
                crate::ui::icons::IconId::Upload,
                crate::i18n::tr(ctx, "Upload", "上传"),
                crate::i18n::tr(ctx, "Upload selected item(s)", "上传所选"),
                can_upload,
            ) {
                self.upload_selected_local(handle, ctx);
            }
            self.paint_bookmark_row(ui, theme, ctx, true, self.local_cwd.clone(), &mut jump_local);
        });
        Self::paint_flux_filter_bar(
            ui,
            theme,
            egui::Id::new("sftp_flux_local_filter"),
            &mut self.local_filter,
            crate::i18n::tr(ctx, "Filter by name", "按名字找"),
            pane_w,
        );
        if let Some(p) = jump_local.take() {
            if p.is_dir() {
                self.local_cwd = p;
                self.sync_local_path_from_cwd();
                self.local_selected.clear();
                self.refresh_local_list();
            }
        }
        let cols_w = ui.available_width().max(80.0);
        let table_cols = FileTableCols::for_width(cols_w);
        if paint_file_table_header(ui, theme, ctx, table_cols, &mut self.local_sort) {
            self.apply_local_sort();
        }
        let list_h = ui.available_height().max(72.0);
        let mut enter_local: Option<PathBuf> = None;
        ui.visuals_mut().extreme_bg_color = theme.color_scroll_extreme_bg();
        egui::ScrollArea::vertical()
            .id_source("sftp_flux_local_list")
            .auto_shrink([false, false])
            .max_height(list_h)
            .show(ui, |ui| {
                ui.set_max_width(table_cols.total);
                let local_rows: Vec<LocalEntry> =
                    self.filtered_local_entries().into_iter().cloned().collect();
                let ordered = local_rows.iter().map(|e| e.path.clone()).collect::<Vec<_>>();
                let (ctrl, shift) = ui.ctx().input(|i| {
                    (i.modifiers.ctrl || i.modifiers.command, i.modifiers.shift)
                });
                for e in &local_rows {
                    let sel = self.local_selected.contains(&e.path);
                    let size_lbl = if e.is_dir {
                        "—".to_string()
                    } else {
                        e.size_human()
                    };
                    let resp = paint_file_table_row(
                        ui,
                        theme,
                        table_cols,
                        &e.name,
                        &size_lbl,
                        &format_file_mtime(e.modified),
                        classify_file_kind(&e.name, e.is_dir),
                        sel,
                        &e.path.display().to_string(),
                    );
                    if resp.double_clicked() && e.is_dir {
                        enter_local = Some(e.path.clone());
                    } else if resp.clicked() {
                        self.local_selected
                            .apply_click(e.path.clone(), &ordered, ctrl, shift);
                    }
                    resp.context_menu(|ui| {
                        self.local_entry_context_menu(ui, theme, ctx, handle, e);
                    });
                }
            });
        if let Some(d) = enter_local {
            self.local_cwd = d;
            self.sync_local_path_from_cwd();
            self.local_selected.clear();
            self.refresh_local_list();
        }
        if let Some(err) = &self.local_list_err {
            ui.label(
                egui::RichText::new(Self::localize_local_list_error(ctx, err))
                    .small()
                    .color(theme.red_color()),
            );
        }
    }

    fn paint_flux_remote_pane(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        handle: &SshSessionHandle,
        pane_h: f32,
    ) {
        let _ = pane_h;
        ui.spacing_mut().item_spacing.y = 0.0;
        let pane_w = Self::clip_flux_pane(ui);
        let remote_cwd = self.cwd.clone();
        let remote_edit_id = egui::Id::new("sftp_flux_remote_path");
        let mut jump_remote = Self::paint_flux_head_bar(
            ui,
            theme,
            ctx,
            crate::i18n::tr(ctx, "Remote", "远端"),
            self.remote_selected.len(),
            &remote_cwd,
            "sftp_flux_remote_crumbs",
            &mut self.remote_path_editing,
            &mut self.path_edit,
            remote_edit_id,
            "/",
        );
        if self.remote_path_editing {
            if ui.ctx().input(|i| i.key_pressed(egui::Key::Enter))
                && ui.memory(|m| m.has_focus(remote_edit_id))
            {
                self.remote_path_editing = false;
                self.spawn_list(handle, PathBuf::from(self.path_edit.trim()), ctx);
            }
            if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
                self.remote_path_editing = false;
                self.path_edit = self.cwd.display().to_string();
            }
        }
        let busy = !self.busy;
        let can_download = self.has_remote_download_selection();
        Self::flux_tool_row(ui, theme, |ui| {
            if Self::flux_action(
                ui,
                theme,
                crate::ui::icons::IconId::ChevronUp,
                crate::i18n::tr(ctx, "Up", "上级"),
                crate::i18n::tr(ctx, "Up", "上级"),
                busy,
            ) {
                self.spawn_list(handle, Self::remote_parent_dir(&self.cwd), ctx);
            }
            if Self::flux_action(
                ui,
                theme,
                crate::ui::icons::IconId::Refresh,
                crate::i18n::tr(ctx, "Refresh", "刷新"),
                crate::i18n::tr(ctx, "Refresh", "刷新"),
                busy,
            ) {
                self.spawn_list(handle, self.cwd.clone(), ctx);
            }
            if Self::flux_action(
                ui,
                theme,
                crate::ui::icons::IconId::Plus,
                crate::i18n::tr(ctx, "New", "新建"),
                crate::i18n::tr(ctx, "New folder", "新建目录"),
                busy,
            ) {
                self.show_mkdir_dialog = true;
            }
            if Self::flux_action(
                ui,
                theme,
                crate::ui::icons::IconId::Download,
                crate::i18n::tr(ctx, "Download", "下载"),
                crate::i18n::tr(ctx, "Download selected remote item(s)", "下载所选"),
                can_download,
            ) {
                self.download_selected_remote(handle, ctx);
            }
            self.paint_bookmark_row(
                ui,
                theme,
                ctx,
                false,
                self.cwd.clone(),
                &mut jump_remote,
            );
        });
        Self::paint_flux_filter_bar(
            ui,
            theme,
            egui::Id::new("sftp_flux_remote_filter"),
            &mut self.remote_filter,
            crate::i18n::tr(ctx, "Filter by name", "按名字找"),
            pane_w,
        );
        if let Some(p) = jump_remote.take() {
            self.spawn_list(handle, p, ctx);
        }
        let cols_w = ui.available_width().max(80.0);
        let table_cols = FileTableCols::for_width(cols_w);
        if paint_file_table_header(ui, theme, ctx, table_cols, &mut self.remote_sort) {
            self.apply_remote_sort();
        }
        let list_h = ui.available_height().max(72.0);
        let mut enter_remote: Option<PathBuf> = None;
        ui.visuals_mut().extreme_bg_color = theme.color_scroll_extreme_bg();
        egui::ScrollArea::vertical()
            .id_source("sftp_flux_remote_list")
            .auto_shrink([false, false])
            .max_height(list_h)
            .show(ui, |ui| {
                let has_dropped = !ui.ctx().input(|i| i.raw.dropped_files.is_empty());
                let is_hovering = ui.ctx().input(|i| {
                    i.pointer
                        .hover_pos()
                        .map_or(false, |pos| ui.clip_rect().contains(pos))
                });
                if has_dropped && is_hovering {
                    let files: Vec<PathBuf> = ui.ctx().input(|i| {
                        i.raw
                            .dropped_files
                            .iter()
                            .filter_map(|f| f.path.clone())
                            .collect()
                    });
                    if !files.is_empty() {
                        self.request_upload_many(handle, self.cwd.clone(), files, ctx);
                    }
                }
                ui.set_max_width(table_cols.total);
                let remote_rows: Vec<crate::ssh::SftpEntry> =
                    self.filtered_remote_entries().into_iter().cloned().collect();
                let ordered = remote_rows.iter().map(|e| e.path.clone()).collect::<Vec<_>>();
                let (ctrl, shift) = ui.ctx().input(|i| {
                    (i.modifiers.ctrl || i.modifiers.command, i.modifiers.shift)
                });
                for e in &remote_rows {
                    let sel = self.remote_selected.contains(&e.path);
                    let size_lbl = if e.is_dir {
                        "—".to_string()
                    } else {
                        e.size_human()
                    };
                    let tip = format!("{}  {}", e.path.to_string_lossy(), e.permissions);
                    let resp = paint_file_table_row(
                        ui,
                        theme,
                        table_cols,
                        &e.name,
                        &size_lbl,
                        &format_file_mtime(e.modified),
                        classify_file_kind(&e.name, e.is_dir),
                        sel,
                        &tip,
                    );
                    if resp.double_clicked() {
                        if e.is_dir {
                            enter_remote = Some(e.path.clone());
                        } else {
                            self.request_open_remote_file(handle, e, ctx);
                        }
                    } else if resp.clicked() {
                        self.remote_selected
                            .apply_click(e.path.clone(), &ordered, ctrl, shift);
                    }
                    resp.context_menu(|ui| {
                        self.remote_row_context_menu(ui, theme, ctx, handle, e);
                    });
                }
            });
        if let Some(d) = enter_remote {
            self.spawn_list(handle, d, ctx);
        }
        if let Some(err) = self.list_err.clone() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(Self::localize_list_error(ctx, &err))
                        .small()
                        .color(theme.red_color()),
                );
                if ui
                    .small_button(crate::i18n::tr(ctx, "Retry", "重试"))
                    .clicked()
                {
                    self.list_err = None;
                    self.spawn_list(handle, self.cwd.clone(), ctx);
                }
            });
        }
        if self.tree_busy {
            ui.label(
                egui::RichText::new(crate::i18n::tr(ctx, "Scanning files…", "正在统计文件…"))
                    .small()
                    .color(theme.amber_color()),
            );
        }
    }

    fn paint_flux_prompts(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        handle: &SshSessionHandle,
    ) {
        if self.edit_dirty {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ctx,
                        "Local copy changed — upload to server?",
                        "本地改过了，要传回服务器吗？",
                    ))
                    .small()
                    .color(theme.amber_color()),
                );
                if ui
                    .small_button(crate::i18n::tr(ctx, "Upload", "上传"))
                    .clicked()
                {
                    self.request_edit_upload(handle, ctx);
                }
                if ui
                    .small_button(crate::i18n::tr(ctx, "Discard", "放弃"))
                    .clicked()
                {
                    self.edit_dirty = false;
                }
            });
        }
        if let Some((remote, local)) = self.pending_edit_conflict.take() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ctx,
                        "Remote file changed on server — overwrite anyway?",
                        "服务器上的文件已变，仍要覆盖？",
                    ))
                    .small()
                    .color(theme.red_color()),
                );
                if ui
                    .small_button(crate::i18n::tr(ctx, "Upload anyway", "仍上传"))
                    .clicked()
                {
                    self.deferred_upload_files.push((local, remote));
                }
            });
        }
        if let Some(prompt) = self.pending_delete.clone() {
            ui.horizontal(|ui| {
                let label = match &prompt {
                    PendingDeletePrompt::One(p) => format!(
                        "{} {}",
                        crate::i18n::tr(ctx, "Delete?", "删除？"),
                        p.to_string_lossy()
                    ),
                    PendingDeletePrompt::Many(v) => format!(
                        "{} {} {}",
                        crate::i18n::tr(ctx, "Delete", "删除"),
                        v.len(),
                        crate::i18n::tr(ctx, "remote items?", "个远端项？")
                    ),
                };
                ui.label(egui::RichText::new(label).small());
                if ui
                    .small_button(crate::i18n::tr(ctx, "Confirm", "确认"))
                    .clicked()
                {
                    match self.pending_delete.take().unwrap() {
                        PendingDeletePrompt::One(path) => self.spawn_remove(handle, path, ctx),
                        PendingDeletePrompt::Many(paths) => {
                            self.spawn_remove_many(handle, paths, ctx)
                        }
                    }
                }
                if ui
                    .small_button(crate::i18n::tr(ctx, "Cancel", "取消"))
                    .clicked()
                {
                    self.pending_delete = None;
                }
            });
        }
        if self.pending_overwrite.is_some() {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(crate::i18n::tr(
                        ctx,
                        "A file with this name already exists.",
                        "已经有同名文件。",
                    ))
                    .small()
                    .color(theme.amber_color()),
                );
                if ui
                    .small_button(crate::i18n::tr(ctx, "Overwrite", "覆盖"))
                    .clicked()
                {
                    self.confirm_overwrite_with_mode(handle, ctx, WriteMode::Overwrite, None);
                }
                if ui
                    .small_button(crate::i18n::tr(ctx, "Overwrite all", "全部覆盖"))
                    .clicked()
                {
                    self.confirm_overwrite_with_mode(
                        handle,
                        ctx,
                        WriteMode::Overwrite,
                        Some(OverwritePolicy::OverwriteAll),
                    );
                }
                if ui
                    .small_button(crate::i18n::tr(ctx, "Skip", "跳过"))
                    .clicked()
                {
                    self.skip_pending_overwrite(handle, ctx, None);
                }
                if ui
                    .small_button(crate::i18n::tr(ctx, "Skip all", "全部跳过"))
                    .clicked()
                {
                    self.skip_pending_overwrite(handle, ctx, Some(OverwritePolicy::SkipAll));
                }
                if ui
                    .small_button(crate::i18n::tr(ctx, "Resume", "续传"))
                    .clicked()
                {
                    self.confirm_overwrite_with_mode(handle, ctx, WriteMode::Resume, None);
                }
                if ui
                    .small_button(crate::i18n::tr(ctx, "Cancel", "取消"))
                    .clicked()
                {
                    self.pending_overwrite = None;
                }
            });
        }
    }

    fn paint_large_open_prompt(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        theme: &Theme,
        handle: &SshSessionHandle,
    ) {
        let Some(entry) = self.pending_large_open.clone() else {
            return;
        };
        ui.add_space(theme.spacing_xs());
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{}（{}）。{}",
                    entry.name,
                    format_bytes(entry.size),
                    crate::i18n::tr(
                        ctx,
                        "This file is large. Download it to a temp file and open?",
                        "文件比较大，下载到临时文件后再打开？",
                    )
                ))
                .small()
                .color(theme.amber_color()),
            );
            if ui
                .small_button(crate::i18n::tr(ctx, "Download and open", "下载并打开"))
                .clicked()
                && !self.busy
                && self.rx.is_none()
            {
                self.pending_large_open = None;
                self.start_remote_edit(handle, &entry, ctx);
            }
            if ui
                .small_button(crate::i18n::tr(ctx, "Cancel", "取消"))
                .clicked()
            {
                self.pending_large_open = None;
            }
        });
    }

    fn paint_browser_section_frame(theme: &Theme) -> egui::Frame {
        if theme.uses_modern_palette() {
            egui::Frame::none().inner_margin(egui::Margin::symmetric(0.0, theme.spacing_xs()))
        } else {
            egui::Frame::none()
                .fill(theme.color_sftp_section_fill())
                .stroke(theme.sftp_section_stroke())
                .rounding(theme.radius_panel())
                .inner_margin(egui::Margin::symmetric(
                    theme.spacing_body_pad(),
                    theme.spacing_body_pad(),
                ))
        }
    }

    fn paint_file_list_viewport_frame(theme: &Theme) -> egui::Frame {
        egui::Frame::none()
            .fill(theme.color_file_list_bg())
            .stroke(if theme.uses_modern_palette() {
                egui::Stroke::NONE
            } else {
                theme.sftp_list_viewport_stroke()
            })
            .rounding(theme.radius_panel())
            .inner_margin(egui::Margin::symmetric(
                theme.spacing_sm(),
                theme.spacing_sm(),
            ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn local_entry(name: &str) -> LocalEntry {
        LocalEntry {
            name: name.to_string(),
            is_dir: false,
            size: 12,
            modified: Utc::now(),
            path: PathBuf::from(r"C:\temp").join(name),
        }
    }

    #[test]
    fn shell_cd_command_quotes_spaces_and_quotes() {
        let cmd = SftpPanel::shell_cd_command(Path::new("/tmp/my dir"));
        assert_eq!(cmd, "cd '/tmp/my dir'");
        let cmd = SftpPanel::shell_cd_command(Path::new("/tmp/a'b"));
        assert!(cmd.starts_with("cd '"));
        assert!(cmd.contains("'\\''"));
    }

    #[test]
    fn pick_local_by_name_finds_file() {
        let entries = vec![
            local_entry("a.txt"),
            local_entry("gui_e2e_upload.txt"),
        ];
        let path = SftpPanel::pick_local_by_name(&entries, "gui_e2e_upload.txt").unwrap();
        assert_eq!(path.file_name().unwrap(), "gui_e2e_upload.txt");
    }

    #[test]
    fn pick_local_by_name_skips_dirs() {
        let mut dir = local_entry("ignored");
        dir.is_dir = true;
        let entries = vec![dir];
        assert!(SftpPanel::pick_local_by_name(&entries, "ignored").is_none());
    }

    #[test]
    fn pick_remote_by_name_finds_file() {
        use crate::ssh::SftpEntry;
        let entries = vec![SftpEntry {
            name: "gui_e2e_upload.txt".to_string(),
            is_dir: false,
            size: 12,
            permissions: "-rw-r--r--".to_string(),
            modified: Utc::now(),
            path: PathBuf::from("/tmp/gui_e2e_upload.txt"),
        }];
        let path = SftpPanel::pick_remote_by_name(&entries, "gui_e2e_upload.txt").unwrap();
        assert_eq!(path.file_name().unwrap(), "gui_e2e_upload.txt");
    }

    #[test]
    fn gui_automation_enabled_reads_env() {
        struct EnvGuard {
            key: &'static str,
            old: Option<String>,
        }
        impl EnvGuard {
            fn set(key: &'static str, val: &str) -> Self {
                let old = std::env::var(key).ok();
                std::env::set_var(key, val);
                Self { key, old }
            }
        }
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                match &self.old {
                    Some(v) => std::env::set_var(self.key, v),
                    None => std::env::remove_var(self.key),
                }
            }
        }

        let _g = EnvGuard::set("MISTTERM_GUI_AUTOMATION", "1");
        assert!(SftpPanel::gui_automation_enabled());
        drop(_g);
        let _g2 = EnvGuard::set("MISTTERM_GUI_AUTOMATION", "true");
        assert!(SftpPanel::gui_automation_enabled());
    }
}