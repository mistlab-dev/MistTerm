//! SFTP 侧栏：本地 / 远端双栏文件浏览，表格式列表，经 shell 泵队列传输。

#[path = "sftp_file_table.rs"]
mod sftp_file_table;

use crate::core::{AuditCategory, AuditEvent, AuditLogger, AuditOutcome};
use crate::i18n::UiLanguage;
use crate::ssh::{SftpClient, SftpEntry, SshSessionHandle};
use crate::ui::layout_util;
use crate::ui::terminal::TerminalView;
use crate::ui::theme::Theme;
use chrono::Utc;
use eframe::egui::{self, Sense};
use rfd::FileDialog;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use sftp_file_table::{
    FileSortState, FileTableCols, LocalEntry, classify_file_kind, format_file_mtime,
    paint_file_table_header, paint_file_table_row, sort_local_entries, sort_remote_entries,
    system_time_to_utc,
};

enum SftpJobResult {
    Listed {
        dir: PathBuf,
        result: Result<Vec<SftpEntry>, String>,
    },
    Msg(Result<String, String>),
}

pub struct SftpPanel {
    /// 远端当前目录
    cwd: PathBuf,
    entries: Vec<SftpEntry>,
    path_edit: String,
    remote_selected: Option<PathBuf>,
    /// 本机当前目录
    local_cwd: PathBuf,
    local_entries: Vec<LocalEntry>,
    local_path_edit: String,
    local_selected: Option<PathBuf>,
    local_list_err: Option<String>,
    list_err: Option<String>,
    /// 待写入底栏 `status_message` 的成功/失败提示(不在面板内渲染)。
    pending_status_ok: Option<String>,
    pending_status_err: Option<String>,
    busy: bool,
    rx: Option<Receiver<SftpJobResult>>,
    mkdir_name: String,
    show_mkdir_dialog: bool,
    pending_delete: Option<PathBuf>,
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

    /// 本机选中文件 → (远端目标路径, 本机源路径)。
    fn local_upload_job(&self) -> Option<(PathBuf, PathBuf)> {
        self.local_selected.as_ref().and_then(|p| {
            self.local_entries
                .iter()
                .find(|e| &e.path == p && !e.is_dir)
                .map(|e| (self.cwd.join(&e.name), e.path.clone()))
        })
    }

