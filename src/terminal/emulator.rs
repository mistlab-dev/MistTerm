//! 终端模拟器 - ANSI 转义码解析和渲染
#![allow(dead_code)]

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
    CharsetDesignate,
}

/// 终端模拟器（可见区为 `height`×`width` 字符网格；`lines` 为滚出顶部的历史）
#[derive(Debug)]
pub struct Terminal {
    /// 滚出屏幕顶部的历史行
    pub lines: VecDeque<Vec<Cell>>,
    /// 备用屏幕模式下进入前保存的 `lines`
    saved_scrollback: VecDeque<Vec<Cell>>,
    /// 当前可见字符网格，行 `0..height`
    screen: Vec<Vec<Cell>>,
    /// 是否在 DEC 备用屏幕（vim 全屏等）
    alt_screen: bool,
    /// 当前光标位置（网格坐标）
    pub cursor_x: usize,
    pub cursor_y: usize,
    saved_cursor_x: usize,
    saved_cursor_y: usize,
    cursor_visible: bool,
    /// 终端尺寸
    pub width: usize,
    pub height: usize,
    /// 当前样式
    current_style: CharStyle,
    /// ANSI 解析状态
    ansi_state: AnsiState,
    /// CSI 参数缓冲
    csi_params: String,
    /// 最大滚出行数
    max_lines: usize,
    /// UTF-8 多字节序列暂存
    utf8_pending: Vec<u8>,
}

impl Default for Terminal {
    fn default() -> Self {
        Self::new(80, 24)
    }
}

impl Terminal {
    fn fresh_row(width: usize) -> Vec<Cell> {
        vec![Cell::default(); width]
    }

    pub fn new(width: usize, height: usize) -> Self {
        let width = width.max(20).min(512);
        let height = height.max(5).min(256);
        let screen = (0..height).map(|_| Self::fresh_row(width)).collect();
        Self {
            lines: VecDeque::new(),
            saved_scrollback: VecDeque::new(),
            screen,
            alt_screen: false,
            cursor_x: 0,
            cursor_y: 0,
            saved_cursor_x: 0,
            saved_cursor_y: 0,
            cursor_visible: true,
            width,
            height,
            current_style: CharStyle::default(),
            ansi_state: AnsiState::Normal,
            csi_params: String::new(),
            max_lines: 1000,
            utf8_pending: Vec::new(),
        }
    }

    /// 与远端 PTY 同步字符网格尺寸
    pub fn resize(&mut self, width: usize, height: usize) {
        let width = width.clamp(20, 512);
        let height = height.clamp(5, 256);
        if width == self.width && height == self.height {
            return;
        }
        self.width = width;
        self.height = height;
        for row in &mut self.screen {
            row.resize(width, Cell::default());
            if row.len() > width {
                row.truncate(width);
            }
        }
        while self.screen.len() < height {
            self.screen.push(Self::fresh_row(width));
        }
        while self.screen.len() > height {
            self.screen.pop();
        }
        for line in &mut self.lines {
            if line.len() > width {
                line.truncate(width);
            }
        }
        self.cursor_x = self.cursor_x.min(self.width.saturating_sub(1));
        self.cursor_y = self.cursor_y.min(self.height.saturating_sub(1));
    }

    /// 处理输入数据
    pub fn feed(&mut self, data: &[u8]) {
        for &byte in data {
            self.process_byte(byte);
        }
    }

    fn scroll_up(&mut self) {
        if self.height == 0 {
            return;
        }
        let top = self.screen.remove(0);
        self.lines.push_back(top);
        while self.lines.len() > self.max_lines {
            self.lines.pop_front();
        }
        self.screen.push(Self::fresh_row(self.width));
    }

    fn wrap_advance_line(&mut self) {
        if self.cursor_y < self.height.saturating_sub(1) {
            self.cursor_y += 1;
            self.cursor_x = 0;
        } else {
            self.scroll_up();
            self.cursor_x = 0;
            self.cursor_y = self.height.saturating_sub(1);
        }
    }

    fn line_feed(&mut self) {
        if self.cursor_y < self.height.saturating_sub(1) {
            self.cursor_y += 1;
        } else {
            self.scroll_up();
        }
    }

