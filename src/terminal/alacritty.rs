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

/// 当前屏可见区内的 PTY 光标（0-based 行列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewportCursor {
    pub col: usize,
    pub row: usize,
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

    /// PTY 光标在视口中的格网位置；隐藏或越界时返回 `None`。
    pub fn viewport_cursor(&self) -> Option<ViewportCursor> {
        let content = self.term.renderable_content();
        if content.cursor.shape == CursorShape::Hidden {
            return None;
        }
        let vp = point_to_viewport(content.display_offset, content.cursor.point)?;
        let col = content.cursor.point.column.0;
        if vp.line >= self.height || col >= self.width {
            return None;
        }
        Some(ViewportCursor {
            col,
            row: vp.line,
        })
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

    /// 滚到最新输出（`display_offset == 0`）。
    pub fn scroll_to_bottom(&mut self) {
        let offset = self.term.grid().display_offset();
        if offset > 0 {
            self.scroll_display(Scroll::Delta(-(offset as i32)));
        }
    }

    /// 按绝对网格坐标提取选中文本（`start`/`end` 为 inclusive-exclusive 列范围惯例，与 UI 选区一致）。
    pub fn text_in_point_range(&self, start: Point, end: Point) -> String {
        let (start, end) = if (start.line.0, start.column.0) <= (end.line.0, end.column.0) {
            (start, end)
        } else {
            (end, start)
        };
        let mut result = String::new();
        for line_idx in start.line.0..=end.line.0 {
            let row = self.row_chars(Line(line_idx));
            let line_len = row.len();
            let c_start = if line_idx == start.line.0 {
                start.column.0
            } else {
                0
            };
            let c_end = if line_idx == end.line.0 {
                end.column.0
            } else {
                line_len
            };
            let c_start = c_start.min(line_len);
            let c_end = c_end.min(line_len);
            if c_start < c_end {
                result.push_str(&row[c_start..c_end].iter().collect::<String>());
            }
            if line_idx < end.line.0 {
                result.push('\n');
            }
        }
        result
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

        let mut out = String::with_capacity(self.height * (self.width + 1));
        for (i, row) in rows.into_iter().enumerate() {
            for ch in row {
                out.push(ch);
            }
            // 与 get_layout_job 一致：末行后不再加 `\n`，避免 TextEdit/行计数多出空行。
            if i + 1 < self.height {
                out.push('\n');
            }
        }
        out
    }

    /// 从缓冲区底部取最近 `max_lines` 行（含 scrollback）。
    pub fn tail_plain_text(&self, max_lines: usize) -> String {
        if max_lines == 0 {
            return String::new();
        }
        let grid = self.term.grid();
        let bottom = grid.bottommost_line().0;
        let top = grid.topmost_line().0;
        let start = bottom.saturating_sub(max_lines as i32 - 1).max(top);
        let mut lines = Vec::new();
        for line_idx in start..=bottom {
            let row = self.row_chars(Line(line_idx));
            let s: String = row.into_iter().collect();
            lines.push(s.trim_end().to_string());
        }
        lines.join("\n")
    }

    /// 返回着色 `LayoutJob` 与逐格背景色。
    ///
    /// **背景不写入 `TextFormat.background`**：egui 会对 background 做 `expand(1.0)`，
    /// 在终端密排下会压住下一行字形。调用方须在文本之下自绘 `cell_bgs`，再铺 galley。
    /// `selection`: 绝对网格选区 `(start_line, start_col, end_line, end_col)`（列 end 开区间）+ 选区底色。
    pub fn get_layout_job(
        &self,
        font_size: f32,
        line_height: f32,
        cell_w: f32,
        shell: &TerminalShellStyle,
        highlight: Option<(usize, usize, usize)>,
        selection: Option<(i32, usize, i32, usize, Color32)>,
    ) -> (LayoutJob, Vec<Vec<Color32>>) {
        let default_fg = shell.default_fg;
        let terminal_bg = shell.terminal_bg;
        let mut rows =
            vec![vec![(' ', default_fg, terminal_bg, Flags::empty()); self.width]; self.height];
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
                    rows[y][x] = (indexed.cell.c, fg, bg, indexed.cell.flags);
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

        if let Some((sl, sc, el, ec, sel_bg)) = selection {
            let offset = content.display_offset;
            for abs_line in sl..=el {
                let pt = Point::new(Line(abs_line), Column(0));
                let Some(vp) = point_to_viewport(offset, pt) else {
                    continue;
                };
                let y = vp.line;
                if y >= self.height {
                    continue;
                }
                let c0 = if abs_line == sl { sc } else { 0 };
                let c1 = if abs_line == el { ec } else { self.width };
                let c0 = c0.min(self.width);
                let c1 = c1.min(self.width);
                for x in c0..c1 {
                    rows[y][x].2 = sel_bg;
                }
            }
        }

        let cell_bgs: Vec<Vec<Color32>> = rows
            .iter()
            .map(|row| row.iter().map(|(_, _, bg, _)| *bg).collect())
            .collect();

        let wide_spacer = Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER;
        let mut job = LayoutJob::default();
        // 字形层底色一律透明；ANSI/选区底由 UI 自绘，避免 egui background expand 压行。
        let cell_fmt = |color: Color32, extra_spacing: f32| TextFormat {
            font_id: FontId::monospace(font_size),
            color,
            background: Color32::TRANSPARENT,
            line_height: Some(line_height),
            extra_letter_spacing: extra_spacing,
            // 显式 BOTTOM：与 egui 默认一致；单字体时基线按行 ascent，勿改 Center/TOP 折腾 CJK。
            valign: egui::Align::BOTTOM,
            ..Default::default()
        };
        let same_run = |a: &TextFormat, b: &TextFormat| {
            a.color == b.color
                && a.extra_letter_spacing == b.extra_letter_spacing
                && a.line_height == b.line_height
                && a.font_id == b.font_id
        };
        let mut run_text = String::new();
        let mut run_fmt: Option<TextFormat> = None;
        let flush_run = |job: &mut LayoutJob, text: &mut String, fmt: &mut Option<TextFormat>| {
            if let Some(f) = fmt.take() {
                if !text.is_empty() {
                    job.append(text, 0.0, f);
                    text.clear();
                }
            }
        };
        let row_count = rows.len();
        for (row_i, row) in rows.into_iter().enumerate() {
            for (ch, color, _bg, flags) in row {
                if flags.intersects(wide_spacer) {
                    flush_run(&mut job, &mut run_text, &mut run_fmt);
                    job.append("\u{200b}", 0.0, cell_fmt(color, 0.0));
                    continue;
                }
                let spacing = if flags.contains(Flags::WIDE_CHAR) {
                    cell_w.max(0.0)
                } else {
                    0.0
                };
                let fmt = cell_fmt(color, spacing);
                if let Some(ref cur) = run_fmt {
                    if same_run(cur, &fmt) {
                        run_text.push(ch);
                        continue;
                    }
                }
                flush_run(&mut job, &mut run_text, &mut run_fmt);
                run_text.push(ch);
                run_fmt = Some(fmt);
            }
            flush_run(&mut job, &mut run_text, &mut run_fmt);
            // 最后一行后勿再 append '\n'，否则 egui 会多出一行空 galley（底栏上方多一截空白）。
            if row_i + 1 < row_count {
                job.append("\n", 0.0, cell_fmt(default_fg, 0.0));
            }
        }
        flush_run(&mut job, &mut run_text, &mut run_fmt);
        (job, cell_bgs)
    }
}