    /// 远端选中文件 → (远端源路径, 本机目标路径)。
    fn remote_download_job(&self) -> Option<(PathBuf, PathBuf)> {
        self.remote_selected.as_ref().and_then(|p| {
            self.entries
                .iter()
                .find(|e| &e.path == p && !e.is_dir)
                .map(|e| (e.path.clone(), self.local_cwd.join(&e.name)))
        })
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
        let delete = self.remote_selected.clone();
        let upload_lbl = crate::i18n::tr(ctx, "Upload", "上传");
        let download_lbl = crate::i18n::tr(ctx, "Download", "下载");
        let delete_lbl = crate::i18n::tr(ctx, "Delete remote", "删除远端");
        if let Some((remote, local)) = upload {
            if crate::ui::chrome::popup_menu_button(ui, theme, upload_lbl).clicked() {
                self.spawn_upload(handle, remote, local, ctx);
                ui.close_menu();
            }
        }
        if let Some((remote, local)) = download {
            if crate::ui::chrome::popup_menu_button(ui, theme, download_lbl).clicked() {
                self.spawn_download(handle, remote, local, ctx);
                ui.close_menu();
            }
        }
        if let Some(p) = delete {
            if crate::ui::chrome::popup_menu_button(ui, theme, delete_lbl).clicked() {
                self.pending_delete = Some(p);
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

    /// 本机分区 chrome(标题/路径/导航/表头，不含列表滚动区)。
    fn estimate_local_section_chrome(theme: &Theme) -> f32 {
        let band_h = theme.size_sftp_toolbar_row_h() + theme.spacing_xs() * 2.0;
        let caption = theme.font_size_caption() + theme.spacing_xs();
        theme.spacing_body_pad() * 2.0
            + caption
            + band_h
            + theme.spacing_xs()
            + band_h
            + theme.size_file_list_row_h()
    }

    /// 远端分区 chrome(标题/路径/导航/表头，不含列表滚动区；新建目录已改为弹窗)。
    fn estimate_remote_section_chrome(theme: &Theme) -> f32 {
        let band_h = theme.size_sftp_toolbar_row_h() + theme.spacing_xs() * 2.0;
        let caption = theme.font_size_caption() + theme.spacing_xs();
        theme.spacing_body_pad() * 2.0
            + caption
            + band_h
            + theme.spacing_xs()
            + band_h
            + theme.size_file_list_row_h()
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
            remote_selected: None,
            local_cwd: local_root.clone(),
            local_entries: Vec::new(),
            local_path_edit: local_root.to_string_lossy().into_owned(),
            local_selected: None,
            local_list_err: None,
            list_err: None,
            pending_status_ok: None,
            pending_status_err: None,
            busy: false,
            rx: None,
            mkdir_name: String::new(),
            show_mkdir_dialog: false,
            pending_delete: None,
            pending_refresh_after_op: false,
            pending_auto_list: false,
            pending_audit: None,
            deferred_gui_download: None,
            last_panel_slot_rect: None,
            local_sort: FileSortState::default(),
            remote_sort: FileSortState::default(),
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
        self.remote_selected = None;
        let local_root = std::env::temp_dir().join("mistterm_downloads");
        let _ = std::fs::create_dir_all(&local_root);
        self.local_cwd = local_root.clone();
        self.local_entries.clear();
        self.local_path_edit = local_root.to_string_lossy().into_owned();
        self.local_selected = None;
        self.local_list_err = None;
        self.list_err = None;
        self.pending_status_ok = None;
        self.pending_status_err = None;
        self.busy = false;
        self.rx = None;
        self.mkdir_name.clear();
        self.show_mkdir_dialog = false;
        self.pending_delete = None;
        self.pending_refresh_after_op = false;
        self.pending_auto_list = false;
        self.pending_audit = None;
        self.deferred_gui_download = None;
        self.last_panel_slot_rect = None;
    }

    /// 轮询 SFTP 后台任务；面板未显示时也需调用以便 GUI 自动化下载完成。
    pub fn tick_sftp_jobs(
        &mut self,
        handle: &SshSessionHandle,
        ctx: &egui::Context,
        audit: &AuditLogger,
    ) {
        self.poll_rx(audit, crate::i18n::language(ctx));
        if !self.busy && self.rx.is_none() {
            if let Some((remote, local)) = self.deferred_gui_download.take() {
                self.spawn_download(handle, remote, local, ctx);
            }
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
                        self.cwd = dir;
                        self.sync_remote_path_from_cwd();
                        self.list_err = None;
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
                        if let Some((action, resource)) = self.pending_audit.take() {
                            audit.record(
                                AuditEvent::new(
                                    AuditCategory::Session,
                                    action,
                                    AuditOutcome::Success,
                                )
                                .with_resource(&resource),
                            );
                            if let Some(scp_action) = action.strip_prefix("sftp.") {
                                audit.record(
                                    AuditEvent::new(
                                        AuditCategory::Session,
                                        format!("file.scp.{scp_action}"),
                                        AuditOutcome::Success,
                                    )
                                    .with_resource(resource),
                                );
                            }
                        }
                        self.pending_status_ok = Some(msg);
                        self.pending_refresh_after_op = true;
                        self.refresh_local_list();
                    }
                    Err(e) => {
                        if let Some((action, resource)) = self.pending_audit.take() {
                            audit.record(
                                AuditEvent::new(
                                    AuditCategory::Session,
                                    action,
                                    AuditOutcome::Failure,
                                )
                                .with_resource(&resource)
                                .with_detail(serde_json::json!({ "error": e })),
                            );
                            if let Some(scp_action) = action.strip_prefix("sftp.") {
                                audit.record(
                                    AuditEvent::new(
                                        AuditCategory::Session,
                                        format!("file.scp.{scp_action}"),
                                        AuditOutcome::Failure,
                                    )
                                    .with_resource(resource)
                                    .with_detail(serde_json::json!({ "error": e })),
                                );
                            }
                        }
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
                self.pending_status_err = Some(
                    crate::i18n::Locale::from(lang)
                        .tr(
                            "SFTP background worker stopped unexpectedly",
                            "SFTP 后台任务异常中断",
                        )
                        .to_string(),
                );
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
        self.local_cwd = p;
        self.sync_local_path_from_cwd();
        self.refresh_local_list();
    }

    fn refresh_local_list(&mut self) {
        match Self::list_local_dir(&self.local_cwd) {
            Ok(entries) => {
                self.local_entries = entries;
                self.apply_local_sort();
                self.local_list_err = None;
                if let Some(sel) = &self.local_selected {
                    if !sel.starts_with(&self.local_cwd) {
                        self.local_selected = None;
                    }
                }
            }
            Err(e) => {
                self.local_entries.clear();
                self.local_selected = None;
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

    fn spawn_list(&mut self, handle: &SshSessionHandle, dir: PathBuf, ctx: &egui::Context) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.enqueue(handle, ctx, move |session| {
            let result = (|| -> Result<Vec<SftpEntry>, String> {
                let client = SftpClient::new(session)?;
                client.list_dir(&dir)
            })();
            SftpJobResult::Listed { dir, result }
        });
    }

    fn spawn_upload(
        &mut self,
        handle: &SshSessionHandle,
        remote: PathBuf,
        local: PathBuf,
        ctx: &egui::Context,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.pending_audit = Some((
            "sftp.upload",
            remote.to_string_lossy().into_owned(),
        ));
        let lang = crate::i18n::language(ctx);
        self.enqueue(handle, ctx, move |session| {
            let msg = (|| -> Result<String, String> {
                let client = SftpClient::new(session)?;
                let n = client.upload(&local, &remote)?;
                Ok(match lang {
                    UiLanguage::En => format!(
                        "Uploaded {} bytes → {}",
                        n,
                        remote.to_string_lossy()
                    ),
                    UiLanguage::Zh => format!(
                        "已上传 {} bytes → {}",
                        n,
                        remote.to_string_lossy()
                    ),
                })
            })();
            SftpJobResult::Msg(msg)
        });
    }

    fn spawn_download(
        &mut self,
        handle: &SshSessionHandle,
        remote: PathBuf,
        local: PathBuf,
        ctx: &egui::Context,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.pending_audit = Some((
            "sftp.download",
            remote.to_string_lossy().into_owned(),
        ));
        let lang = crate::i18n::language(ctx);
        self.enqueue(handle, ctx, move |session| {
            let msg = (|| -> Result<String, String> {
                let client = SftpClient::new(session)?;
                let n = client.download(&remote, &local)?;
                Ok(match lang {
                    UiLanguage::En => format!(
                        "Downloaded {} → {} bytes",
                        remote.to_string_lossy(),
                        n
                    ),
                    UiLanguage::Zh => format!(
                        "已下载 {} → {} bytes",
                        remote.to_string_lossy(),
                        n
                    ),
                })
            })();
            SftpJobResult::Msg(msg)
        });
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
                self.local_selected = Some(path);
            }
        } else if self.local_selected.is_none() {
            if let Some(e) = self.local_entries.iter().find(|e| !e.is_dir) {
                self.local_selected = Some(e.path.clone());
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
        } else if let Some(path) = self.remote_selected.clone() {
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

    fn spawn_upload_many(
        &mut self,
        handle: &SshSessionHandle,
        cwd: PathBuf,
        locals: Vec<PathBuf>,
        ctx: &egui::Context,
    ) {
        if self.busy || locals.is_empty() {
            return;
        }
        self.busy = true;
        self.pending_audit = Some(("sftp.upload_batch", cwd.to_string_lossy().into_owned()));
        let lang = crate::i18n::language(ctx);
        self.enqueue(handle, ctx, move |session| {
            let msg = (|| -> Result<String, String> {
                let client = SftpClient::new(session)?;
                let mut ok_n = 0usize;
                let mut total_bytes = 0u64;
                let mut err_lines = Vec::new();
                for local in locals {
                    let fname = local
                        .file_name()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("upload.bin"));
                    let remote_path = cwd.join(&fname);
                    match client.upload(&local, &remote_path) {
                        Ok(n) => {
                            ok_n += 1;
                            total_bytes += n;
                        }
                        Err(e) => err_lines.push(format!("{}: {}", local.display(), e)),
                    }
                }
                if ok_n == 0 && !err_lines.is_empty() {
                    return Err(err_lines.join("\n"));
                }
                let mut s = match lang {
                    UiLanguage::En => format!(
                        "Uploaded {} file(s), {} bytes total",
                        ok_n, total_bytes
                    ),
                    UiLanguage::Zh => format!(
                        "已上传 {} 个文件，合计 {} bytes",
                        ok_n, total_bytes
                    ),
                };
                if !err_lines.is_empty() {
                    s.push_str(match lang {
                        UiLanguage::En => "\nSome uploads failed:\n",
                        UiLanguage::Zh => "\n部分失败：\n",
                    });
                    s.push_str(&err_lines.join("\n"));
                }
                Ok(s)
            })();
            SftpJobResult::Msg(msg)
        });
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
        self.poll_rx(audit, crate::i18n::language(ctx));

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
            *close_panel = true;
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

        ui.add_space(theme.spacing_sm());

        let (local_list_h, remote_list_h) = Self::split_file_list_heights(ui, theme);

        Self::paint_browser_section_frame(theme).show(ui, |ui| {
            layout_util::set_width_to_available(ui);
            ui.label(crate::ui::chrome::rich_caption(
                theme,
                crate::i18n::tr(ui.ctx(), "Local", "本机"),
            ));
            let local_path_id = egui::Id::new("sftp_local_path");
            let home_hint = crate::platform::home_dir_display_hint();
            let go_lbl = crate::i18n::tr(ui.ctx(), "Go", "前往");
            let up_lbl = crate::i18n::tr(ui.ctx(), "Up", "上级");
            let refresh_lbl = crate::i18n::tr(ui.ctx(), "Refresh", "刷新");
            let browse_short = crate::i18n::tr(ui.ctx(), "Browse", "浏览");
            let browse_tip = crate::i18n::tr(ui.ctx(), "Browse…", "浏览…");
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
            ];
            if theme.uses_modern_palette() {
                layout_util::set_width_to_available(ui);
                let _path_resp = crate::ui::chrome::sftp_path_toolbar_row(
                    ui,
                    theme,
                    local_path_id,
                    &mut self.local_path_edit,
                    &home_hint,
                );
                if ui.ctx().input(|i| i.key_pressed(egui::Key::Enter))
                    && ui.memory(|m| m.has_focus(local_path_id))
                {
                    self.try_navigate_local_path(ctx);
                }
                ui.add_space(theme.spacing_xs());
                if let Some(idx) = crate::ui::chrome::sftp_nav_toolbar_row(
                    ui,
                    theme,
                    &local_nav,
                    "sftp_local_nav",
                ) {
                    match idx {
                        0 => self.try_navigate_local_path(ctx),
                        1 => {
                            if let Some(parent) = self.local_cwd.parent() {
                                self.local_cwd = parent.to_path_buf();
                                self.sync_local_path_from_cwd();
                                self.refresh_local_list();
                            }
                        }
                        2 => self.refresh_local_list(),
                        3 => {
                            if let Some(dir) = FileDialog::new().pick_folder() {
                                self.local_cwd = dir;
                                self.sync_local_path_from_cwd();
                                self.refresh_local_list();
                            }
                        }
                        _ => {}
                    }
                }
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
                        ui.set_min_width(table_cols.total);
                        ui.set_max_width(table_cols.total);
                        let local_rows = self.local_entries.clone();
                        for e in &local_rows {
                            let sel = self.local_selected.as_ref() == Some(&e.path);
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
                            if resp.clicked() || resp.secondary_clicked() {
                                self.local_selected = Some(e.path.clone());
                            }
                            if resp.double_clicked() && e.is_dir {
                                enter_local = Some(e.path.clone());
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
                self.refresh_local_list();
            }
        });

        ui.add_space(theme.spacing_sm());

        Self::paint_browser_section_frame(theme).show(ui, |ui| {
            layout_util::set_width_to_available(ui);
            ui.label(crate::ui::chrome::rich_caption(
                theme,
                crate::i18n::tr(ui.ctx(), "Remote", "远端"),
            ));
            let remote_path_id = egui::Id::new("sftp_path_edit");
            let remote_path_hint = crate::i18n::tr(ui.ctx(), "/home/user", "/home/user");
            let go_lbl = crate::i18n::tr(ui.ctx(), "Go", "前往");
            let up_lbl = crate::i18n::tr(ui.ctx(), "Up", "上级");
            let refresh_lbl = crate::i18n::tr(ui.ctx(), "Refresh", "刷新");
            let new_short = crate::i18n::tr(ui.ctx(), "New", "新建");
            let new_tip = crate::i18n::tr(ui.ctx(), "New folder", "新建目录");
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
            ];
            if theme.uses_modern_palette() {
                layout_util::set_width_to_available(ui);
                crate::ui::chrome::sftp_path_toolbar_row(
                    ui,
                    theme,
                    remote_path_id,
                    &mut self.path_edit,
                    remote_path_hint,
                );
                ui.add_space(theme.spacing_xs());
                if let Some(idx) = crate::ui::chrome::sftp_nav_toolbar_row(
                    ui,
                    theme,
                    &remote_nav,
                    "sftp_remote_nav",
                ) {
                    match idx {
                        0 => self.spawn_list(&handle, PathBuf::from(self.path_edit.trim()), ctx),
                        1 => {
                            let parent = self
                                .cwd
                                .parent()
                                .map(PathBuf::from)
                                .unwrap_or_else(|| PathBuf::from("/"));
                            self.spawn_list(&handle, parent, ctx);
                        }
                        2 => self.spawn_list(&handle, self.cwd.clone(), ctx),
                        3 => self.show_mkdir_dialog = true,
                        _ => {}
                    }
                }
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
                        let parent = self
                            .cwd
                            .parent()
                            .map(PathBuf::from)
                            .unwrap_or_else(|| PathBuf::from("/"));
                        self.spawn_list(&handle, parent, ctx);
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
                });
            }

            if let Some(p) = self.pending_delete.clone() {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {}",
                            crate::i18n::tr(ui.ctx(), "Delete?", "删除?"),
                            p.to_string_lossy()
                        ))
                        .small(),
                    );
                    if crate::ui::chrome::panel_action_primary_icon_button(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Trash,
                        crate::i18n::tr(ui.ctx(), "Confirm", "确认"),
                    )
                    .clicked()
                    {
                        let path = self.pending_delete.take().unwrap();
                        self.spawn_remove(&handle, path, ctx);
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
                        ui.set_min_width(table_cols.total);
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
                                self.spawn_upload_many(&handle, self.cwd.clone(), files, ctx);
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
                        let remote_rows = self.entries.clone();
                        for e in &remote_rows {
                            let sel = self.remote_selected.as_ref() == Some(&e.path);
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
                                &e.path.to_string_lossy(),
                            );
                            if resp.clicked() || resp.secondary_clicked() {
                                self.remote_selected = Some(e.path.clone());
                            }
                            if resp.double_clicked() && e.is_dir {
                                enter_remote = Some(e.path.clone());
                            }
                            resp.context_menu(|ui| {
                                self.xfer_context_menu(ui, theme, ctx, &handle);
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

        self.show_mkdir_dialog_ui(ctx, theme, &handle);

        if self.busy {
            ui.add_space(theme.spacing_panel_gap());
            ui.label(egui::RichText::new(crate::i18n::tr(ui.ctx(), "SFTP busy…", "SFTP 处理中…")).small().color(theme.text_tertiary()));
        }
    }

    fn paint_browser_section_frame(theme: &Theme) -> egui::Frame {
        if theme.uses_modern_palette() {
            theme.frame_inset_section()
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
            .stroke(theme.sftp_list_viewport_stroke())
            .rounding(theme.radius_list_item())
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