    fn enter_alt_screen(&mut self) {
        if self.alt_screen {
            return;
        }
        self.alt_screen = true;
        self.saved_scrollback = std::mem::take(&mut self.lines);
        for row in &mut self.screen {
            *row = Self::fresh_row(self.width);
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
    }

    fn leave_alt_screen(&mut self) {
        if !self.alt_screen {
            return;
        }
        self.alt_screen = false;
        self.lines = std::mem::take(&mut self.saved_scrollback);
        for row in &mut self.screen {
            *row = Self::fresh_row(self.width);
        }
        self.cursor_x = 0;
        self.cursor_y = 0;
    }

    /// 处理单个字节
    fn process_byte(&mut self, byte: u8) {
        match self.ansi_state {
            AnsiState::Normal => {
                match byte {
                    0x1B => {
                        self.flush_utf8_pending();
                        self.ansi_state = AnsiState::Escape;
                    }
                    0x0D => {
                        self.cursor_x = 0;
                    }
                    0x0A => {
                        self.line_feed();
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
                            self.cursor_x = self.width.saturating_sub(1);
                        }
                    }
                    _ if byte >= 0x20 && byte < 0x7F => {
                        self.flush_utf8_pending();
                        self.write_char(byte as char);
                    }
                    _ if byte >= 0x80 => self.feed_utf8_byte(byte),
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
                        self.ansi_state = AnsiState::Osc;
                    }
                    b'7' => {
                        self.saved_cursor_x = self.cursor_x;
                        self.saved_cursor_y = self.cursor_y;
                        self.ansi_state = AnsiState::Normal;
                    }
                    b'8' => {
                        self.cursor_x = self.saved_cursor_x.min(self.width.saturating_sub(1));
                        self.cursor_y = self.saved_cursor_y.min(self.height.saturating_sub(1));
                        self.ansi_state = AnsiState::Normal;
                    }
                    b'(' | b')' => {
                        // 字符集指定（如 ESC(B / ESC(0）；当前先吞掉，保证不会漏残片字符
                        self.ansi_state = AnsiState::CharsetDesignate;
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
                if byte == 0x07 || byte == 0x1B {
                    self.ansi_state = AnsiState::Normal;
                }
            }
            AnsiState::CharsetDesignate => {
                self.ansi_state = AnsiState::Normal;
            }
        }
    }

    fn feed_utf8_byte(&mut self, byte: u8) {
        self.utf8_pending.push(byte);
        let decoded = std::str::from_utf8(&self.utf8_pending).ok().map(|s| s.to_string());
        if let Some(s) = decoded {
            for ch in s.chars() {
                self.write_char(ch);
            }
            self.utf8_pending.clear();
            return;
        }
        // 若不是前缀匹配，丢弃并重置，避免坏流污染后续渲染
        if self.utf8_pending.len() >= 4 {
            self.utf8_pending.clear();
        }
    }

    fn flush_utf8_pending(&mut self) {
        if self.utf8_pending.is_empty() {
            return;
        }
        let decoded = std::str::from_utf8(&self.utf8_pending).ok().map(|s| s.to_string());
        if let Some(s) = decoded {
            for ch in s.chars() {
                self.write_char(ch);
            }
        }
        self.utf8_pending.clear();
    }

    fn write_char(&mut self, ch: char) {
        let cell = Cell {
            ch,
            style: self.current_style.clone(),
        };
        if self.cursor_x >= self.width {
            self.wrap_advance_line();
        }
        if self.height == 0 || self.width == 0 {
            return;
        }
        self.screen[self.cursor_y][self.cursor_x] = cell;
        self.cursor_x += 1;
        if self.cursor_x >= self.width {
            self.wrap_advance_line();
        }
    }

    /// 执行 CSI（含 `CSI ? … h/l` 备用屏）
    fn execute_csi(&mut self, cmd: char) {
        if self.csi_params.starts_with('?') {
            self.execute_private_csi(cmd);
            return;
        }
        let params: Vec<usize> = self
            .csi_params
            .split(';')
            .filter_map(|s| s.parse().ok())
            .collect();

        match cmd {
            'm' => self.sgr(&params),
            'H' | 'f' => self.cursor_home(&params),
            'G' => self.cursor_horizontal_absolute(&params),
            'd' => self.cursor_vertical_absolute(&params),
            'J' => self.erase_display(&params),
            'K' => self.erase_line(&params),
            'A' => self.cursor_up(&params),
            'B' => self.cursor_down(&params),
            'C' => self.cursor_forward(&params),
            'D' => self.cursor_back(&params),
            'E' => self.cursor_next_line(&params),
            'F' => self.cursor_prev_line(&params),
            'P' => self.delete_chars(&params),
            '@' => self.insert_blank_chars(&params),
            'X' => self.erase_chars(&params),
            's' => {
                self.saved_cursor_x = self.cursor_x;
                self.saved_cursor_y = self.cursor_y;
            }
            'u' => {
                self.cursor_x = self.saved_cursor_x.min(self.width.saturating_sub(1));
                self.cursor_y = self.saved_cursor_y.min(self.height.saturating_sub(1));
            }
            _ => {}
        }
    }

    fn execute_private_csi(&mut self, cmd: char) {
        let rest = self.csi_params.strip_prefix('?').unwrap_or("");
        let modes: Vec<usize> = rest
            .split(';')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        match cmd {
            'h' => {
                for m in modes {
                    if m == 1049 {
                        self.enter_alt_screen();
                    } else if m == 25 {
                        self.cursor_visible = true;
                    }
                }
            }
            'l' => {
                for m in modes {
                    if m == 1049 {
                        self.leave_alt_screen();
                    } else if m == 25 {
                        self.cursor_visible = false;
                    }
                }
            }
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

    fn erase_display(&mut self, params: &[usize]) {
        let mode = params.first().copied().unwrap_or(0);
        match mode {
            0 => {
                for c in self.cursor_x..self.width {
                    if self.cursor_y < self.height {
                        self.screen[self.cursor_y][c] = Cell::default();
                    }
                }
                for r in (self.cursor_y + 1)..self.height {
                    self.screen[r] = Self::fresh_row(self.width);
                }
            }
            1 => {
                for r in 0..self.cursor_y {
                    self.screen[r] = Self::fresh_row(self.width);
                }
                if self.cursor_y < self.height {
                    for c in 0..=self.cursor_x.min(self.width.saturating_sub(1)) {
                        self.screen[self.cursor_y][c] = Cell::default();
                    }
                }
            }
            2 => {
                for row in &mut self.screen {
                    *row = Self::fresh_row(self.width);
                }
                self.cursor_x = 0;
                self.cursor_y = 0;
            }
            _ => {}
        }
    }

    fn erase_line(&mut self, params: &[usize]) {
        if self.cursor_y >= self.height {
            return;
        }
        let row = &mut self.screen[self.cursor_y];
        match params.first().copied().unwrap_or(0) {
            0 => {
                for c in self.cursor_x..self.width {
                    row[c] = Cell::default();
                }
            }
            1 => {
                for c in 0..=self.cursor_x.min(self.width.saturating_sub(1)) {
                    row[c] = Cell::default();
                }
            }
            2 => {
                *row = Self::fresh_row(self.width);
            }
            _ => {}
        }
    }

    fn cursor_up(&mut self, params: &[usize]) {
        let n = params.first().copied().unwrap_or(1).max(1);
        self.cursor_y = self.cursor_y.saturating_sub(n);
    }

    fn cursor_down(&mut self, params: &[usize]) {
        let n = params.first().copied().unwrap_or(1).max(1);
        self.cursor_y = (self.cursor_y + n).min(self.height.saturating_sub(1));
    }

    fn cursor_forward(&mut self, params: &[usize]) {
        let n = params.first().copied().unwrap_or(1).max(1);
        self.cursor_x = (self.cursor_x + n).min(self.width.saturating_sub(1));
    }

    fn cursor_back(&mut self, params: &[usize]) {
        let n = params.first().copied().unwrap_or(1).max(1);
        self.cursor_x = self.cursor_x.saturating_sub(n);
    }

    fn cursor_horizontal_absolute(&mut self, params: &[usize]) {
        let col = params.first().copied().unwrap_or(1).max(1).saturating_sub(1);
        self.cursor_x = col.min(self.width.saturating_sub(1));
    }

    fn cursor_vertical_absolute(&mut self, params: &[usize]) {
        let row = params.first().copied().unwrap_or(1).max(1).saturating_sub(1);
        self.cursor_y = row.min(self.height.saturating_sub(1));
    }

    fn cursor_next_line(&mut self, params: &[usize]) {
        let n = params.first().copied().unwrap_or(1).max(1);
        self.cursor_y = (self.cursor_y + n).min(self.height.saturating_sub(1));
        self.cursor_x = 0;
    }

    fn cursor_prev_line(&mut self, params: &[usize]) {
        let n = params.first().copied().unwrap_or(1).max(1);
        self.cursor_y = self.cursor_y.saturating_sub(n);
        self.cursor_x = 0;
    }

    fn delete_chars(&mut self, params: &[usize]) {
        if self.cursor_y >= self.height || self.cursor_x >= self.width {
            return;
        }
        let n = params.first().copied().unwrap_or(1).max(1).min(self.width - self.cursor_x);
        let row = &mut self.screen[self.cursor_y];
        for x in self.cursor_x..(self.width - n) {
            row[x] = row[x + n].clone();
        }
        for x in (self.width - n)..self.width {
            row[x] = Cell::default();
        }
    }

    fn insert_blank_chars(&mut self, params: &[usize]) {
        if self.cursor_y >= self.height || self.cursor_x >= self.width {
            return;
        }
        let n = params.first().copied().unwrap_or(1).max(1).min(self.width - self.cursor_x);
        let row = &mut self.screen[self.cursor_y];
        for x in (self.cursor_x..(self.width - n)).rev() {
            row[x + n] = row[x].clone();
        }
        for x in self.cursor_x..(self.cursor_x + n) {
            row[x] = Cell::default();
        }
    }

    fn erase_chars(&mut self, params: &[usize]) {
        if self.cursor_y >= self.height || self.cursor_x >= self.width {
            return;
        }
        let n = params.first().copied().unwrap_or(1).max(1).min(self.width - self.cursor_x);
        let row = &mut self.screen[self.cursor_y];
        for x in self.cursor_x..(self.cursor_x + n) {
            row[x] = Cell::default();
        }
    }

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
        for row in &self.screen {
            for cell in row {
                if cell.ch != ' ' {
                    result.push(cell.ch);
                }
            }
            result.push('\n');
        }
        result
    }

    fn append_row_with_cursor(
        &self,
        out: &mut String,
        row: &[Cell],
        row_idx: usize,
        insert_cursor: bool,
    ) {
        // 按固定网格宽度输出，避免裁剪行尾空格导致表格错位
        let mut chars: Vec<char> = row.iter().map(|c| c.ch).collect();
        // 光标应覆盖单元格而非插入（插入会把后续列右移，top/htop 会错位）
        if insert_cursor && row_idx == self.cursor_y && self.cursor_x < chars.len() {
            chars[self.cursor_x] = '│';
        }
        for ch in chars {
            out.push(ch);
        }
        out.push('\n');
    }

    /// 供 UI 显示的纯文本（滚出历史 + 当前屏幕网格；备用屏仅显示网格）
    pub fn get_formatted_output(&self) -> String {
        let mut result = String::new();
        if self.alt_screen {
            for (i, row) in self.screen.iter().enumerate() {
                self.append_row_with_cursor(&mut result, row, i, self.cursor_visible);
            }
            return result;
        }
        for row in &self.lines {
            for cell in row {
                result.push(cell.ch);
            }
            result.push('\n');
        }
        for (i, row) in self.screen.iter().enumerate() {
            self.append_row_with_cursor(&mut result, row, i, self.cursor_visible);
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
        let out = term.get_formatted_output();
        assert!(out.contains("Hello"));
    }

    #[test]
    fn test_ansi_color() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"\x1b[31mRed\x1b[0m\n");
        assert!(term.get_formatted_output().contains("Red"));
    }

    #[test]
    fn test_cursor_movement() {
        let mut term = Terminal::new(80, 24);
        term.feed(b"AB\x1b[D\x1b[DCD");
        assert_eq!(term.screen[0][0].ch, 'C');
        assert_eq!(term.screen[0][1].ch, 'D');
    }

    #[test]
    fn test_cup_writes_to_grid() {
        let mut term = Terminal::new(40, 12);
        term.feed(b"\x1b[5;10H");
        term.feed(b"X");
        assert_eq!(term.screen[4][9].ch, 'X');
    }

    #[test]
    fn test_alt_screen_toggle() {
        let mut term = Terminal::new(40, 8);
        term.feed(b"a\nb\n");
        assert!(!term.alt_screen);
        term.feed(b"\x1b[?1049h");
        assert!(term.alt_screen);
        term.feed(b"\x1b[2;2H");
        term.feed(b"V");
        assert_eq!(term.screen[1][1].ch, 'V');
        term.feed(b"\x1b[?1049l");
        assert!(!term.alt_screen);
    }
}
