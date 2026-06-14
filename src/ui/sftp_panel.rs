//! SFTP 侧栏：本地 / 远端双栏文件浏览，表格式列表，经 shell 泵队列传输。

use crate::core::{AuditCategory, AuditEvent, AuditLogger, AuditOutcome};
use crate::ssh::{SftpClient, SftpEntry, SshSessionHandle};
use crate::i18n::UiLanguage;
use crate::ui::terminal::TerminalView;
use crate::ui::layout_util;
use crate::ui::theme::Theme;
use chrono::{DateTime, Utc};
use eframe::egui::{self, Color32, RichText, Sense};
use rfd::FileDialog;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::SystemTime;

/// 本机目录项（`std::fs::read_dir`）
#[derive(Debug, Clone)]
struct LocalEntry {
    name: String,
    is_dir: bool,
    size: u64,
    modified: DateTime<Utc>,
    path: PathBuf,
}

impl LocalEntry {
    fn size_human(&self) -> String {
        format_file_size(self.size)
    }
}

fn system_time_to_utc(t: SystemTime) -> Option<DateTime<Utc>> {
    let dur = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    DateTime::from_timestamp(dur.as_secs() as i64, dur.subsec_nanos())
}

fn format_file_mtime(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M").to_string()
}

/// SFTP 文件列表行类型（用于文件名/图标前景色）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SftpFileKind {
    Dir,
    Hidden,
    Archive,
    Image,
    Code,
    Config,
    Document,
    Executable,
    Plain,
}

fn classify_file_kind(name: &str, is_dir: bool) -> SftpFileKind {
    if is_dir {
        return SftpFileKind::Dir;
    }
    if name.starts_with('.') && name != "." && name != ".." {
        return SftpFileKind::Hidden;
    }
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "zip" | "tar" | "gz" | "bz2" | "xz" | "tgz" | "tbz2" | "txz" | "7z" | "rar" | "jar"
        | "war" | "zst" | "lz4" => SftpFileKind::Archive,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp" | "heic" | "avif" => {
            SftpFileKind::Image
        }
        "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "go" | "java" | "kt" | "c" | "cc" | "cpp"
        | "h" | "hpp" | "cs" | "swift" | "rb" | "php" | "lua" | "sql" | "sh" | "bash" | "zsh"
        | "fish" | "vue" | "svelte" | "html" | "htm" | "css" | "scss" | "less" | "wasm" => {
            SftpFileKind::Code
        }
        "json" | "yaml" | "yml" | "toml" | "ini" | "conf" | "cfg" | "env" | "xml"
        | "properties" | "plist" => SftpFileKind::Config,
        "md" | "txt" | "pdf" | "doc" | "docx" | "rtf" | "csv" | "log" | "rst" => {
            SftpFileKind::Document
        }
        "exe" | "bin" | "deb" | "rpm" | "dmg" | "app" | "msi" => SftpFileKind::Executable,
        _ => SftpFileKind::Plain,
    }
}

fn file_kind_name_color(theme: &Theme, kind: SftpFileKind, selected: bool) -> Color32 {
    if selected {
        return theme.text_primary();
    }
    if theme.uses_modern_palette() {
        return match kind {
            SftpFileKind::Hidden => theme.text_tertiary(),
            SftpFileKind::Executable => theme.red_color(),
            _ => theme.text_primary(),
        };
    }
    match kind {
        SftpFileKind::Dir => theme.accent_color(),
        SftpFileKind::Hidden => theme.text_tertiary(),
        SftpFileKind::Archive => theme.amber_color(),
        SftpFileKind::Image => theme.green_color(),
        SftpFileKind::Code => theme.accent_color(),
        SftpFileKind::Config => theme.amber_color().gamma_multiply(0.88),
        SftpFileKind::Document => theme.text_secondary(),
        SftpFileKind::Executable => theme.red_color(),
        SftpFileKind::Plain => theme.text_secondary(),
    }
}

