//! 基于 alacritty_terminal 的终端适配层

use crate::terminal::style::{
    TerminalShellStyle, is_user_error_line, is_user_info_line, is_user_success_line,
    is_user_warn_line,
};
use egui::{Color32, FontId, TextFormat, text::LayoutJob};
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::{point_to_viewport, Config, Term};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{CursorShape, Processor};

#[derive(Clone, Copy)]
struct TermSize {
    columns: usize,
    screen_lines: usize,
}

impl TermSize {
    fn new(columns: usize, screen_lines: usize) -> Self {
        Self { columns, screen_lines }
    }
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.screen_lines
    }

    fn screen_lines(&self) -> usize {
        self.screen_lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// 缓冲区搜索命中（含 scrollback）；`column` 为 **0-based** 网格列。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub line: Line,
    pub column: usize,
}

/// 终端模拟器（由 alacritty_terminal 驱动）
pub struct Terminal {
    term: Term<VoidListener>,
    parser: Processor,
    width: usize,
    height: usize,
    /// PTY 有字节写入 VTE 时递增；用于 UI 跳过未变更帧的整屏 `LayoutJob` 重建（FUNCTIONAL_SPEC §2.3.1）。
    content_epoch: u64,
}

impl Default for Terminal {
    fn default() -> Self {
        Self::new(80, 24)
    }
}

impl Terminal {
    pub fn new(width: usize, height: usize) -> Self {
        let width = width.clamp(20, 512);
        let height = height.clamp(5, 256);
        let size = TermSize::new(width, height);
        // FUNCTIONAL_SPEC §2.4：`alacritty_terminal` 默认 `scrolling_history` 已为 10000，与「保留最后 10000 行」一致。
        Self {
            term: Term::new(Config::default(), &size, VoidListener),
            parser: Processor::default(),
            width,
            height,
            content_epoch: 0,
        }
    }

    #[inline]
    pub fn content_epoch(&self) -> u64 {
        self.content_epoch
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        let nw = width.clamp(20, 512);
        let nh = height.clamp(5, 256);
        if nw != self.width || nh != self.height {
            self.content_epoch = self.content_epoch.wrapping_add(1);
            self.width = nw;
            self.height = nh;
            self.term.resize(TermSize::new(self.width, self.height));
        }
    }

    pub fn feed(&mut self, data: &[u8]) {
        if !data.is_empty() {
            self.content_epoch = self.content_epoch.wrapping_add(1);
        }
        self.parser.advance(&mut self.term, data);
    }

    /// 清空滚动历史缓冲区，保留当前屏幕内容
    pub fn clear_history(&mut self) {
        self.content_epoch = self.content_epoch.wrapping_add(1);
        self.term.grid_mut().clear_history();
    }

    /// 滚动视口查看 scrollback（`Scroll::Delta` 为正时向上翻历史）。
    pub fn scroll_display(&mut self, scroll: Scroll) {
        let before = self.term.grid().display_offset();
        self.term.scroll_display(scroll);
        if self.term.grid().display_offset() != before {
            self.content_epoch = self.content_epoch.wrapping_add(1);
        }
    }

    /// 是否在最新输出（未向上滚动）。
    pub fn is_scrolled_to_bottom(&self) -> bool {
        self.term.grid().display_offset() == 0
    }

    #[inline]
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    fn row_chars(&self, line: Line) -> Vec<char> {
        let grid = self.term.grid();
        let cols = grid.columns();
        (0..cols)
            .map(|c| grid[line][Column(c)].c)
            .collect()
    }

