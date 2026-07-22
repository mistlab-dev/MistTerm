use crate::ssh::SftpEntry;
use crate::ui::layout_util;
use crate::ui::theme::Theme;
use chrono::{DateTime, Utc};
use eframe::egui::{self, Color32, RichText, Sense};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 本机目录项（`std::fs::read_dir`）
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

pub(super) fn system_time_to_utc(t: SystemTime) -> Option<DateTime<Utc>> {
    let dur = t.duration_since(std::time::UNIX_EPOCH).ok()?;
    DateTime::from_timestamp(dur.as_secs() as i64, dur.subsec_nanos())
}

pub(super) fn format_file_mtime(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M").to_string()
}

/// SFTP 文件列表行类型（用于文件名/图标前景色）
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

/// 固定列宽表格布局（表头与各行共用，保证列对齐）
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
            table_cell(ui, cols, col, row_h, |cell| paint_col(cell, col));
        }
    });
}

pub(super) fn paint_file_table_header(
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
        paint_file_table_row_strip(ui, cols, h, |cell, col| match col {
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