fn file_kind_meta_color(theme: &Theme, kind: SftpFileKind, selected: bool) -> Color32 {
    if selected {
        return theme.text_secondary();
    }
    if theme.uses_modern_palette() {
        let _ = kind;
        return theme.text_tertiary();
    }
    match kind {
        SftpFileKind::Dir | SftpFileKind::Hidden | SftpFileKind::Document | SftpFileKind::Plain => {
            theme.text_tertiary()
        }
        SftpFileKind::Archive => theme.amber_color().gamma_multiply(0.78),
        SftpFileKind::Image => theme.green_color().gamma_multiply(0.78),
        SftpFileKind::Code => theme.accent_color().gamma_multiply(0.78),
        SftpFileKind::Config => theme.amber_color().gamma_multiply(0.72),
        SftpFileKind::Executable => theme.red_color().gamma_multiply(0.78),
    }
}

fn file_kind_icon_color(theme: &Theme, kind: SftpFileKind, selected: bool) -> Color32 {
    if theme.uses_modern_palette() {
        if selected {
            return theme.text_primary();
        }
        return match kind {
            SftpFileKind::Dir => theme.amber_color(),
            SftpFileKind::Executable => theme.red_color(),
            _ => theme.text_secondary(),
        };
    }
    file_kind_name_color(theme, kind, selected)
}

fn file_kind_icon(kind: SftpFileKind) -> crate::ui::icons::IconId {
    match kind {
        SftpFileKind::Dir => crate::ui::icons::IconId::Folder,
        SftpFileKind::Archive => crate::ui::icons::IconId::Package,
        _ => crate::ui::icons::IconId::File,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileSortColumn {
    Name,
    Size,
    Time,
}

#[derive(Debug, Clone, Copy)]
struct FileSortState {
    column: FileSortColumn,
    ascending: bool,
}

impl Default for FileSortState {
    fn default() -> Self {
        Self {
            column: FileSortColumn::Name,
            ascending: true,
        }
    }
}

impl FileSortState {
    fn toggle_column(&mut self, col: FileSortColumn) {
        if self.column == col {
            self.ascending = !self.ascending;
        } else {
            self.column = col;
            self.ascending = true;
        }
    }
}

fn sort_header_suffix(sort: FileSortState, col: FileSortColumn) -> &'static str {
    if sort.column != col {
        return "";
    }
    if sort.ascending {
        " ▲"
    } else {
        " ▼"
    }
}

fn sort_local_entries(entries: &mut [LocalEntry], sort: FileSortState) {
    entries.sort_by(|a, b| {
        let dir_ord = b.is_dir.cmp(&a.is_dir);
        if dir_ord != std::cmp::Ordering::Equal {
            return dir_ord;
        }
        let ord = match sort.column {
            FileSortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            FileSortColumn::Size => a.size.cmp(&b.size),
            FileSortColumn::Time => a.modified.cmp(&b.modified),
        };
        if sort.ascending {
            ord
        } else {
            ord.reverse()
        }
    });
}

fn sort_remote_entries(entries: &mut [SftpEntry], sort: FileSortState) {
    entries.sort_by(|a, b| {
        let dir_ord = b.is_dir.cmp(&a.is_dir);
        if dir_ord != std::cmp::Ordering::Equal {
            return dir_ord;
        }
        let ord = match sort.column {
            FileSortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            FileSortColumn::Size => a.size.cmp(&b.size),
            FileSortColumn::Time => a.modified.cmp(&b.modified),
        };
        if sort.ascending {
            ord
        } else {
            ord.reverse()
        }
    });
}

fn format_file_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;
    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{size} B")
    }
}

/// 固定列宽表格布局（表头与各行共用，保证列对齐）
#[derive(Clone, Copy)]
struct FileTableCols {
    total: f32,
    icon: f32,
    name: f32,
    size: f32,
    time: f32,
}

impl FileTableCols {
    const ICON_W: f32 = 22.0;
    const SIZE_W: f32 = 56.0;
    const TIME_W: f32 = 110.0;