    fn chars_match(a: &[char], b: &[char], ignore_case: bool) -> bool {
        if a.len() != b.len() {
            return false;
        }
        if ignore_case {
            a.iter()
                .zip(b.iter())
                .all(|(x, y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
        } else {
            a == b
        }
    }

    /// 在完整网格（含 scrollback）中搜索子串。
    pub fn search_all(&self, query: &str, ignore_case: bool) -> Vec<SearchHit> {
        if query.is_empty() {
            return Vec::new();
        }
        let q: Vec<char> = if ignore_case {
            query.to_ascii_lowercase().chars().collect()
        } else {
            query.chars().collect()
        };
        let q_len = q.len();
        if q_len == 0 {
            return Vec::new();
        }
        let grid = self.term.grid();
        let cols = grid.columns();
        if cols < q_len {
            return Vec::new();
        }
        let mut hits = Vec::new();
        let top = grid.topmost_line().0;
        let bottom = grid.bottommost_line().0;
        for line_idx in top..=bottom {
            let line = Line(line_idx);
            let row = self.row_chars(line);
            for start_col in 0..=cols - q_len {
                let window: Vec<char> = row[start_col..start_col + q_len].to_vec();
                if Self::chars_match(&window, &q, ignore_case) {
                    hits.push(SearchHit {
                        line,
                        column: start_col,
                    });
                }
            }
        }
        hits
    }

    /// 滚动视口使 `line` 出现在屏内，并返回用于高亮的 **(视口行, 列)**（均为 1-based）。
    pub fn reveal_search_hit(&mut self, hit: SearchHit) -> Option<(usize, usize)> {
        self.scroll_line_into_view(hit.line);
        let offset = self.term.grid().display_offset();
        let pt = Point::new(hit.line, Column(hit.column));
        point_to_viewport(offset, pt).map(|vp| (vp.line + 1, hit.column + 1))
    }

    fn scroll_line_into_view(&mut self, line: Line) {
        let grid = self.term.grid();
        let target_offset = (0i32.saturating_sub(line.0)).max(0) as usize;
        let target_offset = target_offset.min(grid.history_size());
        let current = grid.display_offset();
        if target_offset > current {
            self.scroll_display(Scroll::Delta((target_offset - current) as i32));
        } else if current > target_offset {
            self.scroll_display(Scroll::Delta(-((current - target_offset) as i32)));
        }
    }

    /// 返回当前视口（screen）可见文本，保持固定列宽，避免表格错位
    pub fn get_formatted_output(&self) -> String {
        let mut rows = vec![vec![' '; self.width]; self.height];
        let content = self.term.renderable_content();

        for indexed in content.display_iter {
            if let Some(vp) = point_to_viewport(content.display_offset, indexed.point) {
                if vp.line < self.height && indexed.point.column.0 < self.width {
                    rows[vp.line][indexed.point.column.0] = indexed.cell.c;
                }
            }
        }

        // 光标覆盖而非插入，避免把后续列右移
        if content.cursor.shape != CursorShape::Hidden {
            if let Some(vp) = point_to_viewport(content.display_offset, content.cursor.point) {
                if vp.line < self.height && content.cursor.point.column.0 < self.width {
                    rows[vp.line][content.cursor.point.column.0] = '│';
                }
            }
        }

        let mut out = String::with_capacity(self.height * (self.width + 1));
        for row in rows {
            for ch in row {
                out.push(ch);
            }
            out.push('\n');
        }
        out
    }
    /// 返回带颜色信息的布局（保持等宽）。`shell` 须由 [`TerminalShellStyle::from_theme`] 生成，
    /// 且 `terminal_bg` 与 UI 外框一致，否则整块格子与外框底色色差会像「四周留白」。
    /// `highlight`: 当前命中 `(行, 列, 长度)`，均为 **1-based** 字符下标（与 [`Self::search_viewport`] 一致）。
    pub fn get_layout_job(
        &self,
        font_size: f32,
        shell: &TerminalShellStyle,
        highlight: Option<(usize, usize, usize)>,
    ) -> LayoutJob {
        let default_fg = shell.default_fg;
        let terminal_bg = shell.terminal_bg;
        let mut rows = vec![vec![(' ', default_fg, terminal_bg); self.width]; self.height];
        let content = self.term.renderable_content();

        for indexed in content.display_iter {
            if let Some(vp) = point_to_viewport(content.display_offset, indexed.point) {
                let x = indexed.point.column.0;
                let y = vp.line;
                if y < self.height && x < self.width {
                    let bold = indexed.cell.flags.contains(Flags::BOLD);
                    let dim = indexed.cell.flags.contains(Flags::DIM);
                    let mut fg = map_cell_color(indexed.cell.fg, default_fg, terminal_bg, bold, dim);
                    let mut bg =
                        map_cell_color(indexed.cell.bg, terminal_bg, terminal_bg, false, false);
                    if indexed.cell.flags.contains(Flags::INVERSE) {
                        std::mem::swap(&mut fg, &mut bg);
                    }
                    rows[y][x] = (indexed.cell.c, fg, bg);
                }
            }
        }

        if content.cursor.shape != CursorShape::Hidden {
            if let Some(vp) = point_to_viewport(content.display_offset, content.cursor.point) {
                let x = content.cursor.point.column.0;
                let y = vp.line;
                if y < self.height && x < self.width {
                    rows[y][x] = ('│', default_fg, terminal_bg);
                }
            }
        }

        apply_heuristic_shell_row_style(&mut rows, shell, self.width);

        if let Some((hl_line, hl_col, hl_len)) = highlight {
            let y = hl_line.saturating_sub(1);
            if y < self.height && hl_len > 0 {
                let x0 = hl_col.saturating_sub(1);
                for i in 0..hl_len {
                    let x = x0 + i;
                    if x < self.width {
                        rows[y][x].1 = shell.search_match_fg;
                        rows[y][x].2 = shell.search_match_bg;
                    }
                }
            }
        }

        let mut job = LayoutJob::default();
        for row in rows {
            for (ch, color, bg) in row {
                let mut buf = [0u8; 4];
                let s = ch.encode_utf8(&mut buf);
                job.append(
                    s,
                    0.0,
                    TextFormat {
                        font_id: FontId::monospace(font_size),
                        color,
                        background: bg,
                        ..Default::default()
                    },
                );
            }
            job.append(
                "\n",
                0.0,
                TextFormat {
                    font_id: FontId::monospace(font_size),
                    color: default_fg,
                    background: terminal_bg,
                    ..Default::default()
                },
            );
        }
        job
    }
}

/// FUNCTIONAL_SPEC §2.3.2：对「整行均为应用默认前景 + 终端背景」的行做轻量 shell 提示启发式着色；
/// 任意单元格已带 ANSI 前景/背景差异时整行跳过，避免覆盖远端配色。
fn apply_heuristic_shell_row_style(
    rows: &mut [Vec<(char, Color32, Color32)>],
    shell: &TerminalShellStyle,
    width: usize,
) {
    if width == 0 {
        return;
    }
    let default_fg = shell.default_fg;
    let terminal_bg = shell.terminal_bg;

    for row in rows.iter_mut() {
        let mut all_default = true;
        for (_ch, fg, bg) in row.iter() {
            if *fg != default_fg || *bg != terminal_bg {
                all_default = false;
                break;
            }
        }
        if !all_default {
            continue;
        }

        let chars: Vec<char> = row.iter().map(|(c, _, _)| *c).collect();
        if chars.iter().all(|c| c.is_whitespace()) {
            continue;
        }

        let line: String = chars.iter().collect();
        let line_trim = line.trim_end();
        if line_trim.is_empty() {
            continue;
        }

        if line_trim.contains("://") {
            continue;
        }

        if is_user_error_line(line_trim) {
            for cell in row.iter_mut() {
                if !cell.0.is_whitespace() {
                    cell.1 = shell.user_error;
                }
            }
            continue;
        }
        if is_user_success_line(line_trim) {
            for cell in row.iter_mut() {
                if !cell.0.is_whitespace() {
                    cell.1 = shell.user_success;
                }
            }
            continue;
        }
        if is_user_warn_line(line_trim) {
            for cell in row.iter_mut() {
                if !cell.0.is_whitespace() {
                    cell.1 = shell.user_warn;
                }
            }
            continue;
        }
        if is_user_info_line(line_trim) {
            for cell in row.iter_mut() {
                if !cell.0.is_whitespace() {
                    cell.1 = shell.user_info;
                }
            }
            continue;
        }

        let looks_prompt = line_trim.contains('➜')
            || (line_trim.contains('@')
                && line_trim.contains(':')
                && (line_trim.contains('~') || line_trim.contains('/'))
                && line_trim
                    .find('@')
                    .map(|i| i > 0 && line_trim.chars().nth(i - 1).is_some_and(|c| {
                        c.is_alphanumeric() || c == ']' || c == '_'
                    }))
                    .unwrap_or(false));

        let last_non_ws = chars
            .iter()
            .enumerate()
            .rev()
            .find(|(_, c)| !c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(0);

        let scale_line_fg = |cell: &mut (char, Color32, Color32), factor: f32| {
            if cell.1 == default_fg {
                cell.1 = Color32::from_rgb(
                    ((default_fg.r() as f32) * factor).min(255.0) as u8,
                    ((default_fg.g() as f32) * factor).min(255.0) as u8,
                    ((default_fg.b() as f32) * factor).min(255.0) as u8,
                );
            }
        };

        if looks_prompt {
            let mut path_end_col: Option<usize> = None;
            for cell in row.iter_mut() {
                if cell.0 == '➜' {
                    cell.1 = shell.prompt_arrow;
                }
            }
            if let Some(at) = chars.iter().position(|&c| c == '@') {
                if let Some(colon_pos) = chars
                    .iter()
                    .enumerate()
                    .skip(at.saturating_add(1))
                    .find(|(_, &c)| c == ':')
                    .map(|(i, _)| i)
                {
                    let mut x = colon_pos + 1;
                    while x < width && chars.get(x) == Some(&' ') {
                        x += 1;
                    }
                    if x < width {
                        let first = chars[x];
                        if first == '~' || first == '/' {
                            while x < width {
                                let c = row[x].0;
                                if c.is_whitespace() {
                                    break;
                                }
                                if matches!(c, '$' | '%' | '#' | '`') {
                                    break;
                                }
                                row[x].1 = shell.path_hint;
                                x += 1;
                            }
                            path_end_col = Some(x);
                        }
                    }
                }
            }

            if let Some(pe) = path_end_col {
                let mut i = pe;
                while i < width && row[i].0.is_whitespace() {
                    i += 1;
                }
                for k in i..=last_non_ws {
                    scale_line_fg(&mut row[k], shell.command_dim_factor);
                }
            } else if line_trim.contains('➜') {
                if let Some(i) = chars.iter().position(|&c| c == '➜') {
                    let mut j = i.saturating_add(1);
                    while j < width && chars[j].is_whitespace() {
                        j += 1;
                    }
                    for k in j..=last_non_ws {
                        scale_line_fg(&mut row[k], shell.command_dim_factor);
                    }
                }
            }
        } else {
            for cell in row.iter_mut() {
                if !cell.0.is_whitespace() {
                    scale_line_fg(cell, shell.output_dim_factor);
                }
            }
        }
    }
}

fn map_cell_color(
    color: alacritty_terminal::vte::ansi::Color,
    fallback_fg: Color32,
    fallback_bg: Color32,
    bold: bool,
    dim: bool,
) -> Color32 {
    use alacritty_terminal::vte::ansi::Color;
    match color {
        Color::Spec(rgb) => {
            let c = Color32::from_rgb(rgb.r, rgb.g, rgb.b);
            if dim { dim_color(c) } else { c }
        }
        Color::Indexed(mut idx) => {
            // 兼容经典终端行为：粗体将 0..7 前景提升到亮色 8..15
            if bold && idx < 8 {
                idx += 8;
            }
            let c = indexed_to_color(idx);
            if dim { dim_color(c) } else { c }
        }
        Color::Named(mut name) => {
            if bold {
                name = name.to_bright();
            }
            if dim {
                name = name.to_dim();
            }
            named_to_color(name, fallback_fg, fallback_bg)
        }
    }
}

fn indexed_to_color(idx: u8) -> Color32 {
    if idx < 16 {
        return match idx {
            // xterm 标准 16 色
            0 => Color32::from_rgb(0, 0, 0),
            1 => Color32::from_rgb(205, 0, 0),
            2 => Color32::from_rgb(0, 205, 0),
            3 => Color32::from_rgb(205, 205, 0),
            4 => Color32::from_rgb(0, 0, 238),
            5 => Color32::from_rgb(205, 0, 205),
            6 => Color32::from_rgb(0, 205, 205),
            7 => Color32::from_rgb(229, 229, 229),
            8 => Color32::from_rgb(127, 127, 127),
            9 => Color32::from_rgb(255, 0, 0),
            10 => Color32::from_rgb(0, 255, 0),
            11 => Color32::from_rgb(255, 255, 0),
            12 => Color32::from_rgb(92, 92, 255),
            13 => Color32::from_rgb(255, 0, 255),
            14 => Color32::from_rgb(0, 255, 255),
            _ => Color32::from_rgb(255, 255, 255),
        };
    }
    if idx < 232 {
        let i = idx as usize - 16;
        let r = i / 36;
        let g = (i / 6) % 6;
        let b = i % 6;
        let map = |n: usize| if n == 0 { 0 } else { 55 + n as u8 * 40 };
        return Color32::from_rgb(map(r), map(g), map(b));
    }
    let gray = 8 + (idx - 232) * 10;
    Color32::from_rgb(gray, gray, gray)
}

fn named_to_color(
    name: alacritty_terminal::vte::ansi::NamedColor,
    fallback_fg: Color32,
    fallback_bg: Color32,
) -> Color32 {
    use alacritty_terminal::vte::ansi::NamedColor;
    match name {
        NamedColor::Black => indexed_to_color(0),
        NamedColor::Red => indexed_to_color(1),
        NamedColor::Green => indexed_to_color(2),
        NamedColor::Yellow => indexed_to_color(3),
        NamedColor::Blue => indexed_to_color(4),
        NamedColor::Magenta => indexed_to_color(5),
        NamedColor::Cyan => indexed_to_color(6),
        NamedColor::White => indexed_to_color(7),
        NamedColor::BrightBlack => indexed_to_color(8),
        NamedColor::BrightRed => indexed_to_color(9),
        NamedColor::BrightGreen => indexed_to_color(10),
        NamedColor::BrightYellow => indexed_to_color(11),
        NamedColor::BrightBlue => indexed_to_color(12),
        NamedColor::BrightMagenta => indexed_to_color(13),
        NamedColor::BrightCyan => indexed_to_color(14),
        NamedColor::BrightWhite => indexed_to_color(15),
        NamedColor::Foreground => fallback_fg,
        NamedColor::Background => fallback_bg,
        NamedColor::Cursor => fallback_fg,
        NamedColor::DimBlack => dim_color(indexed_to_color(0)),
        NamedColor::DimRed => dim_color(indexed_to_color(1)),
        NamedColor::DimGreen => dim_color(indexed_to_color(2)),
        NamedColor::DimYellow => dim_color(indexed_to_color(3)),
        NamedColor::DimBlue => dim_color(indexed_to_color(4)),
        NamedColor::DimMagenta => dim_color(indexed_to_color(5)),
        NamedColor::DimCyan => dim_color(indexed_to_color(6)),
        NamedColor::DimWhite => dim_color(indexed_to_color(7)),
        NamedColor::BrightForeground => indexed_to_color(15),
        NamedColor::DimForeground => dim_color(fallback_fg),
    }
}

fn dim_color(color: Color32) -> Color32 {
    let scale = |c: u8| -> u8 { ((c as u16 * 2) / 3) as u8 };
    Color32::from_rgb(scale(color.r()), scale(color.g()), scale(color.b()))
}

#[cfg(test)]
mod tests {
    use super::Terminal;

    #[test]
    fn content_epoch_increments_on_nonempty_feed_only() {
        let mut t = Terminal::new(20, 5);
        let e0 = t.content_epoch();
        t.feed(b"a");
        assert_eq!(t.content_epoch(), e0.wrapping_add(1));
        t.feed(&[]);
        assert_eq!(t.content_epoch(), e0.wrapping_add(1));
    }

    #[test]
    fn content_epoch_changes_on_resize_when_dimensions_change() {
        let mut t = Terminal::new(20, 5);
        let e0 = t.content_epoch();
        t.resize(20, 5);
        assert_eq!(t.content_epoch(), e0);
        t.resize(21, 5);
        assert_eq!(t.content_epoch(), e0.wrapping_add(1));
    }

    #[test]
    fn search_all_finds_substring_at_grid_column() {
        let mut t = Terminal::new(40, 3);
        t.feed(b"    3655.1 total\n");
        let hits = t.search_all("55", false);
        assert!(!hits.is_empty());
        let line = t.get_formatted_output();
        let first = line.lines().next().expect("line");
        let chars: Vec<char> = first.chars().collect();
        let window: String = chars[hits[0].column..hits[0].column + 2]
            .iter()
            .collect();
        assert_eq!(window, "55");
    }
}
