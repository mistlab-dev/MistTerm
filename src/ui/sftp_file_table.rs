use crate::ssh::SftpEntry;
use crate::ui::layout_util;
use crate::ui::theme::Theme;
use chrono::{DateTime, Utc};
use eframe::egui::{self, Color32, RichText, Sense};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 本机目录项(`std::fs::read_dir`)
#[derive(Debug, Clone)]
pub(super) struct LocalEntry {
    pub(super) name: String,
    pub(super) is_dir: bool,
    pub(super) size: u64,
    pub(super) modified: DateTime<Utc>,
    pub(super) path: PathBuf,
}

impl LocalEntry {
    pub(super) fn size_human(&self) -> String {
        format_file_size(self.size)
    }
}

pub(super) fn path_crumbs(path: &Path) -> Vec<(String, PathBuf)> {
    use std::path::Component;
    let mut acc = PathBuf::new();
    let mut out = Vec::new();
    for c in path.components() {
        match c {
            Component::Prefix(p) => {
                acc.push(p.as_os_str());
                out.push((p.as_os_str().to_string_lossy().into_owned(), acc.clone()));
            }
            Component::RootDir => {
                if acc.as_os_str().is_empty() {
                    acc.push(Component::RootDir.as_os_str());
                    out.push(("/".to_string(), acc.clone()));
                } else {
                    acc.push(c.as_os_str());
                }
            }
            Component::Normal(s) => {
                acc.push(s);
                out.push((s.to_string_lossy().into_owned(), acc.clone()));
            }
            Component::CurDir => {}
            Component::ParentDir => {
                acc.push("..");
                out.push(("..".to_string(), acc.clone()));
            }
        }
    }
    if out.is_empty() {
        out.push((path.display().to_string(), path.to_path_buf()));
    }
    out
}

/// 面包屑显示片段 `(文本, 目标路径, 是否当前目录)`：各段之间插入 `/` 分隔符，
/// 但根 `/`（或 Windows 盘符根）本身已带分隔，不再重复插入，避免显示成 `//root`。
pub(super) fn crumb_display_parts(crumbs: &[(String, PathBuf)]) -> Vec<(String, PathBuf, bool)> {
    let n = crumbs.len();
    let mut parts: Vec<(String, PathBuf, bool)> = Vec::new();
    for (i, (label, dest)) in crumbs.iter().enumerate() {
        if i > 0 {
            let prev = &crumbs[i - 1].0;
            if !(prev.ends_with('/') || prev.ends_with('\\')) {
                parts.push(("/".to_string(), dest.clone(), false));
            }
        }
        parts.push((label.clone(), dest.clone(), i + 1 == n));
    }
    parts
}

/// 省略前缀 `…/` 之后不能再紧跟分隔符 `/`（否则显示 `…//9d/...`）：把起点挪过分隔符。
pub(super) fn skip_leading_separator(parts: &[(String, PathBuf, bool)], start: usize) -> usize {
    let mut s = start;
    if s > 0 {
        while s + 1 < parts.len() && is_crumb_separator(&parts[s]) {
            s += 1;
        }
    }
    s
}

/// 分隔符片段（不可点击）；根 `/` 片段的 `dest` 恰为 `/`，与分隔符区分开。
pub(super) fn is_crumb_separator(part: &(String, PathBuf, bool)) -> bool {
    part.0 == "/" && part.1.as_os_str() != "/"
}

pub(super) fn system_time_to_utc(t: SystemTime) -> Option<DateTime<Utc>> {
    let dur = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    DateTime::from_timestamp(dur.as_secs() as i64, dur.subsec_nanos())
}

pub(super) fn format_file_mtime(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M").to_string()
}

/// SFTP 文件列表行类型(用于文件名/图标前景色)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SftpFileKind {
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

