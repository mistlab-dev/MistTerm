//! 终端模拟器 - ANSI 转义码解析和渲染

use egui::Color32;
use std::collections::VecDeque;

/// 字符样式
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CharStyle {
    pub foreground: Color32,
    pub background: Color32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

/// 终端单元格
#[derive(Debug, Clone)]
pub struct Cell {
    pub ch: char,
    pub style: CharStyle,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            style: CharStyle::default(),
        }
    }
}

/// ANSI 转义码解析器状态
#[derive(Debug, Clone, Default)]
pub enum AnsiState {
    #[default]
    Normal,
    Escape,
    Csi,
    CsiParam(String),
    Osc,
}

/// 终端模拟器
#[derive(Debug)]
pub struct Terminal {
    /// 终端行缓冲
    pub lines: VecDeque<Vec<Cell>>,
    /// 当前行
    pub current_line: Vec<Cell>,
    /// 当前光标位置
    pub cursor_x: usize,
    pub cursor_y: usize,
    /// 终端尺寸
    pub width: usize,
    pub height: usize,
    /// 当前样式
    current_style: CharStyle,
    /// ANSI 解析状态
    ansi_state: AnsiState,
    /// CSI 参数缓冲
    csi_params: String,
    /// 最大行数
    max_lines: usize,
}

impl Default for Terminal {
    fn default() -> Self {
        Self::new(80, 24)
    }
}