    fn from_panel_width(panel_w: f32) -> Self {
        let panel_w = panel_w.max(1.0);
        let icon = Self::ICON_W;
        let mut size = Self::SIZE_W;
        let mut time = Self::TIME_W;
        const MIN_NAME: f32 = 32.0;

        let fixed = icon + size + time;
        if panel_w >= fixed + MIN_NAME {
            let name = panel_w - fixed;
            return Self {
                total: panel_w,
                icon,
                name,
                size,
                time,
            };
        }

        let budget = (panel_w - icon - MIN_NAME).max(0.0);
        let flex = size + time;
        if flex > 0.0 && budget < flex {
            let scale = budget / flex;
            size = (size * scale).max(36.0);
            time = (time * scale).max(56.0);
        }
        let name = (panel_w - icon - size - time).max(0.0);
        Self {
            total: panel_w,
            icon,
            name,
            size,
            time,
        }
    }

    /// 按列表视口当前可用宽度计算列宽（预留竖向滚动条占位，避免「修改时间」等右列被切）。
    fn for_list_ui(ui: &mut egui::Ui, body_cap: f32) -> Self {
        layout_util::set_width_to_available(ui);
        Self::from_panel_width(layout_util::dock_scroll_viewport_width(ui, body_cap))
    }

    fn col_width(self, col: usize) -> f32 {
        match col {
            0 => self.icon,
            1 => self.name,
            2 => self.size,
            _ => self.time,
        }
    }