/// FUNCTIONAL_SPEC §2.3.2：对「整行均为应用默认前景 + 终端背景」的行做轻量 shell 提示启发式着色；
/// 任意单元格已带 ANSI 前景/背景差异时整行跳过，避免覆盖远端配色。
fn apply_heuristic_shell_row_style(
    rows: &mut [Vec<(char, Color32, Color32, Flags)>],
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
        for (_ch, fg, bg, _flags) in row.iter() {
            if *fg != default_fg || *bg != terminal_bg {
                all_default = false;
                break;
            }
        }
        if !all_default {
            continue;
        }

        let chars: Vec<char> = row.iter().map(|(c, _, _, _)| *c).collect();
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

        let scale_line_fg = |cell: &mut (char, Color32, Color32, Flags), factor: f32| {
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
        } else if is_user_error_line(line_trim)
            || is_user_info_line(line_trim)
            || is_user_success_line(line_trim)
            || is_user_warn_line(line_trim)
        {
            // 状态行若未命中上方着色（如 CJK 被拉开空格），勿按输出行压暗
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
    use alacritty_terminal::vte::ansi::{Color, NamedColor};
    match color {
        Color::Spec(rgb) => {
            // truecolor 已是显式色值；勿因 BOLD 再提亮（会发糊且非标准）。
            let c = Color32::from_rgb(rgb.r, rgb.g, rgb.b);
            if dim {
                dim_color(c)
            } else {
                c
            }
        }
        Color::Indexed(mut idx) => {
            // 经典 ANSI：粗体将 0..7 前景提升到亮色 8..15
            if bold && idx < 8 {
                idx += 8;
            }
            let c = indexed_to_color(idx);
            if dim {
                dim_color(c)
            } else {
                c
            }
        }
        Color::Named(mut name) => {
            // 基础 8 色：bold→bright；默认前景：适度提亮（勿到纯白），与略暗的 default_fg 形成区分。
            if bold {
                match name {
                    NamedColor::Black
                    | NamedColor::Red
                    | NamedColor::Green
                    | NamedColor::Yellow
                    | NamedColor::Blue
                    | NamedColor::Magenta
                    | NamedColor::Cyan
                    | NamedColor::White => {
                        name = name.to_bright();
                    }
                    NamedColor::Foreground => {
                        let c = brighten_fg(fallback_fg);
                        return if dim { dim_color(c) } else { c };
                    }
                    _ => {}
                }
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
            7 => Color32::from_rgb(200, 200, 200),
            8 => Color32::from_rgb(127, 127, 127),
            9 => Color32::from_rgb(255, 0, 0),
            10 => Color32::from_rgb(0, 255, 0),
            11 => Color32::from_rgb(255, 255, 0),
            12 => Color32::from_rgb(92, 92, 255),
            13 => Color32::from_rgb(255, 0, 255),
            14 => Color32::from_rgb(0, 255, 255),
            // BrightWhite：与暗色 default_fg(~#A8) 拉开，但低于纯白以免发糊
            _ => Color32::from_rgb(248, 248, 248),
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
        // 勿用纯白：相对默认前景再提亮一档即可（default 已是软白）。
        NamedColor::BrightForeground => brighten_fg(fallback_fg),
        NamedColor::DimForeground => dim_color(fallback_fg),
    }
}

/// 粗体 / bright 提亮：向白靠拢约 55%，与略暗的 default_fg 拉开对比。
fn brighten_fg(color: Color32) -> Color32 {
    let lift = |c: u8| -> u8 {
        c.saturating_add(((255u16.saturating_sub(c as u16)) * 55 / 100) as u8)
    };
    Color32::from_rgb(lift(color.r()), lift(color.g()), lift(color.b()))
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

    #[test]
    fn scroll_to_bottom_clears_display_offset() {
        use alacritty_terminal::grid::Scroll;
        let mut t = Terminal::new(20, 5);
        for i in 0..15 {
            t.feed(format!("row-{i:02}\r\n").as_bytes());
        }
        t.scroll_display(Scroll::Delta(4));
        assert!(t.display_offset() > 0);
        assert!(!t.is_scrolled_to_bottom());
        t.scroll_to_bottom();
        assert_eq!(t.display_offset(), 0);
        assert!(t.is_scrolled_to_bottom());
    }

    #[test]
    fn text_in_point_range_reads_from_absolute_grid() {
        use alacritty_terminal::index::{Column, Point};
        let mut t = Terminal::new(40, 3);
        t.feed(b"alpha needle omega\r\n");
        let hits = t.search_all("needle", false);
        let hit = hits.first().expect("hit");
        let start = Point::new(hit.line, Column(hit.column));
        let end = Point::new(hit.line, Column(hit.column + 6));
        assert_eq!(t.text_in_point_range(start, end), "needle");
        // 反向端点仍应得到相同文本
        assert_eq!(t.text_in_point_range(end, start), "needle");
    }
}
