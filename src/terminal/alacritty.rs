//! 基于 alacritty_terminal 的终端适配层

use egui::{Color32, FontId, TextFormat, text::LayoutJob};
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
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

/// 终端模拟器（由 alacritty_terminal 驱动）
pub struct Terminal {
    term: Term<VoidListener>,
    parser: Processor,
    width: usize,
    height: usize,
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
        Self {
            term: Term::new(Config::default(), &size, VoidListener),
            parser: Processor::default(),
            width,
            height,
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width.clamp(20, 512);
        self.height = height.clamp(5, 256);
        self.term.resize(TermSize::new(self.width, self.height));
    }

    pub fn feed(&mut self, data: &[u8]) {
        self.parser.advance(&mut self.term, data);
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
    /// 返回带颜色信息的布局（保持等宽）。`terminal_bg` 须与 UI [`Theme::bg_terminal_color`] 一致，
    /// 否则整块格子与外框底色色差会像「四周留白」。
    pub fn get_layout_job(
        &self,
        font_size: f32,
        default_fg: Color32,
        terminal_bg: Color32,
    ) -> LayoutJob {
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