impl Terminal {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(height),
            current_line: Vec::with_capacity(width),
            cursor_x: 0,
            cursor_y: 0,
            width,
            height,
            current_style: CharStyle::default(),
            ansi_state: AnsiState::Normal,
            csi_params: String::new(),
            max_lines: 1000,
        }
    }

    /// 处理输入数据
    pub fn feed(&mut self, data: &[u8]) {
        for &byte in data {
            self.process_byte(byte);
        }
    }

    /// 处理单个字节
    fn process_byte(&mut self, byte: u8) {
        match self.ansi_state {
            AnsiState::Normal => {
                match byte {
                    0x1B => {
                        self.ansi_state = AnsiState::Escape;
                    }
                    0x0D => {
                        // CR: 回到行首，不换行（配合后续字符覆盖当前行）
                        self.cursor_x = 0;
                    }
                    0x0A => {
                        self.newline();
                    }
                    0x08 => {
                        if self.cursor_x > 0 {
                            self.cursor_x -= 1;
                        }
                    }
                    0x07 => {}
                    0x09 => {
                        self.cursor_x = (self.cursor_x + 8) & !7;
                        if self.cursor_x >= self.width {
                            self.cursor_x = self.width - 1;
                        }
                    }
                    _ if byte >= 0x20 && byte < 0x7F => {
                        self.write_char(byte as char);
                    }
                    _ => {}
                }
            }
            AnsiState::Escape => {
                match byte {
                    b'[' => {
                        self.ansi_state = AnsiState::Csi;
                        self.csi_params.clear();
                    }
                    b']' => {
                        // OSC 序列（如设置终端标题），跳过到 BEL/ESC 结束
                        self.ansi_state = AnsiState::Osc;
                    }
                    _ => {
                        self.ansi_state = AnsiState::Normal;
                    }
                }
            }
            AnsiState::Csi => {
                if byte >= 0x30 && byte <= 0x3F {
                    self.csi_params.push(byte as char);
                } else if byte >= 0x20 && byte <= 0x2F {
                    self.csi_params.push(byte as char);
                } else if byte >= 0x40 && byte <= 0x7E {
                    self.execute_csi(byte as char);
                    self.ansi_state = AnsiState::Normal;
                } else {
                    self.ansi_state = AnsiState::Normal;
                }
            }
            AnsiState::CsiParam(_) => {
                self.ansi_state = AnsiState::Normal;
            }
            AnsiState::Osc => {
                // OSC 常见结束符: BEL(\x07) 或 ESC
                if byte == 0x07 || byte == 0x1B {
                    self.ansi_state = AnsiState::Normal;
                }
            }
        }
    }

    /// 写入字符
    fn write_char(&mut self, ch: char) {
        let cell = Cell {
            ch,
            style: self.current_style.clone(),
        };

        if self.cursor_x >= self.width {
            self.newline();
        }

        if self.cursor_x < self.current_line.len() {
            self.current_line[self.cursor_x] = cell;
        } else {
            self.current_line.push(cell);
        }
        self.cursor_x += 1;
    }

    /// 换行
    fn newline(&mut self) {
        if !self.current_line.is_empty() || self.cursor_x > 0 {
            self.lines.push_back(std::mem::take(&mut self.current_line));
            self.current_line = Vec::with_capacity(self.width);
        }

        self.cursor_x = 0;
        self.cursor_y += 1;

        while self.lines.len() > self.max_lines {
            self.lines.pop_front();
        }
    }

    /// 执行 CSI 命令
    fn execute_csi(&mut self, cmd: char) {
        let params: Vec<usize> = self
            .csi_params
            .split(';')
            .filter_map(|s| s.parse().ok())
            .collect();

        match cmd {
            'm' => self.sgr(&params),
            'H' | 'f' => self.cursor_home(&params),
            'J' => self.erase_display(&params),
            'K' => self.erase_line(&params),
            'A' => self.cursor_up(&params),
            'B' => self.cursor_down(&params),
            'C' => self.cursor_forward(&params),
            'D' => self.cursor_back(&params),
            'l' | 'h' => {}
            _ => {}
        }
    }

    /// SGR - Select Graphic Rendition
    fn sgr(&mut self, params: &[usize]) {
        if params.is_empty() || params == &[0] {
            self.current_style = CharStyle::default();
            return;
        }

        let mut i = 0;
        while i < params.len() {
            match params[i] {
                0 => self.current_style = CharStyle::default(),
                1 => self.current_style.bold = true,
                3 => self.current_style.italic = true,
                4 => self.current_style.underline = true,
                9 => self.current_style.strikethrough = true,
                22 => {
                    self.current_style.bold = false;
                    self.current_style.italic = false;
                }
                24 => self.current_style.underline = false,
                39 => self.current_style.foreground = Color32::LIGHT_GREEN,
                49 => self.current_style.background = Color32::BLACK,
                30..=37 => {
                    self.current_style.foreground = self.get_color(params[i], false);
                }
                38 => {
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        self.current_style.foreground = self.get_256_color(params[i + 2]);
                        i += 2;
                    }
                }
                40..=47 => {
                    self.current_style.background = self.get_color(params[i], true);
                }
                48 => {
                    if i + 2 < params.len() && params[i + 1] == 5 {
                        self.current_style.background = self.get_256_color(params[i + 2]);
                        i += 2;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// 获取标准颜色
    fn get_color(&self, param: usize, is_bg: bool) -> Color32 {
        if is_bg {
            match param {
                40 => Color32::BLACK,
                41 => Color32::RED,
                42 => Color32::GREEN,
                43 => Color32::YELLOW,
                44 => Color32::BLUE,
                45 => Color32::from_rgb(128, 0, 128),
                46 => Color32::from_rgb(0, 128, 128),
                47 => Color32::WHITE,
                _ => Color32::BLACK,
            }
        } else {
            match param {
                30 => Color32::from_rgb(128, 0, 0),
                31 => Color32::from_rgb(0, 128, 0),
                32 => Color32::from_rgb(128, 128, 0),
                33 => Color32::from_rgb(0, 0, 128),
                34 => Color32::from_rgb(128, 0, 128),
                35 => Color32::from_rgb(0, 128, 128),
                36 => Color32::from_rgb(192, 192, 192),
                37 => Color32::WHITE,
                _ => Color32::LIGHT_GREEN,
            }
        }
    }

    /// 获取 256 色
    fn get_256_color(&self, color_idx: usize) -> Color32 {
        if color_idx < 16 {
            self.get_color(30 + color_idx, false)
        } else if color_idx < 232 {
            let idx = color_idx - 16;
            let r = (idx / 36) % 6;
            let g = (idx / 6) % 6;
            let b = idx % 6;
            Color32::from_rgb(
                (55 + r * 40) as u8,
                (55 + g * 40) as u8,
                (55 + b * 40) as u8,
            )
        } else if color_idx < 256 {
            let gray = 8 + (color_idx - 232) * 10;
            Color32::from_rgb(gray as u8, gray as u8, gray as u8)
        } else {
            Color32::LIGHT_GREEN
        }
    }

    /// 光标位置
    fn cursor_home(&mut self, params: &[usize]) {
        let row = if params.is_empty() || params[0] == 0 {
            0
        } else {
            params[0].saturating_sub(1)
        };
        let col = if params.len() > 1 && params[1] != 0 {
            params[1].saturating_sub(1)
        } else {
            0
        };

        self.cursor_y = row.min(self.height.saturating_sub(1));
        self.cursor_x = col.min(self.width.saturating_sub(1));
    }

    /// 清除显示
    fn erase_display(&mut self, params: &[usize]) {
        match params.first().copied().unwrap_or(0) {
            0 => {
                self.current_line.truncate(self.cursor_x);
                self.lines.clear();
            }
            1 => {
                self.lines.clear();
                self.current_line.clear();
                self.cursor_x = 0;
            }
            2 => {
                self.lines.clear();
                self.current_line.clear();
                self.cursor_x = 0;
                self.cursor_y = 0;
            }
            _ => {}
        }
    }

    /// 清除行
    fn erase_line(&mut self, params: &[usize]) {
        match params.first().copied().unwrap_or(0) {
            0 => self.current_line.truncate(self.cursor_x),
            1 => {
                self.current_line.drain(0..self.cursor_x.min(self.current_line.len()));
                self.cursor_x = 0;
            }
            2 => self.current_line.clear(),
            _ => {}
        }
    }

    /// 光标上移
    fn cursor_up(&mut self, params: &[usize]) {
        let n = params.first().copied().unwrap_or(1).max(1);
        if n > 0 && self.cursor_y > 0 {
            self.cursor_y = self.cursor_y.saturating_sub(n);
        }
    }

    /// 光标下移
    fn cursor_down(&mut self, params: &[usize]) {
        let n = params.first().copied().unwrap_or(1).max(1);
        self.cursor_y = (self.cursor_y + n).min(self.height.saturating_sub(1));
    }

    /// 光标前移
    fn cursor_forward(&mut self, params: &[usize]) {
        let n = params.first().copied().unwrap_or(1).max(1);
        self.cursor_x = (self.cursor_x + n).min(self.width.saturating_sub(1));
    }

    /// 光标后移
    fn cursor_back(&mut self, params: &[usize]) {
        let n = params.first().copied().unwrap_or(1).max(1);
        self.cursor_x = self.cursor_x.saturating_sub(n);
    }

    /// 获取纯文本输出
    pub fn to_plain_text(&self) -> String {
        let mut result = String::new();
        for line in &self.lines {
            for cell in line {
                if cell.ch != ' ' {
                    result.push(cell.ch);
                }
            }
            result.push('\n');
        }
        result
    }

    /// 获取带样式的输出
    pub fn get_formatted_output(&self) -> String {
        let mut result = String::new();
        for line in &self.lines {
            for cell in line {
                result.push(cell.ch);
            }
            result.push('\n');
        }

        if !self.current_line.is_empty() {
            for cell in &self.current_line {
                result.push(cell.ch);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_output() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"Hello, World!\n");
        assert_eq!(term.lines.len(), 1);
    }

    #[test]
    fn test_ansi_color() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"\x1b[31mRed\x1b[0m\n");
        assert_eq!(term.lines.len(), 1);
    }

    #[test]
    fn test_cursor_movement() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"AB\x1b[D\x1b[DCD");
        assert_eq!(term.current_line.len(), 4);
    }
}