    fn col_layout(col: usize) -> egui::Layout {
        if col >= 2 {
            egui::Layout::right_to_left(egui::Align::Center)
        } else {
            egui::Layout::left_to_right(egui::Align::Center)
        }
    }
}

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
    toast_ok: Option<String>,
    toast_err: Option<String>,
    busy: bool,
    rx: Option<Receiver<SftpJobResult>>,
    mkdir_name: String,
    pending_delete: Option<PathBuf>,
    pending_refresh_after_op: bool,
    /// 面板打开后与切换标签时为 true，触发一次列表加载
    pending_auto_list: bool,
    /// 后台操作成功后待写入审计
    pending_audit: Option<(&'static str, String)>,
    /// 右 dock 槽位（用于 Central 之后前景重绘）
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
    /// 右 dock 正文区可用宽（与其它侧栏并排时须随槽位收缩）。
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

    /// 本机分区 chrome（标题/路径/导航/表头，不含列表滚动区）。
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

    /// 远端分区 chrome（标题/路径/导航/创建/表头，不含列表滚动区）。
    fn estimate_remote_section_chrome(theme: &Theme) -> f32 {
        let band_h = theme.size_sftp_toolbar_row_h() + theme.spacing_xs() * 2.0;
        let caption = theme.font_size_caption() + theme.spacing_xs();
        theme.spacing_body_pad() * 2.0
            + caption
            + band_h
            + theme.spacing_xs()
            + band_h
            + theme.spacing_xs()
            + band_h
            + theme.size_file_list_row_h()
    }

    /// 在本机/远端分区绘制前，一次性拆分两侧文件列表可用高度（各约一半）。
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
            toast_ok: None,
            toast_err: None,
            busy: false,
            rx: None,
            mkdir_name: String::new(),
            pending_delete: None,
            pending_refresh_after_op: false,
            pending_auto_list: false,
            pending_audit: None,
            last_panel_slot_rect: None,
            local_sort: FileSortState::default(),
            remote_sort: FileSortState::default(),
        }
    }

    pub fn request_list_on_open(&mut self) {
        self.pending_auto_list = true;
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
        self.toast_ok = None;
        self.toast_err = None;
        self.busy = false;
        self.rx = None;
        self.mkdir_name.clear();
        self.pending_delete = None;
        self.pending_refresh_after_op = false;
        self.pending_auto_list = false;
        self.pending_audit = None;
        self.last_panel_slot_rect = None;
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
                        self.toast_ok = Some(msg);
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
                        self.toast_err = Some(e);
                    }
                }
                self.busy = false;
                self.rx = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.busy = false;
                self.rx = None;
                self.toast_err = Some(
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

    fn table_cell(
        ui: &mut egui::Ui,
        cols: FileTableCols,
        col: usize,
        row_h: f32,
        add: impl FnOnce(&mut egui::Ui),
    ) {
        let w = cols.col_width(col);
        ui.allocate_ui_with_layout(egui::vec2(w, row_h), FileTableCols::col_layout(col), |ui| {
            ui.set_width(w);
            ui.set_min_width(w);
            ui.set_max_width(w);
            add(ui);
        });
    }

    fn paint_file_table_row_strip(
        ui: &mut egui::Ui,
        cols: FileTableCols,
        row_h: f32,
        mut paint_col: impl FnMut(&mut egui::Ui, usize),
    ) {
        ui.set_width(cols.total);
        ui.set_min_width(cols.total);
        ui.set_max_width(cols.total);
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_width(cols.total);
            ui.set_min_width(cols.total);
            for col in 0..4 {
                Self::table_cell(ui, cols, col, row_h, |cell| paint_col(cell, col));
            }
        });
    }

    fn paint_file_table_header(
        ui: &mut egui::Ui,
        theme: &Theme,
        ctx: &egui::Context,
        cols: FileTableCols,
        sort: &mut FileSortState,
    ) -> bool {
        let mut clicked_col: Option<FileSortColumn> = None;
        let cap_default = theme.color_table_header_inactive();
        let cap_font = egui::FontId::proportional(theme.font_size_file_list_meta());
        let h = theme.size_file_list_row_h();
        ui.allocate_ui_with_layout(
            egui::vec2(cols.total, h),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                Self::paint_file_table_row_strip(ui, cols, h, |cell, col| {
                    let (base_label, col_enum) = match col {
                        0 => return,
                        1 => (crate::i18n::tr(ctx, "Name", "名称"), FileSortColumn::Name),
                        2 => (crate::i18n::tr(ctx, "Size", "大小"), FileSortColumn::Size),
                        _ => (
                            crate::i18n::tr(ctx, "Modified", "修改时间"),
                            FileSortColumn::Time,
                        ),
                    };
                    let text = format!("{}{}", base_label, sort_header_suffix(*sort, col_enum));
                    let color = if sort.column == col_enum {
                        if theme.uses_modern_palette() {
                            theme.text_primary()
                        } else {
                            theme.accent_color()
                        }
                    } else {
                        cap_default
                    };
                    let resp = cell.add(
                        egui::Label::new(RichText::new(text).font(cap_font.clone()).color(color))
                            .truncate(col >= 2)
                            .sense(Sense::click()),
                    );
                    if resp.clicked() {
                        clicked_col = Some(col_enum);
                    }
                });
            },
        );
        if !theme.uses_modern_palette() {
            ui.separator();
        }
        if let Some(c) = clicked_col {
            sort.toggle_column(c);
            true
        } else {
            false
        }
    }

    fn paint_file_table_row(
        ui: &mut egui::Ui,
        theme: &Theme,
        cols: FileTableCols,
        name: &str,
        size_label: &str,
        time_label: &str,
        file_kind: SftpFileKind,
        selected: bool,
        tooltip: &str,
    ) -> egui::Response {
        let h = theme.size_file_list_row_h();
        let (row_rect, response) =
            ui.allocate_exact_size(egui::vec2(cols.total, h), Sense::click());
        let rounding = theme.radius_list_item();
        if selected {
            ui.painter()
                .rect_filled(row_rect, rounding, theme.list_row_selected_bg());
        } else if response.hovered() {
            ui.painter()
                .rect_filled(row_rect, rounding, theme.color_file_list_row_hover_bg());
        }
        let icon = file_kind_icon(file_kind);
        let icon_px = theme.size_file_list_icon();
        let name_color = file_kind_name_color(theme, file_kind, selected);
        let icon_color = file_kind_icon_color(theme, file_kind, selected);
        let meta_color = file_kind_meta_color(theme, file_kind, selected);
        let body_px = theme.font_size_file_list_name();
        let small_px = theme.font_size_file_list_meta();

        ui.allocate_ui_at_rect(row_rect, |ui| {
            Self::paint_file_table_row_strip(ui, cols, h, |cell, col| match col {
                0 => {
                    let (icon_r, _) =
                        cell.allocate_exact_size(egui::vec2(cols.icon, h), Sense::hover());
                    crate::ui::icons::paint_icon(cell, icon_r, icon, icon_color, icon_px);
                }
                1 => {
                    cell.add(
                        egui::Label::new(
                            RichText::new(name)
                                .font(egui::FontId::proportional(body_px))
                                .color(name_color),
                        )
                        .truncate(true),
                    );
                }
                2 => {
                    cell.add(
                        egui::Label::new(
                            RichText::new(size_label)
                                .font(egui::FontId::proportional(small_px))
                                .color(meta_color),
                        )
                        .truncate(true),
                    );
                }
                _ => {
                    cell.add(
                        egui::Label::new(
                            RichText::new(time_label)
                                .font(egui::FontId::proportional(small_px))
                                .color(meta_color),
                        )
                        .truncate(true),
                    );
                }
            });
        });
        response.on_hover_text(tooltip)
    }

    fn localize_list_error(ctx: &egui::Context, msg: &str) -> String {
        if crate::ssh::sftp::is_sftp_would_block_message(msg) {
            return crate::i18n::tr(
                ctx,
                "SFTP channel busy (shell is using the connection). Wait a moment and tap Refresh.",
                "SFTP 通道繁忙（终端正在占用连接），请稍候再点「刷新」重试。",
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
            self.toast_err = Some(e);
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

    /// 右侧 SFTP 侧栏入口（`close_panel` 置为 true 时由宿主隐藏侧栏）
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

    /// Central 之后绘制 SFTP 前景正文（与 AI/监控一致，避免列壳层风格不一致）。
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
                crate::i18n::tr(ctx, "Hide sidebar · or use bottom SFTP toggle", "隐藏侧栏 · 也可用底部 SFTP 切换"),
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

        if let Some(ok) = self.toast_ok.clone() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&ok).color(theme.green_color()));
                if crate::ui::chrome::chrome_small_icon_button(ui, theme, crate::ui::icons::IconId::Close)
                    .on_hover_text(crate::i18n::tr(ui.ctx(), "Dismiss", "关闭提示"))
                    .clicked()
                {
                    self.toast_ok = None;
                }
            });
        }
        if let Some(err) = self.toast_err.clone() {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&err).color(theme.red_color()));
                if crate::ui::chrome::chrome_small_icon_button(ui, theme, crate::ui::icons::IconId::Close)
                    .on_hover_text(crate::i18n::tr(ui.ctx(), "Dismiss", "关闭"))
                    .clicked()
                {
                    self.toast_err = None;
                }
            });
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

        let upload_job = self.local_selected.as_ref().and_then(|p| {
            self.local_entries
                .iter()
                .find(|e| &e.path == p && !e.is_dir)
                .map(|e| (self.cwd.join(&e.name), e.path.clone()))
        });
        let download_job = self.remote_selected.as_ref().and_then(|p| {
            self.entries
                .iter()
                .find(|e| &e.path == p && !e.is_dir)
                .map(|e| (e.path.clone(), self.local_cwd.join(&e.name)))
        });
        let can_upload = !self.busy && upload_job.is_some();
        let can_download = !self.busy && download_job.is_some();
        let can_delete_remote = !self.busy && self.remote_selected.is_some();
        let upload_ready = upload_job.clone();
        let download_ready = download_job.clone();

        let upload_lbl = crate::i18n::tr(ctx, "Upload", "上传").to_string();
        let download_lbl = crate::i18n::tr(ctx, "Download", "下载").to_string();
        let delete_lbl = crate::i18n::tr(ctx, "Delete remote", "删除远端").to_string();
        let upload_tip = crate::i18n::tr(
            ctx,
            "Upload selected local file to remote folder",
            "将选中的本机文件上传到远端当前目录",
        );
        let download_tip = crate::i18n::tr(
            ctx,
            "Download selected remote file to local folder",
            "将选中的远端文件下载到本机当前目录",
        );
        let xfer_group = [
            crate::ui::chrome::ButtonGroupAction {
                icon: crate::ui::icons::IconId::Upload,
                label: &upload_lbl,
                enabled: can_upload,
                tooltip: upload_tip,
            },
            crate::ui::chrome::ButtonGroupAction {
                icon: crate::ui::icons::IconId::Package,
                label: &download_lbl,
                enabled: can_download,
                tooltip: download_tip,
            },
            crate::ui::chrome::ButtonGroupAction {
                icon: crate::ui::icons::IconId::Trash,
                label: &delete_lbl,
                enabled: can_delete_remote,
                tooltip: crate::i18n::tr(ctx, "Delete remote", "删除远端"),
            },
        ];
        if theme.uses_modern_palette() {
            layout_util::set_width_to_available(ui);
            if let Some(idx) = crate::ui::chrome::sftp_toolbar_band(ui, theme, |ui, theme| {
                crate::ui::chrome::sftp_toolbar_actions(ui, theme, &xfer_group, "sftp_xfer")
            }) {
                match idx {
                    0 => {
                        if let Some((remote, local)) = upload_ready {
                            self.spawn_upload(&handle, remote, local, ctx);
                        }
                    }
                    1 => {
                        if let Some((remote, local)) = download_ready {
                            self.spawn_download(&handle, remote, local, ctx);
                        }
                    }
                    2 => {
                        if let Some(p) = self.remote_selected.clone() {
                            self.pending_delete = Some(p);
                        }
                    }
                    _ => {}
                }
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                Self::begin_dock_row(ui);
                ui.spacing_mut().item_spacing.x = theme.spacing_panel_gap();
                if crate::ui::chrome::panel_action_primary_button_with_icon_ex(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Upload,
                    &upload_lbl,
                    can_upload,
                )
                .on_hover_text(upload_tip)
                .clicked()
                {
                    if let Some((remote, local)) = upload_ready {
                        self.spawn_upload(&handle, remote, local, ctx);
                    }
                }
                if crate::ui::chrome::panel_action_primary_button_with_icon_ex(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Package,
                    &download_lbl,
                    can_download,
                )
                .on_hover_text(download_tip)
                .clicked()
                {
                    if let Some((remote, local)) = download_ready {
                        self.spawn_download(&handle, remote, local, ctx);
                    }
                }
                if crate::ui::chrome::panel_action_button_with_icon_ex(
                    ui,
                    theme,
                    crate::ui::icons::IconId::Trash,
                    &delete_lbl,
                    can_delete_remote,
                )
                .clicked()
                {
                    if let Some(p) = self.remote_selected.clone() {
                        self.pending_delete = Some(p);
                    }
                }
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
            let browse_lbl = crate::i18n::tr(ui.ctx(), "Browse…", "浏览…");
            let up_ok = self.local_cwd.parent().is_some();
            let local_nav = [
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::Search,
                    label: go_lbl,
                    enabled: true,
                    tooltip: go_lbl,
                },
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::ChevronLeft,
                    label: up_lbl,
                    enabled: up_ok,
                    tooltip: up_lbl,
                },
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::Refresh,
                    label: "",
                    enabled: true,
                    tooltip: refresh_lbl,
                },
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::Folder,
                    label: browse_lbl,
                    enabled: true,
                    tooltip: browse_lbl,
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
                        crate::ui::icons::IconId::Search,
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
                        crate::ui::icons::IconId::ChevronLeft,
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
                        browse_lbl,
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
                if Self::paint_file_table_header(ui, theme, ctx, table_cols, &mut self.local_sort) {
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
                        for e in self.local_entries.iter() {
                            let sel = self.local_selected.as_ref() == Some(&e.path);
                            let size_lbl =
                                if e.is_dir { "—".to_string() } else { e.size_human() };
                            let time_lbl = format_file_mtime(e.modified);
                            let kind = classify_file_kind(&e.name, e.is_dir);
                            let resp = Self::paint_file_table_row(
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
                            if resp.clicked() {
                                self.local_selected = Some(e.path.clone());
                            }
                            if resp.double_clicked() && e.is_dir {
                                enter_local = Some(e.path.clone());
                            }
                        }
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
            let root_lbl = crate::i18n::tr(ui.ctx(), "Root /", "根 /");
            let busy = !self.busy;
            let remote_nav = [
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::Search,
                    label: go_lbl,
                    enabled: busy,
                    tooltip: go_lbl,
                },
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::ChevronLeft,
                    label: up_lbl,
                    enabled: busy,
                    tooltip: up_lbl,
                },
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::Refresh,
                    label: "",
                    enabled: busy,
                    tooltip: refresh_lbl,
                },
                crate::ui::chrome::ButtonGroupAction {
                    icon: crate::ui::icons::IconId::Folder,
                    label: root_lbl,
                    enabled: busy,
                    tooltip: root_lbl,
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
                        3 => self.spawn_list(&handle, PathBuf::from("/"), ctx),
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
                        crate::ui::icons::IconId::Search,
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
                        crate::ui::icons::IconId::ChevronLeft,
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
                        crate::ui::icons::IconId::Folder,
                        root_lbl,
                        busy,
                    )
                    .clicked()
                    {
                        self.spawn_list(&handle, PathBuf::from("/"), ctx);
                    }
                });
            }

            let mkdir_id = egui::Id::new("sftp_mkdir_name");
            let mkdir_hint = crate::i18n::tr(ui.ctx(), "New folder name", "新建目录名");
            let create_lbl = crate::i18n::tr(ui.ctx(), "Create", "创建");
            let can_create = !self.busy && !self.mkdir_name.trim().is_empty();
            if theme.uses_modern_palette() {
                crate::ui::chrome::sftp_toolbar_band(ui, theme, |ui, theme| {
                    let gap = theme.spacing_sm();
                    let create_w = theme.size_control_btn_min_w() + theme.spacing_lg();
                    let field_w = (ui.available_width() - create_w - gap).max(96.0);
                    ui.scope(|ui| {
                        ui.set_max_width(field_w);
                        ui.set_min_width(field_w);
                        crate::ui::chrome::form_singleline_field_sftp_embedded(
                            ui,
                            theme,
                            mkdir_id,
                            &mut self.mkdir_name,
                            mkdir_hint,
                        );
                    });
                    ui.add_space(gap);
                    if crate::ui::chrome::sftp_ghost_submit_button(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Plus,
                        create_lbl,
                        can_create,
                    )
                    .clicked()
                        && can_create
                    {
                        let p = self.cwd.join(self.mkdir_name.trim());
                        self.mkdir_name.clear();
                        self.spawn_mkdir(&handle, p, ctx);
                    }
                });
            } else {
                let mkdir_w = Self::dock_field_width(ui);
                crate::ui::chrome::form_singleline_field(
                    ui,
                    theme,
                    mkdir_id,
                    &mut self.mkdir_name,
                    mkdir_hint,
                    mkdir_w,
                    false,
                );
                ui.horizontal_wrapped(|ui| {
                    Self::begin_dock_row(ui);
                    if crate::ui::chrome::panel_action_button_with_icon_ex(
                        ui,
                        theme,
                        crate::ui::icons::IconId::Plus,
                        create_lbl,
                        can_create,
                    )
                    .clicked()
                        && can_create
                    {
                        let p = self.cwd.join(self.mkdir_name.trim());
                        self.mkdir_name.clear();
                        self.spawn_mkdir(&handle, p, ctx);
                    }
                });
            }
            if let Some(p) = self.pending_delete.clone() {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {}",
                            crate::i18n::tr(ui.ctx(), "Delete?", "删除？"),
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
                if Self::paint_file_table_header(ui, theme, ctx, table_cols, &mut self.remote_sort) {
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
                        for e in self.entries.iter() {
                            let sel = self.remote_selected.as_ref() == Some(&e.path);
                            let size_lbl =
                                if e.is_dir { "—".to_string() } else { e.size_human() };
                            let time_lbl = format_file_mtime(e.modified);
                            let kind = classify_file_kind(&e.name, e.is_dir);
                            let resp = Self::paint_file_table_row(
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
                            if resp.clicked() {
                                self.remote_selected = Some(e.path.clone());
                            }
                            if resp.double_clicked() && e.is_dir {
                                enter_remote = Some(e.path.clone());
                            }
                        }
                    });
            });
            });
            if let Some(d) = enter_remote {
                self.spawn_list(&handle, d, ctx);
            }
        });

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