pub(super) fn classify_file_kind(name: &str, is_dir: bool) -> SftpFileKind {
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
        SftpFileKind::Archive | SftpFileKind::Config => theme.color_status_warning_text(),
        SftpFileKind::Image => theme.color_status_positive_text(),
        SftpFileKind::Code => theme.accent_color(),
        SftpFileKind::Document | SftpFileKind::Plain => theme.text_secondary(),
        SftpFileKind::Executable => theme.color_status_negative_text(),
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
        SftpFileKind::Archive | SftpFileKind::Config => theme.color_status_warning_text(),
        SftpFileKind::Image => theme.color_status_positive_text(),
        SftpFileKind::Code => theme.accent_color(),
        SftpFileKind::Executable => theme.color_status_negative_text(),
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

fn file_ext_lower(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn flux_name_color(theme: &Theme, name: &str, kind: SftpFileKind) -> Color32 {
    const TEXT: Color32 = Color32::from_rgb(0xf3, 0xf4, 0xf6);
    const TEXT_3: Color32 = Color32::from_rgb(0x71, 0x71, 0x7a);
    if kind == SftpFileKind::Dir {
        return if theme.is_light_theme() {
            theme.text_primary()
        } else {
            TEXT
        };
    }
    if kind == SftpFileKind::Hidden {
        return if theme.is_light_theme() {
            theme.text_tertiary()
        } else {
            TEXT_3
        };
    }
    if theme.is_light_theme() {
        return match file_ext_lower(name).as_str() {
            "py" | "rs" | "js" | "ts" | "jsx" | "tsx" | "go" | "java" | "c" | "cc" | "cpp" | "h"
            | "cs" | "rb" | "php" | "lua" | "vue" | "svelte" => Color32::from_rgb(30, 64, 175),
            "sh" | "bash" | "zsh" | "fish" => Color32::from_rgb(6, 95, 70),
            "json" | "yaml" | "yml" | "toml" => Color32::from_rgb(146, 64, 14),
            "conf" | "cfg" | "ini" | "env" | "xml" | "properties" => Color32::from_rgb(91, 33, 182),
            _ => theme.text_primary(),
        };
    }
    match file_ext_lower(name).as_str() {
        "py" | "rs" | "js" | "ts" | "jsx" | "tsx" | "go" | "java" | "c" | "cc" | "cpp" | "h"
        | "cs" | "rb" | "php" | "lua" | "vue" | "svelte" => Color32::from_rgb(0xbf, 0xdb, 0xfe),
        "sh" | "bash" | "zsh" | "fish" => Color32::from_rgb(0xa7, 0xf3, 0xd0),
        "json" | "yaml" | "yml" | "toml" => Color32::from_rgb(0xfd, 0xe6, 0x8a),
        "conf" | "cfg" | "ini" | "env" | "xml" | "properties" => Color32::from_rgb(0xdd, 0xd6, 0xfe),
        "md" => Color32::from_rgb(0xcb, 0xd5, 0xe1),
        _ => TEXT,
    }
}

/// 设计稿色块：实心底 + 深色字，而不是淡洗的文件名色。
fn flux_badge_fill_ink(name: &str, kind: SftpFileKind) -> (Color32, Color32) {
    if kind == SftpFileKind::Dir {
        return (
            Color32::from_rgb(0xea, 0xb3, 0x08),
            Color32::from_rgb(0x1c, 0x14, 0x03),
        );
    }
    match file_ext_lower(name).as_str() {
        "py" | "rs" | "js" | "ts" | "jsx" | "tsx" | "go" | "java" | "c" | "cc" | "cpp" | "h"
        | "cs" | "rb" | "php" | "lua" | "vue" | "svelte" => (
            Color32::from_rgb(0x60, 0xa5, 0xfa),
            Color32::from_rgb(0x0b, 0x12, 0x20),
        ),
        "sh" | "bash" | "zsh" | "fish" => (
            Color32::from_rgb(0x34, 0xd3, 0x99),
            Color32::from_rgb(0x05, 0x2e, 0x1a),
        ),
        "json" | "yaml" | "yml" | "toml" => (
            Color32::from_rgb(0xfb, 0xbf, 0x24),
            Color32::from_rgb(0x1c, 0x14, 0x03),
        ),
        "conf" | "cfg" | "ini" | "env" | "xml" | "properties" => (
            Color32::from_rgb(0xc4, 0xb5, 0xfd),
            Color32::from_rgb(0x1a, 0x10, 0x30),
        ),
        "md" | "rst" => (
            Color32::from_rgb(0x94, 0xa3, 0xb8),
            Color32::from_rgb(0x0f, 0x17, 0x2a),
        ),
        _ => (
            Color32::from_rgb(0x64, 0x74, 0x8b),
            Color32::from_rgb(0x0b, 0x0c, 0x0f),
        ),
    }
}

fn flux_badge_color(theme: &Theme, name: &str, kind: SftpFileKind) -> Color32 {
    if kind == SftpFileKind::Dir {
        return if theme.is_light_theme() {
            theme.amber_color()
        } else {
            Color32::from_rgb(0xea, 0xb3, 0x08)
        };
    }
    flux_badge_fill_ink(name, kind).0
}

fn paint_ext_badge(ui: &egui::Ui, icon_rect: egui::Rect, name: &str, fill: Color32, ink: Color32) {
    let ext = file_ext_lower(name);
    let label: String = ext.chars().take(2).collect();
    let label = if label.is_empty() {
        "·".to_string()
    } else {
        label
    };
    let side = 16.0;
    let rect = egui::Rect::from_center_size(icon_rect.center(), egui::vec2(side, side));
    ui.painter().rect_filled(rect, 3.0, fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(9.0),
        ink,
    );
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
pub(super) struct FileSortState {
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

pub(super) fn sort_local_entries(entries: &mut [LocalEntry], sort: FileSortState) {
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

pub(super) fn sort_remote_entries(entries: &mut [SftpEntry], sort: FileSortState) {
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

/// 固定列宽表格布局(表头与各行共用，保证列对齐)
#[derive(Clone, Copy)]
pub(super) struct FileTableCols {
    pub(super) total: f32,
    pub(super) icon: f32,
    name: f32,
    size: f32,
    time: f32,
}

impl FileTableCols {
    const ICON_W: f32 = 22.0;
    /// 「108.52 KB」这一档的固定宽度，不随面板变宽。
    const SIZE_W: f32 = 68.0;
    /// 「2026-10-14 16:12」的固定宽度。
    const TIME_W: f32 = 112.0;

    const PAD_X: f32 = 14.0;
    const GAP: f32 = 4.0;

    fn from_panel_width(panel_w: f32) -> Self {
        let panel_w = panel_w.max(1.0);
        let icon = Self::ICON_W;
        let mut size = Self::SIZE_W;
        let mut time = Self::TIME_W;
        // 分屏半栏很窄时先收起时间，再收起大小。这两列宽度不随面板拉伸。
        if panel_w < 200.0 {
            size = 0.0;
            time = 0.0;
        } else if panel_w < 260.0 {
            time = 0.0;
        }

        let gaps = |size: f32, time: f32| -> f32 {
            let mut n = 2.0;
            if size >= 1.0 {
                n += 1.0;
            }
            if time >= 1.0 {
                n += 1.0;
            }
            Self::PAD_X * 2.0 + Self::GAP * (n - 1.0)
        };
        let inner = (panel_w - gaps(size, time)).max(1.0);
        let name = (inner - icon - size - time).max(0.0);
        Self {
            total: panel_w,
            icon,
            name,
            size,
            time,
        }
    }

    pub(super) fn for_width(panel_w: f32) -> Self {
        Self::from_panel_width(panel_w)
    }

    /// 按列表视口当前可用宽度计算列宽(预留竖向滚动条占位，避免「修改时间」等右列被切)。
    pub(super) fn for_list_ui(ui: &mut egui::Ui, body_cap: f32) -> Self {
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

fn table_cell(
    ui: &mut egui::Ui,
    cols: FileTableCols,
    col: usize,
    row_h: f32,
    add: impl FnOnce(&mut egui::Ui),
) {
    let w = cols.col_width(col);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, row_h), egui::Sense::hover());
    let mut cell = ui.child_ui(rect, FileTableCols::col_layout(col));
    cell.set_clip_rect(cell.clip_rect().intersect(rect));
    cell.set_width(w);
    cell.set_min_width(w);
    cell.set_max_width(w);
    add(&mut cell);
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
        ui.add_space(FileTableCols::PAD_X);
        let mut placed = false;
        for col in 0..4 {
            if cols.col_width(col) < 1.0 {
                continue;
            }
            if placed {
                ui.add_space(FileTableCols::GAP);
            }
            placed = true;
            table_cell(ui, cols, col, row_h, |cell| paint_col(cell, col));
        }
    });
}

fn paint_valign_text(
    ui: &mut egui::Ui,
    text: &str,
    font: egui::FontId,
    color: Color32,
    align_right: bool,
) {
    let max_w = ui.max_rect().width().max(0.0);
    let galley = ui.fonts(|f| {
        let full = f.layout_no_wrap(text.to_string(), font.clone(), color);
        if full.size().x <= max_w + 0.5 {
            return full;
        }
        let mut keep = text.chars().count();
        while keep > 0 {
            keep -= 1;
            let shown: String = text.chars().take(keep).collect();
            let candidate = format!("{shown}…");
            let g = f.layout_no_wrap(candidate, font.clone(), color);
            if g.size().x <= max_w {
                return g;
            }
        }
        f.layout_no_wrap("…".to_string(), font, color)
    });
    let y = ui.max_rect().center().y - galley.size().y * 0.5;
    let x = if align_right {
        ui.max_rect().right() - galley.size().x
    } else {
        ui.max_rect().left()
    };
    ui.painter().galley(egui::pos2(x, y), galley);
}

pub(super) fn paint_file_table_header(
    ui: &mut egui::Ui,
    theme: &Theme,
    ctx: &egui::Context,
    cols: FileTableCols,
    sort: &mut FileSortState,
    id_salt: &str,
) -> bool {
    let mut clicked_col: Option<FileSortColumn> = None;
    let cap_default = theme.color_table_header_inactive();
    let cap_font = egui::FontId::proportional(theme.font_size_file_list_meta());
    let h = theme.size_file_list_row_h();
    let (header_rect, _) = ui.allocate_exact_size(egui::vec2(cols.total, h), Sense::hover());
    if theme.uses_modern_palette() {
        ui.painter()
            .rect_filled(header_rect, 0.0, theme.color_sftp_column_header_fill());
        ui.painter().hline(
            header_rect.x_range(),
            header_rect.bottom() - 0.5,
            egui::Stroke::new(1.0, theme.border.to_color32()),
        );
    }
    ui.allocate_ui_at_rect(header_rect, |ui| {
        paint_file_table_row_strip(ui, cols, h, |cell, col| {
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
                paint_valign_text(cell, &text, cap_font.clone(), color, col >= 2);
                let resp = cell.interact(
                    cell.max_rect(),
                    // 本机/远端两张表的表头 cell 在 egui 里会派生出相同 id，须按面板加盐区分。
                    egui::Id::new(("sftp_col_header", id_salt, col)),
                    Sense::click(),
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

#[allow(clippy::too_many_arguments)]
pub(super) fn paint_file_table_row(
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
    let (row_rect, response) = ui.allocate_exact_size(egui::vec2(cols.total, h), Sense::click());
    let modern = theme.uses_modern_palette();
    let rounding = if modern { 0.0 } else { theme.radius_list_item() };
    if selected {
        let selected_bg = if modern {
            Color32::from_rgba_unmultiplied(96, 165, 250, 31)
        } else {
            theme.list_row_selected_bg()
        };
        ui.painter().rect_filled(row_rect, rounding, selected_bg);
        if modern {
            let bar = egui::Rect::from_min_max(
                row_rect.left_top(),
                egui::pos2(row_rect.left() + 2.0, row_rect.bottom()),
            );
            ui.painter()
                .rect_filled(bar, 0.0, Color32::from_rgb(0x60, 0xa5, 0xfa));
        }
    } else if response.hovered() {
        let hover_bg = if modern {
            Color32::from_rgba_unmultiplied(255, 255, 255, 11)
        } else {
            theme.color_file_list_row_hover_bg()
        };
        ui.painter().rect_filled(row_rect, rounding, hover_bg);
    }
    let icon = file_kind_icon(file_kind);
    let icon_px = theme.size_file_list_icon();
    let name_color = if modern {
        flux_name_color(theme, name, file_kind)
    } else {
        file_kind_name_color(theme, file_kind, selected)
    };
    let icon_color = if modern {
        flux_badge_color(theme, name, file_kind)
    } else {
        file_kind_icon_color(theme, file_kind, selected)
    };
    let meta_color = if modern {
        Color32::from_rgb(0x71, 0x71, 0x7a)
    } else {
        file_kind_meta_color(theme, file_kind, selected)
    };
    let body_px = theme.font_size_file_list_name();
    let small_px = theme.font_size_file_list_meta();
    let name_font = if modern {
        crate::platform::ui_font_id(body_px)
    } else {
        egui::FontId::proportional(body_px)
    };
    let meta_font = if modern {
        crate::platform::ui_font_id(small_px)
    } else {
        egui::FontId::proportional(small_px)
    };

    ui.allocate_ui_at_rect(row_rect, |ui| {
        paint_file_table_row_strip(ui, cols, h, |cell, col| match col {
            0 => {
                let (icon_r, _) =
                    cell.allocate_exact_size(egui::vec2(cols.icon, h), Sense::hover());
                if modern && file_kind != SftpFileKind::Dir {
                    let (fill, ink) = flux_badge_fill_ink(name, file_kind);
                    paint_ext_badge(cell, icon_r, name, fill, ink);
                } else {
                    crate::ui::icons::paint_icon(cell, icon_r, icon, icon_color, icon_px);
                }
            }
            1 => paint_valign_text(cell, name, name_font.clone(), name_color, false),
            2 => paint_valign_text(cell, size_label, meta_font.clone(), meta_color, true),
            _ => paint_valign_text(cell, time_label, meta_font.clone(), meta_color, true),
        });
    });
    response.on_hover_text(tooltip)
}

#[cfg(test)]
mod crumb_tests {
    use super::*;

    fn shown(path: &str, start: usize) -> String {
        let parts = crumb_display_parts(&path_crumbs(Path::new(path)));
        let s = skip_leading_separator(&parts, start);
        let body: String = parts.iter().skip(s).map(|p| p.0.as_str()).collect();
        if s > 0 {
            format!("…/{body}")
        } else {
            body
        }
    }

    #[test]
    fn root_child_has_single_slash() {
        assert_eq!(shown("/root", 0), "/root");
        assert_eq!(shown("/", 0), "/");
        assert_eq!(shown("/var/folders/9d/T", 0), "/var/folders/9d/T");
    }

    #[test]
    fn elided_prefix_has_no_double_slash() {
        // 从分隔符处截断时也只显示一个斜杠
        for start in 1..8 {
            let s = shown("/var/folders/9d/T/mistterm_downloads", start);
            assert!(!s.contains("//"), "{start}: {s}");
        }
    }

    #[test]
    fn root_crumb_is_clickable_not_separator() {
        let parts = crumb_display_parts(&path_crumbs(Path::new("/root")));
        assert_eq!(parts.len(), 2);
        assert!(!is_crumb_separator(&parts[0]));
        assert_eq!(parts[0].1, PathBuf::from("/"));
        let parts = crumb_display_parts(&path_crumbs(Path::new("/a/b")));
        assert!(is_crumb_separator(&parts[2]));
    }
}
