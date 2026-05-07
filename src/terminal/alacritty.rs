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
    /// 返回带颜色信息的布局（保持等宽）
    pub fn get_layout_job(&self, font_size: f32, default_fg: Color32) -> LayoutJob {
        let mut rows = vec![vec![(' ', default_fg); self.width]; self.height];
        let content = self.term.renderable_content();

        for indexed in content.display_iter {
            if let Some(vp) = point_to_viewport(content.display_offset, indexed.point) {
                let x = indexed.point.column.0;
                let y = vp.line;
                if y < self.height && x < self.width {
                    let mut fg = map_ansi_color(indexed.cell.fg, default_fg);
                    if indexed.cell.flags.contains(Flags::INVERSE) {
                        fg = map_ansi_color(indexed.cell.bg, default_fg);
                    }
                    rows[y][x] = (indexed.cell.c, fg);
                }
            }
        }

        if content.cursor.shape != CursorShape::Hidden {
            if let Some(vp) = point_to_viewport(content.display_offset, content.cursor.point) {
                let x = content.cursor.point.column.0;
                let y = vp.line;
                if y < self.height && x < self.width {
                    rows[y][x] = ('│', default_fg);
                }
            }
        }

        let mut job = LayoutJob::default();
        for row in rows {
            for (ch, color) in row {
                let mut buf = [0u8; 4];
                let s = ch.encode_utf8(&mut buf);
                job.append(
                    s,
                    0.0,
                    TextFormat {
                        font_id: FontId::monospace(font_size),
                        color,
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
                    ..Default::default()
                },
            );
        }
        job
    }
}

fn map_ansi_color(color: alacritty_terminal::vte::ansi::Color, fallback: Color32) -> Color32 {
    use alacritty_terminal::vte::ansi::{Color, NamedColor};
    match color {
        Color::Spec(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
        Color::Indexed(idx) => indexed_to_color(idx),
        Color::Named(name) => match name {
            NamedColor::Black => Color32::from_rgb(0, 0, 0),
            NamedColor::Red => Color32::from_rgb(205, 49, 49),
            NamedColor::Green => Color32::from_rgb(13, 188, 121),
            NamedColor::Yellow => Color32::from_rgb(229, 229, 16),
            NamedColor::Blue => Color32::from_rgb(36, 114, 200),
            NamedColor::Magenta => Color32::from_rgb(188, 63, 188),
            NamedColor::Cyan => Color32::from_rgb(17, 168, 205),
            NamedColor::White => Color32::from_rgb(229, 229, 229),
            NamedColor::BrightBlack => Color32::from_rgb(102, 102, 102),
            NamedColor::BrightRed => Color32::from_rgb(241, 76, 76),
            NamedColor::BrightGreen => Color32::from_rgb(35, 209, 139),
            NamedColor::BrightYellow => Color32::from_rgb(245, 245, 67),
            NamedColor::BrightBlue => Color32::from_rgb(59, 142, 234),
            NamedColor::BrightMagenta => Color32::from_rgb(214, 112, 214),
            NamedColor::BrightCyan => Color32::from_rgb(41, 184, 219),
            NamedColor::BrightWhite => Color32::from_rgb(255, 255, 255),
            NamedColor::Foreground => fallback,
            NamedColor::Background => Color32::from_rgb(30, 30, 30),
            _ => fallback,
        },
    }
}

fn indexed_to_color(idx: u8) -> Color32 {
    if idx < 16 {
        return match idx {
            0 => Color32::from_rgb(0, 0, 0),
            1 => Color32::from_rgb(205, 49, 49),
            2 => Color32::from_rgb(13, 188, 121),
            3 => Color32::from_rgb(229, 229, 16),
            4 => Color32::from_rgb(36, 114, 200),
            5 => Color32::from_rgb(188, 63, 188),
            6 => Color32::from_rgb(17, 168, 205),
            7 => Color32::from_rgb(229, 229, 229),
            8 => Color32::from_rgb(102, 102, 102),
            9 => Color32::from_rgb(241, 76, 76),
            10 => Color32::from_rgb(35, 209, 139),
            11 => Color32::from_rgb(245, 245, 67),
            12 => Color32::from_rgb(59, 142, 234),
            13 => Color32::from_rgb(214, 112, 214),
            14 => Color32::from_rgb(41, 184, 219),
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

