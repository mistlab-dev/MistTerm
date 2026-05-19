//! 终端视图
#![allow(dead_code)]
//!
//! 显示终端模拟器、处理输入输出、集成 SSH 连接。
//!
//! **与本文件相关的传文件入口（与 SFTP 侧栏、终端内 `rz` 并列，互不合并实现）**：
//! - **ZMODEM**：`rz` 检测 → `LrzszTransfer::start_send`（`zmodem2` + shell 泵）。
//! - **直传·SCP**：[`TerminalView::start_upload`](TerminalView::start_upload)（当前为 `scp_send`）。
//! - **直传·cat**：[`TerminalView::start_upload_to_remote`](TerminalView::start_upload_to_remote)（`cat >` 通道）。

use eframe::egui;
use arboard::Clipboard;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use crate::ssh::{SshManager, SshConfig, SshMessage, SshSessionHandle, SshSessionId, LrzszTransfer, TransferEvent, format_ssh_connect_error};
use alacritty_terminal::grid::Scroll;
use crate::terminal::{Terminal as VtTerminal, TerminalShellStyle};
use crate::terminal::style::{
    format_user_error_line, format_user_info_line, format_user_success_line, format_user_warn_line,
};
use crate::ui::theme::Theme;

/// 与 [`VtTerminal::content_epoch`] 组合，避免 PTY 无输出帧重复构建整屏 [`egui::text::LayoutJob`]。
struct TerminalVisualLayoutCache {
    vt_gen: u64,
    content_epoch: u64,
    cols: u32,
    rows: u32,
    font_bits: u32,
    fg: egui::Color32,
    bg: egui::Color32,
    layout_job: egui::text::LayoutJob,
    formatted: String,
}

impl TerminalVisualLayoutCache {
    fn matches(
        &self,
        vt_gen: u64,
        content_epoch: u64,
        cols: u32,
        rows: u32,
        font_bits: u32,
        fg: egui::Color32,
        bg: egui::Color32,
    ) -> bool {
        self.vt_gen == vt_gen
            && self.content_epoch == content_epoch
            && self.cols == cols
            && self.rows == rows
            && self.font_bits == font_bits
            && self.fg == fg
            && self.bg == bg
    }
}

/// 底栏展示的 SSH 连接状态（不写入 VTE scrollback）。
#[derive(Clone, Debug)]
pub struct ConnectionBarStatus {
    pub host_line: String,
    pub state_line: String,
    pub state_color: egui::Color32,
}

fn truncate_connection_status(s: &str, max_chars: usize) -> String {
    let s = s.trim();
    let mut it = s.chars();
    let head: String = it.by_ref().take(max_chars).collect();
    if it.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

/// 终端文本选择（行号从 0 开始，列号从 0 开始）
#[derive(Clone, Debug, Default)]
struct Selection {
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
    active: bool,
}

impl Selection {
    fn is_empty(&self) -> bool {
        !self.active || (self.start_line == self.end_line && self.start_col == self.end_col)
    }

    fn normalize(&self) -> (usize, usize, usize, usize) {
        if (self.start_line, self.start_col) <= (self.end_line, self.end_col) {
            (self.start_line, self.start_col, self.end_line, self.end_col)
        } else {
            (self.end_line, self.end_col, self.start_line, self.start_col)
        }
    }

    fn clear(&mut self) {
        self.active = false;
        self.start_line = 0;
        self.start_col = 0;
        self.end_line = 0;
        self.end_col = 0;
    }
}

/// 终端视图组件
pub struct TerminalView {
    /// 会话 ID
    session_id: Option<usize>,
    
    /// SSH 管理器
    ssh_manager: Option<SshManager>,
    
    /// SSH 消息接收器
    ssh_rx: Option<Receiver<SshMessage>>,
    
    /// SSH 会话句柄
    ssh_handle: Option<SshSessionHandle>,
    
    /// VT100/ANSI 终端模拟器（含光标、屏幕、滚动历史）
    terminal: VtTerminal,
    
    /// 连接状态
    connected: bool,
    
    /// 连接错误信息
    error_message: Option<String>,
    
    /// 终端尺寸
    cols: u32,
    rows: u32,
    
    /// lrzsz 文件传输器
    lrzsz: LrzszTransfer,
    
    /// 命令片段面板可见性
    pub show_fragment_panel: bool,
    
    /// 是否需要弹出文件选择对话框（检测到 rz 命令时设置）
    pub pending_rz_upload: bool,
    
    /// 文件传输进度（文件名、已传字节、总字节）
    transfer_progress: Option<(String, u64, u64)>,
    /// 当前传输是否为本机→远端（`rz` 上传）；用于文案与收尾
    transfer_outgoing: bool,
    /// 连接成功后下一帧请求一次终端焦点（避免每帧 `request_focus` 与 PTY 光标叠成双光标）
    pending_focus_terminal: bool,
    
    /// 下载目录
    download_dir: String,
    font_size: f32,
    connected_at: Option<Instant>,
    connection_target: Option<(String, String)>,
    auto_follow_output: bool,
    terminal_focused: bool,
    rz_control_mode_until: Option<Instant>,
    upload_result_rx: Option<Receiver<Result<String, String>>>,
    command_usage: HashMap<String, u64>,
    /// FUNCTIONAL_SPEC §2.4：超长粘贴分片发送（>10KB 时每批 4096 字节、间隔 5ms）
    paste_pending: Vec<u8>,
    paste_next_chunk_at: Option<Instant>,
    /// 「仅断开 SSH 保留画面」后为 true：键盘输入写入 [`Self::disconnected_input_buffer`] 而非 PTY（FUNCTIONAL_SPEC §2.4）。
    buffer_input_while_disconnected: bool,
    /// 断线期间缓存的待发送字节（上限 [`Self::OFFLINE_INPUT_CAP`]）。
    disconnected_input_buffer: Vec<u8>,
    /// 重连且 shell 就绪后，若缓存非空则弹出是否重发。
    resend_offline_input_dialog_open: bool,
    /// 整屏替换 VTE 或清屏时递增，与 [`VtTerminal::content_epoch`] 一并参与布局缓存键（FUNCTIONAL_SPEC §2.3.1）。
    vt_visual_generation: u64,
    visual_layout_cache: Option<TerminalVisualLayoutCache>,
    /// 用户主动断开（`disconnect` / `disconnect_ssh_keep_buffer`）期间忽略随后到达的 `Disconnected`，避免误触发自动重连。
    local_disconnect_intent: bool,
    /// 本轮 `process_ssh_messages` 后若曾为「非主动」断开则置位；由宿主 `take()` 后清除。
    unexpected_disconnect_notified: bool,
    /// 连接成功/失败待宿主写入审计（`take_connect_audit` 取走）
    pending_connect_audit: Option<(bool, String)>,
    /// 拖入终端区域、待宿主处理的上传路径（§4.3.2）。
    pending_drop_upload_paths: Vec<PathBuf>,
    /// 大文件上传：用户选 ZMODEM 后先发 `rz -y`，握手检测到后再用此路径 `start_rz_upload`。
    zmodem_upload_after_rz_path: Option<PathBuf>,
    /// 终端文本选择状态
    selection: Selection,
    /// 当前行已键入字节（用于 Ctrl+R 命令历史，近似 PTY 行缓冲）
    typed_line_buffer: String,
    /// Enter 提交后待取走的整行命令（供命令历史 / 会话日志）
    submitted_line: Option<String>,
    /// 待写入会话日志的命令（片段执行、Enter 提交等）
    pending_log_commands: Vec<String>,
    /// 待写入会话日志的 PTY 输出块
    pending_log_output: Vec<Vec<u8>>,
    /// 查找命中高亮：`(行, 列, 长度)` 均为 1-based 字符下标
    search_highlight: Option<(usize, usize, usize)>,
}

impl TerminalView {
    /// 断线缓存输入上限（字节）
    const OFFLINE_INPUT_CAP: usize = 64 * 1024;

    const SFTP_RETRY_ATTEMPTS: usize = 160;
    const SFTP_RETRY_SLEEP_MS: u64 = 8;
    /// Scroll 内容与视口边框的极小余量，避免偶发裁切一个字形
    const INNER_TEXT_SLACK: f32 = 0.0;
    /// ScrollArea **内容区内宽**（已不含纵向滚动条）→ TextEdit.desired_width
    #[inline]
    fn text_width_in_scroll_viewport(scroll_inner_width: f32) -> f32 {
        (scroll_inner_width - Self::INNER_TEXT_SLACK).max(64.0)
    }

    fn layout_terminal_galley(
        ui: &egui::Ui,
        layout_job: &egui::text::LayoutJob,
    ) -> std::sync::Arc<egui::Galley> {
        ui.ctx().fonts(|f| f.layout_job(layout_job.clone()))
    }

    /// 与 TextEdit `vertical_align(BOTTOM)` 一致：文本块顶边（勿用 `行数 * cell_h` 估算）。
    fn terminal_text_top(response: &egui::Response, galley: &egui::Galley) -> f32 {
        response.rect.max.y - galley.size().y
    }

    fn terminal_cell_metrics(ui: &egui::Ui, font_size: f32, color: egui::Color32) -> (f32, f32) {
        ui.ctx().fonts(|fonts| {
            let galley = fonts.layout_no_wrap(
                "W".to_string(),
                egui::FontId::monospace(font_size),
                color,
            );
            (galley.size().x.max(6.0), galley.size().y.max(12.0))
        })
    }

    fn terminal_row_col_at_pointer(
        galley: &egui::Galley,
        text_top: f32,
        response: &egui::Response,
        pos: egui::Pos2,
    ) -> (usize, usize) {
        let rel_y = (pos.y - text_top).max(0.0);
        let row_i = galley
            .rows
            .iter()
            .enumerate()
            .find(|(_, r)| rel_y >= r.rect.min.y && rel_y < r.rect.max.y)
            .map(|(i, _)| i)
            .unwrap_or_else(|| galley.rows.len().saturating_sub(1));
        let row = galley.rows.get(row_i);
        let col = row
            .map(|r| r.char_at((pos.x - response.rect.min.x).max(0.0)))
            .unwrap_or(0);
        (row_i, col)
    }

    fn terminal_block_cursor_rect(
        response: &egui::Response,
        galley: &egui::Galley,
        text_top: f32,
        cur: crate::terminal::ViewportCursor,
        cell_w: f32,
    ) -> egui::Rect {
        let row_i = cur.row.min(galley.rows.len().saturating_sub(1));
        let row = &galley.rows[row_i];
        let y = text_top + row.rect.min.y;
        let h = row.height().max(1.0);
        let x = response.rect.min.x + row.x_offset(cur.col);
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(cell_w, h))
    }

    fn terminal_selection_rect(
        response: &egui::Response,
        galley: &egui::Galley,
        text_top: f32,
        line_idx: usize,
        c_start: usize,
        c_end: usize,
        cell_w: f32,
    ) -> Option<egui::Rect> {
        let row = galley.rows.get(line_idx)?;
        if c_start >= c_end {
            return None;
        }
        let y = text_top + row.rect.min.y;
        let h = row.height().max(1.0);
        let x_start = response.rect.min.x + row.x_offset(c_start);
        let x_end = response.rect.min.x + row.x_offset(c_end).max(x_start + cell_w * 0.5);
        Some(egui::Rect::from_min_max(
            egui::pos2(x_start, y),
            egui::pos2(x_end, y + h),
        ))
    }

    /// 本机状态行：truecolor ANSI（随主题），避免被 §2.3.2 输出压暗后在黑底上几乎看不见。
    fn feed_user_error_line(&mut self, theme: &Theme, message: &str) {
        let line = format_user_error_line(theme, message);
        self.terminal.feed(line.as_bytes());
    }

    fn feed_user_info_line(&mut self, theme: &Theme, message: &str) {
        let line = format_user_info_line(theme, message);
        self.terminal.feed(line.as_bytes());
    }

    fn feed_user_success_line(&mut self, theme: &Theme, message: &str) {
        let line = format_user_success_line(theme, message);
        self.terminal.feed(line.as_bytes());
    }

    fn feed_user_warn_line(&mut self, theme: &Theme, message: &str) {
        let line = format_user_warn_line(theme, message);
        self.terminal.feed(line.as_bytes());
    }

    /// §4.3.2：拖放文件到终端区域时收集路径（由宿主决定 SCP / ZMODEM）。
    fn collect_file_drops_into(ui: &egui::Ui, pending: &mut Vec<PathBuf>) {
        if ui.ctx().input(|i| i.raw.dropped_files.is_empty()) {
            return;
        }
        let rect = ui.clip_rect();
        let inside = ui.ctx().input(|i| {
            i.pointer
                .interact_pos()
                .map(|p| rect.contains(p))
                .unwrap_or(false)
        });
        if !inside {
            return;
        }
        ui.ctx().input(|i| {
            for f in &i.raw.dropped_files {
                if let Some(p) = &f.path {
                    if !pending.iter().any(|x| x == p) {
                        pending.push(p.clone());
                    }
                }
            }
        });
    }

    fn is_would_block_text(msg: &str) -> bool {
        let msg = msg.to_lowercase();
        msg.contains("would block")
            || msg.contains("eagain")
            || msg.contains("resource temporarily unavailable")
            || msg.contains("libssh2_error_eagain")
            || msg.contains("try again")
    }

    fn is_would_block_like(err: &std::io::Error) -> bool {
        if err.kind() == std::io::ErrorKind::WouldBlock
            || err.kind() == std::io::ErrorKind::Interrupted
        {
            return true;
        }
        Self::is_would_block_text(&err.to_string())
    }

    fn retry_sftp_op<T, E, F>(mut op: F, context: &str) -> Result<T, String>
    where
        F: FnMut() -> Result<T, E>,
        E: std::fmt::Display,
    {
        let mut last_err: Option<E> = None;
        for _ in 0..Self::SFTP_RETRY_ATTEMPTS {
            match op() {
                Ok(v) => return Ok(v),
                Err(e) => {
                    let msg = e.to_string();
                    if Self::is_would_block_text(&msg) {
                        last_err = Some(e);
                        thread::sleep(Duration::from_millis(Self::SFTP_RETRY_SLEEP_MS));
                        continue;
                    }
                    return Err(format!("{}：{}", context, msg));
                }
            }
        }
        if let Some(e) = last_err {
            return Err(format!("{}：重试超时（最后错误：{}）", context, e));
        }
        Err(format!("{}：重试失败", context))
    }

    fn contains_shell_prompt_fragment(text: &str) -> bool {
        text.contains("$ ") || text.contains("# ") || text.contains("> ")
    }

    /// 上传旁路开启时默认不把 PTY 画进 VTE（避免 ZMODEM 二进制污染）；设 `MISTTERM_ZMODEM_MIRROR_RZ_TEXT=1` 可把**疑似纯文本**片段镜像到终端，便于看 `rz -vv`。
    fn mirror_rz_text_to_vte_enabled() -> bool {
        std::env::var("MISTTERM_ZMODEM_MIRROR_RZ_TEXT")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    }

    /// 可安全映到 VTE：不得含 ZMODEM ZDLE，以免损坏模拟器状态。
    fn pty_chunk_safe_to_mirror_vte(data: &[u8]) -> bool {
        if data.is_empty() || data.contains(&0x18) {
            return false;
        }
        data.iter().all(|&b| {
            matches!(b, b'\n' | b'\r' | b'\t' | b'\x08' | b'\x1b')
                || (b >= 0x20 && b < 0x7f)
        })
    }


    /// 创建新的终端视图
    pub fn new() -> Self {
        let download_dir = std::env::temp_dir().join("mistterm_downloads");
        let _ = std::fs::create_dir_all(&download_dir);
        
        Self {
            session_id: None,
            ssh_manager: None,
            ssh_rx: None,
            ssh_handle: None,
            terminal: VtTerminal::new(80, 24),
            connected: false,
            error_message: None,
            cols: 80,
            rows: 24,
            lrzsz: LrzszTransfer::new(&download_dir.to_string_lossy()),
            show_fragment_panel: false,
            pending_rz_upload: false,
            transfer_progress: None,
            transfer_outgoing: false,
            pending_focus_terminal: false,
            download_dir: download_dir.to_string_lossy().to_string(),
            font_size: 13.0,
            connected_at: None,
            connection_target: None,
            auto_follow_output: true,
            terminal_focused: false,
            rz_control_mode_until: None,
            upload_result_rx: None,
            command_usage: HashMap::new(),
            paste_pending: Vec::new(),
            paste_next_chunk_at: None,
            buffer_input_while_disconnected: false,
            disconnected_input_buffer: Vec::new(),
            resend_offline_input_dialog_open: false,
            vt_visual_generation: 0,
            visual_layout_cache: None,
            local_disconnect_intent: false,
            unexpected_disconnect_notified: false,
            pending_connect_audit: None,
            pending_drop_upload_paths: Vec::new(),
            zmodem_upload_after_rz_path: None,
            selection: Selection::default(),
            typed_line_buffer: String::new(),
            submitted_line: None,
            pending_log_commands: Vec::new(),
            pending_log_output: Vec::new(),
            search_highlight: None,
        }
    }

    pub fn set_search_highlight(&mut self, highlight: Option<(usize, usize, usize)>) {
        if self.search_highlight != highlight {
            self.search_highlight = highlight;
            self.visual_layout_cache = None;
        }
    }

    pub fn take_pending_log_output(&mut self) -> Option<Vec<u8>> {
        self.pending_log_output.pop()
    }

    /// Enter 提交后取走当前行命令（供命令历史记录）
    pub fn take_submitted_line(&mut self) -> Option<String> {
        self.submitted_line.take()
    }

    /// 取走待写入会话日志的命令
    pub fn take_pending_log_command(&mut self) -> Option<String> {
        if self.pending_log_commands.is_empty() {
            None
        } else {
            Some(self.pending_log_commands.remove(0))
        }
    }

    fn commit_typed_line_on_enter(&mut self) {
        let line = self.typed_line_buffer.trim().to_string();
        self.typed_line_buffer.clear();
        if line.is_empty() {
            return;
        }
        self.submitted_line = Some(line.clone());
        self.pending_log_commands.push(line);
    }

    fn enqueue_log_command(&mut self, command: &str) {
        let trimmed = command.trim();
        if !trimmed.is_empty() {
            self.pending_log_commands.push(trimmed.to_string());
        }
    }

    fn track_typed_text(&mut self, text: &str) {
        for ch in text.chars() {
            if ch == '\n' || ch == '\r' {
                continue;
            }
            if ch.is_control() && ch != '\t' {
                continue;
            }
            self.typed_line_buffer.push(ch);
        }
    }

    fn track_backspace(&mut self) {
        if !self.typed_line_buffer.is_empty() {
            self.typed_line_buffer.pop();
        }
    }

    /// 连接到 SSH 服务器
    pub fn connect(
        &mut self,
        _theme: &Theme,
        host: &str,
        port: u16,
        username: &str,
        password: &str,
        private_key_path: &str,
        keepalive_enabled: bool,
        keepalive_interval_secs: u32,
        keepalive_count_max: u8,
    ) {
        let interval = if keepalive_enabled {
            keepalive_interval_secs.max(1)
        } else {
            0
        };
        let config = SshConfig {
            host: host.to_string(),
            port,
            username: username.to_string(),
            password: password.to_string(),
            private_key_path: private_key_path.to_string(),
            keepalive_interval_secs: interval,
            keepalive_count_max: keepalive_count_max.max(1),
        };

        let (manager, rx) = SshManager::new();
        
        match manager.create_session_async(config) {
            Ok(session_id) => {
                self.buffer_input_while_disconnected = false;
                self.disconnected_input_buffer.clear();
                self.resend_offline_input_dialog_open = false;
                self.session_id = Some(session_id);
                self.ssh_manager = Some(manager);
                self.ssh_rx = Some(rx);
                self.connected = false;
                self.error_message = None;
                self.terminal = VtTerminal::new(self.cols as usize, self.rows as usize);
                self.vt_visual_generation = self.vt_visual_generation.wrapping_add(1);
                self.visual_layout_cache = None;
                self.local_disconnect_intent = false;
                self.connected_at = None;
                self.connection_target = Some((username.to_string(), host.to_string()));
            }
            Err(e) => {
                self.error_message = Some(format_ssh_connect_error(&format!(
                    "Failed to create session: {}",
                    e
                )));
            }
        }
    }

    /// 显示终端视图。`column_width` 须为宿主在 **标签栏下方** 为终端列 `allocate_ui_with_layout` 的宽度（勿用 `clip_rect`，常为整窗宽）。
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        column_width: f32,
        terminal_search_open: bool,
        capture_pty_keyboard: bool,
    ) {
        // 先处理网络与键盘，再绘制，避免输入/输出滞后一帧
        self.process_ssh_messages(theme);
        self.flush_paste_queue(ui.ctx());
        self.process_transfer_events(theme, ui.ctx());

        // Ctrl + 滚轮：缩放终端字体（不改变 PTY 行列，仅视觉）
        let wheel = ui.ctx().input(|i| {
            let z = i.scroll_delta.y;
            if self.terminal_focused
                && (self.connected || self.buffer_input_while_disconnected)
                && i.modifiers.ctrl
                && !i.modifiers.shift
                && z.abs() > 0.5
            {
                Some(z)
            } else {
                None
            }
        });
        if let Some(z) = wheel {
            let step = if z > 0.0 { 1.0 } else { -1.0 };
            self.set_font_size(self.font_size + step);
        }

        if capture_pty_keyboard {
            self.capture_inline_input(ui);
        }
        if self.connected {
            // ZMODEM 上传时 PTY 回包经 UI 旁路进 lrzsz；须高频重绘以免 `mpsc` 积压导致「等 ZACK」式卡顿。
            if self.lrzsz.is_upload_pty_capture() {
                ui.ctx().request_repaint();
            } else {
                // 保持动态程序（top/vim）持续刷新
                ui.ctx().request_repaint_after(Duration::from_millis(33));
            }
        }

        let column_width = column_width.max(1.0).min(16_384.0);
        // 宿主传入的列宽（已扣右侧 dock）；勿仅用 available_width，否则中央区 max_rect 仍可能盖住右栏。
        ui.set_max_width(column_width);

        // 吃满宿主矩形，但不得超过 column_width（否则 Central 后绘会盖住右栏）
        let mut fill_region = ui.available_size();
        if fill_region.x.is_finite() {
            fill_region.x = fill_region.x.min(column_width);
        }
        if fill_region.x.is_finite()
            && fill_region.y.is_finite()
            && fill_region.x > 0.0
            && fill_region.y > 0.0
        {
            ui.set_min_size(fill_region);
        }
        // 进度条在 Frame 内先占位；若仍用全高算行列，网格会高于 ScrollArea，滚动与「│」光标错位
        // 与 Frame 内底部进度条占位一致（分隔线 + 两行文案 + ProgressBar）
        const TRANSFER_FOOTER_H: f32 = 72.0;
        // README §2.4：主底栏承担状态；PTY 尺寸在 ScrollArea 内容区内按真实 viewport 同步（见下方）
        egui::Frame::none()
            .fill(theme.bg_terminal_color())
            .inner_margin(theme.terminal_content_margin())
            .show(ui, |ui| {
                // Scroll 滑道、Multiline/TextEdit(code_editor) 背板都用「极暗底色」；
                // 设为终端同色，否则会露出比 bg_terminal 更浅的灰条。
                let prev_extreme = ui.visuals().extreme_bg_color;
                ui.visuals_mut().extreme_bg_color = theme.bg_terminal_color();
                let inner_fill = ui.available_size();
                if inner_fill.x.is_finite()
                    && inner_fill.y.is_finite()
                    && inner_fill.x > 0.0
                    && inner_fill.y > 0.0
                {
                    ui.set_min_size(inner_fill);
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    // 同上：收窄的 min_rect 会让 Frame.fill 不按整列铺开，左侧/上出现 bg_body 「灰框」
                    let col_w = ui.available_width().min(column_width);
                    let col_h = ui.available_height();
                    ui.set_max_width(column_width);
                    ui.set_min_width(col_w.max(1.0));
                    ui.set_min_height(col_h.max(1.0));
                    Self::collect_file_drops_into(ui, &mut self.pending_drop_upload_paths);
                    let footer_h = if self.transfer_progress.is_some() {
                        TRANSFER_FOOTER_H
                    } else {
                        0.0
                    };
                    let scroll_h = (ui.available_height() - footer_h).max(80.0);

                    // 终端内容区在上，ZMODEM 进度条固定在底部，避免插在命令与 shell 输出之间
                    let shell = TerminalShellStyle::from_theme(theme);
                    let font_bits = self.font_size.to_bits();
                    let cache_key = (
                        self.vt_visual_generation,
                        self.terminal.content_epoch(),
                        self.cols,
                        self.rows,
                        font_bits,
                        shell.default_fg,
                        shell.terminal_bg,
                    );
                    let (layout_job, display_owned) =
                        if let Some(ref c) = self.visual_layout_cache {
                            if c.matches(
                                cache_key.0,
                                cache_key.1,
                                cache_key.2,
                                cache_key.3,
                                cache_key.4,
                                cache_key.5,
                                cache_key.6,
                            ) {
                                (
                                    c.layout_job.clone(),
                                    c.formatted.clone(),
                                )
                            } else {
                                let layout_job = self.terminal.get_layout_job(
                                    self.font_size,
                                    &shell,
                                    self.search_highlight,
                                );
                                let formatted = self.terminal.get_formatted_output();
                                self.visual_layout_cache = Some(TerminalVisualLayoutCache {
                                    vt_gen: cache_key.0,
                                    content_epoch: cache_key.1,
                                    cols: cache_key.2,
                                    rows: cache_key.3,
                                    font_bits: cache_key.4,
                                    fg: cache_key.5,
                                    bg: cache_key.6, // shell.default_fg / terminal_bg
                                    layout_job: layout_job.clone(),
                                    formatted: formatted.clone(),
                                });
                                (layout_job, formatted)
                            }
                        } else {
                            let layout_job = self.terminal.get_layout_job(
                                self.font_size,
                                &shell,
                                self.search_highlight,
                            );
                            let formatted = self.terminal.get_formatted_output();
                            self.visual_layout_cache = Some(TerminalVisualLayoutCache {
                                vt_gen: cache_key.0,
                                content_epoch: cache_key.1,
                                cols: cache_key.2,
                                rows: cache_key.3,
                                font_bits: cache_key.4,
                                fg: cache_key.5,
                                bg: cache_key.6,
                                layout_job: layout_job.clone(),
                                formatted: formatted.clone(),
                            });
                            (layout_job, formatted)
                        };

                    let prev_scroll_w = ui.spacing().scroll_bar_width;
                    let prev_inactive_fill = ui.visuals().widgets.inactive.bg_fill;
                    ui.spacing_mut().scroll_bar_width = theme.terminal_scroll_bar_width();
                    ui.visuals_mut().widgets.inactive.bg_fill =
                        theme.terminal_scroll_bar_track_fill();
                    // 内容仅为当前视口行数；历史在 VTE scrollback，须用 scroll_display 而非 ScrollArea 滑条
                    let scroll_output = egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .enable_scrolling(false)
                        .drag_to_scroll(false)
                        .scroll_bar_visibility(
                            egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden,
                        )
                        .max_height(scroll_h)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            ui.set_min_width(ui.available_width().max(1.0));
                            let vw = ui.available_width().max(1.0);
                            // 先铺与格子一致的底色，避免 PTY 行高取整后 galley 略矮露出「另一块」底色
                            let pre = ui
                                .available_rect_before_wrap()
                                .intersect(ui.clip_rect());
                            if pre.width() > 0.0 && pre.height() > 0.0 {
                                ui.painter().rect_filled(pre, 0.0, shell.terminal_bg);
                            }
                            // 与 TextEdit 同宽：Scroll 内容区已不含纵向条，勿再扣 SCROLL_VBAR_RESERVE（否则会窄一截、右侧露 extreme_bg）
                            self.sync_pty_size_with_ui(ui, egui::vec2(vw, scroll_h.max(1.0)), theme);
                            let edit_w = Self::text_width_in_scroll_viewport(vw);
                            let (cell_w, cell_h) =
                                Self::terminal_cell_metrics(ui, self.font_size, theme.text_primary());
                            // `String` + 可编辑会触发 egui 插入/IME 光标，与 VT 里「│」叠成双光标；用 `&str` 只读缓冲只保留 PTY 光标
                            let mut display_view: &str = display_owned.as_str();
                            let mut layouter = |ui: &egui::Ui, text: &str, _wrap_width: f32| {
                                let _ = text;
                                let job = layout_job.clone();
                                ui.ctx().fonts(|f| f.layout_job(job))
                            };
                            // 勿让 TextEdit 自带拖选（visuals.selection 灰底）与终端格网选区（accent 底）叠成双色
                            let prev_sel_bg = ui.visuals().selection.bg_fill;
                            let prev_sel_fg = ui.visuals().selection.stroke.color;
                            ui.visuals_mut().selection.bg_fill = egui::Color32::TRANSPARENT;
                            ui.visuals_mut().selection.stroke.color = egui::Color32::TRANSPARENT;
                            // Scroll 内短内容时 galley 高度 < 视口，底下会留一块「纯黑」；强制最小高度铺满视口
                            let response = ui.add(
                                egui::TextEdit::multiline(&mut display_view)
                                    .id_source("terminal_text_area")
                                    // egui 默认 margin (4,2)，文本不会贴齐 Scroll 内缘；终端须为 0
                                    .margin(egui::vec2(0.0, 0.0))
                                    .horizontal_align(egui::Align::LEFT)
                                    // min_size 拉高控件后默认 TOP 对齐，底下一大块「纯黑」；终端内容贴底更贴近真实 shell
                                    .vertical_align(egui::Align::BOTTOM)
                                    .font(egui::TextStyle::Monospace)
                                    // 必须与父 Scroll/Frame 同宽；过大 min 宽会把 Central 撑到盖住右侧栏
                                    .desired_width(edit_w)
                                    .min_size(egui::vec2(vw, scroll_h))
                                    .code_editor()
                                    // 查找条打开时释放 lock，否则无法聚焦搜索框输入。
                                    .lock_focus(!terminal_search_open)
                                    .interactive(true)
                                    .frame(false)
                                    .layouter(&mut layouter),
                            );
                            ui.visuals_mut().selection.bg_fill = prev_sel_bg;
                            ui.visuals_mut().selection.stroke.color = prev_sel_fg;
                            if let Some(mut te_state) =
                                egui::widgets::text_edit::TextEditState::load(ui.ctx(), response.id)
                            {
                                te_state.set_cursor_range(None);
                                te_state.store(ui.ctx(), response.id);
                            }
                            let galley = Self::layout_terminal_galley(ui, &layout_job);
                            let text_top = Self::terminal_text_top(&response, &galley);
                            if !terminal_search_open {
                                if response.clicked() {
                                    response.request_focus();
                                }
                                if self.pending_focus_terminal {
                                    response.request_focus();
                                    self.pending_focus_terminal = false;
                                }
                            }
                            self.terminal_focused = response.has_focus();

                            // 滚轮浏览 scrollback（与 alacritty 一致：Delta>0 向上翻历史）
                            if (response.hovered() || response.has_focus())
                                && ui.input(|i| !i.modifiers.ctrl && i.scroll_delta.y.abs() > 0.5)
                            {
                                let dy = ui.input(|i| i.scroll_delta.y);
                                let lines = (dy / cell_h).round().clamp(-64.0, 64.0) as i32;
                                if lines != 0 {
                                    self.terminal.scroll_display(Scroll::Delta(lines));
                                    self.visual_layout_cache = None;
                                }
                            }

                            // === 文本选择处理 ===

                            // 点击时清除选择或开始选择
                            if response.clicked() {
                                if let Some(pos) = response.interact_pointer_pos() {
                                    let (line, col) = Self::terminal_row_col_at_pointer(
                                        &galley, text_top, &response, pos,
                                    );
                                    self.selection.start_line = line;
                                    self.selection.start_col = col;
                                    self.selection.end_line = line;
                                    self.selection.end_col = col;
                                    self.selection.active = true;
                                }
                            }

                            // 拖拽更新选择范围
                            if response.dragged() {
                                if let Some(pos) = response.interact_pointer_pos() {
                                    let (line, col) = Self::terminal_row_col_at_pointer(
                                        &galley, text_top, &response, pos,
                                    );
                                    self.selection.end_line = line;
                                    self.selection.end_col = col;
                                    self.selection.active = true;
                                }
                            }

                            // 松开鼠标时复制选中内容到剪贴板
                            if response.drag_released() && !self.selection.is_empty() {
                                let text = self.get_selected_text();
                                if !text.is_empty() {
                                    if let Ok(mut clip) = Clipboard::new() {
                                        let _ = clip.set_text(text);
                                    }
                                }
                            }

                            // 块状闪烁光标（UI 层绘制，勿再用 │ 字符占位）
                            if self.terminal_focused
                                && !terminal_search_open
                                && self.selection.is_empty()
                            {
                                if let Some(cur) = self.terminal.viewport_cursor() {
                                    let t = ui.input(|i| i.time);
                                    let phase = (t
                                        / crate::terminal::style::TERMINAL_CURSOR_BLINK_PERIOD_SECS)
                                        .floor() as i64
                                        % 2;
                                    if phase == 0 {
                                        let cursor_rect = Self::terminal_block_cursor_rect(
                                            &response,
                                            &galley,
                                            text_top,
                                            cur,
                                            cell_w,
                                        );
                                        let painter =
                                            ui.painter().clone().with_layer_id(response.layer_id);
                                        painter.rect_filled(
                                            cursor_rect,
                                            0.0,
                                            theme.color_terminal_cursor_block(),
                                        );
                                    }
                                }
                                ui.ctx().request_repaint_after(
                                    std::time::Duration::from_millis(60),
                                );
                            }

                            // 绘制选择高亮
                            if !self.selection.is_empty() {
                                let painter = ui.painter().clone().with_layer_id(response.layer_id);
                                let (start_l, start_c, end_l, end_c) = self.selection.normalize();
                                let lines = display_owned.lines().collect::<Vec<_>>();
                                
                                for line_idx in start_l..=end_l {
                                    if line_idx >= lines.len() {
                                        break;
                                    }
                                    let line = lines[line_idx];
                                    let chars: Vec<char> = line.chars().collect();
                                    let line_len = chars.len();
                                    
                                    let c_start = if line_idx == start_l { start_c } else { 0 };
                                    let c_end = if line_idx == end_l { end_c } else { line_len };
                                    let c_start = c_start.min(line_len);
                                    let c_end = c_end.min(line_len);
                                    
                                    if c_start < c_end {
                                        if let Some(sel_rect) = Self::terminal_selection_rect(
                                            &response,
                                            &galley,
                                            text_top,
                                            line_idx,
                                            c_start,
                                            c_end,
                                            cell_w,
                                        ) {
                                            painter.rect_filled(
                                                sel_rect,
                                                0.0,
                                                theme.color_terminal_selection(),
                                            );
                                        }
                                    }
                                }
                            }
                            response.context_menu(|ui| {
                                crate::ui::chrome::apply_context_menu_style(ui, theme);
                                if !self.selection.is_empty() {
                                    if crate::ui::chrome::popup_menu_button(ui, theme, "复制选中")
                                        .clicked()
                                    {
                                        let text = self.get_selected_text();
                                        if !text.is_empty() {
                                            if let Ok(mut clip) = Clipboard::new() {
                                                let _ = clip.set_text(text);
                                            }
                                        }
                                        ui.close_menu();
                                    }
                                    ui.separator();
                                }
                                if crate::ui::chrome::popup_menu_button(ui, theme, "复制全部").clicked()
                                {
                                    if let Ok(mut clip) = Clipboard::new() {
                                        let _ = clip.set_text(self.terminal.get_formatted_output());
                                    }
                                    ui.close_menu();
                                }
                                if crate::ui::chrome::popup_menu_button(ui, theme, "粘贴").clicked() {
                                    if let Ok(mut clip) = Clipboard::new() {
                                        if let Ok(text) = clip.get_text() {
                                            self.paste_text(&text, ui.ctx());
                                        }
                                    }
                                    ui.close_menu();
                                }
                                ui.separator();
                                if crate::ui::chrome::popup_menu_button(ui, theme, "清屏").clicked() {
                                    self.clear_screen();
                                    ui.close_menu();
                                }
                                if crate::ui::chrome::popup_menu_button(ui, theme, "字体 +").clicked()
                                {
                                    self.set_font_size(self.font_size + 1.0);
                                    ui.close_menu();
                                }
                                if crate::ui::chrome::popup_menu_button(ui, theme, "字体 -").clicked()
                                {
                                    self.set_font_size(self.font_size - 1.0);
                                    ui.close_menu();
                                }
                            });
                        });

                    ui.spacing_mut().scroll_bar_width = prev_scroll_w;
                    ui.visuals_mut().widgets.inactive.bg_fill = prev_inactive_fill;

                    // 离开底部（display_offset>0）则停止自动跟随；滚回最新输出后恢复
                    self.auto_follow_output = self.terminal.is_scrolled_to_bottom();
                    let _ = scroll_output;

                    if let Some(ref progress) = self.transfer_progress {
                        ui.add_space(theme.spacing_sm());
                        ui.separator();
                        let dir = if self.transfer_outgoing {
                            "本机 → 远端"
                        } else {
                            "远端 → 本机"
                        };
                        ui.horizontal(|ui| {
                            crate::ui::icons::icon_label_row(
                                ui,
                                crate::ui::icons::IconId::Zmodem,
                                "ZMODEM",
                                theme.font_size_body(),
                                6.0,
                                |t| t.strong().color(theme.accent_color()),
                            );
                            ui.label(
                                egui::RichText::new(dir).color(theme.text_secondary()),
                            );
                            ui.label(egui::RichText::new("·").color(theme.text_tertiary()));
                            ui.label(
                                egui::RichText::new(&progress.0)
                                    .monospace()
                                    .color(theme.text_primary()),
                            );
                        });
                        let percent = if progress.2 > 0 {
                            (progress.1 as f32 / progress.2 as f32 * 100.0).min(100.0)
                        } else {
                            0.0
                        };
                        let detail = format!(
                            "{} / {} · {:.1}%",
                            human_readable_size(progress.1),
                            human_readable_size(progress.2.max(1)),
                            percent
                        );
                        let bar_w = Self::text_width_in_scroll_viewport(ui.available_width().max(1.0));
                        ui.add(
                            egui::ProgressBar::new(percent / 100.0)
                                .fill(theme.accent_color())
                                .desired_width(bar_w)
                                .text(egui::RichText::new(detail).color(theme.text_primary())),
                        );
                    }
                });
                ui.visuals_mut().extreme_bg_color = prev_extreme;
            });
        self.render_resend_offline_dialog(ui.ctx(), theme);
    }

    /// `viewport`：ScrollArea **内容区**（inner）的宽 × 可视区高度（与 `max_height(scroll_h)` 一致），已不含 Frame inner_margin 与纵向滚动条占位。
    fn sync_pty_size_with_ui(&mut self, ui: &egui::Ui, viewport: egui::Vec2, theme: &Theme) {
        let usable_width = Self::text_width_in_scroll_viewport(viewport.x).max(120.0);
        let usable_height = viewport.y.max(48.0);

        // 用真实字体测量单字符网格尺寸，避免 80x24 误差
        let font_id = egui::FontId::monospace(self.font_size);
        let (cell_w, cell_h) = ui.ctx().fonts(|fonts| {
            let galley =
                fonts.layout_no_wrap("W".to_string(), font_id, theme.text_primary());
            (galley.size().x.max(6.0), galley.size().y.max(12.0))
        });

        let cols = (usable_width / cell_w).floor().clamp(20.0, 512.0) as u32;
        let rows = (usable_height / cell_h).floor().clamp(5.0, 256.0) as u32;

        if cols != self.cols || rows != self.rows {
            self.resize(cols, rows);
        }
    }

    /// 非活动标签仅消费 SSH 接收队列；若有新内容写入 VTE 则返回 `true`（用于低频重绘）。
    pub fn pump_ssh_only(&mut self, theme: &Theme) -> bool {
        self.process_ssh_messages(theme)
    }

    /// FUNCTIONAL_SPEC §2.4：超长粘贴分片写入 PTY（每批 4096 字节，间隔 5ms）。
    fn flush_paste_queue(&mut self, ctx: &egui::Context) {
        const CHUNK: usize = 4096;
        const GAP: Duration = Duration::from_millis(5);

        if self.paste_pending.is_empty() {
            self.paste_next_chunk_at = None;
            return;
        }
        if !self.connected {
            self.paste_pending.clear();
            self.paste_next_chunk_at = None;
            return;
        }
        let Some(handle) = self.ssh_handle.as_ref() else {
            self.paste_pending.clear();
            self.paste_next_chunk_at = None;
            return;
        };

        let now = Instant::now();
        if let Some(t) = self.paste_next_chunk_at {
            if now < t {
                ctx.request_repaint_after(t - now);
                return;
            }
        }

        let n = CHUNK.min(self.paste_pending.len());
        let chunk: Vec<u8> = self.paste_pending.drain(..n).collect();
        if let Err(e) = handle.send_input(&chunk) {
            log::error!("PTY write (paste chunk): {}", e);
        }

        if !self.paste_pending.is_empty() {
            self.paste_next_chunk_at = Some(now + GAP);
            ctx.request_repaint_after(GAP);
        } else {
            self.paste_next_chunk_at = None;
        }
    }

    /// 处理 SSH 消息；若终端缓冲有更新则返回 `true`。
    fn process_ssh_messages(&mut self, _theme: &Theme) -> bool {
        let mut vte_dirty = false;
        if let Some(ref rx) = self.ssh_rx {
            let batch: Vec<SshMessage> = rx.try_iter().collect();
            for msg in batch {
                match msg {
                    SshMessage::Output { data, .. } => {
                        // 上传（本机 sz→远端 rz）时，PTY 上的 ZMODEM 帧只能经 SSH 泵线程到达此处；
                        // 必须旁路给 lrzsz，不得在另一线程对 Channel 再 read。
                        let text = String::from_utf8_lossy(&data);
                        let is_rz_prompt = text.contains("rz rz rz")
                            || text.contains("Awaiting rz")
                            || text.contains("rz waiting to receive")
                            || text.contains("**B0")
                            || text.contains("B0000"); // 兼容不同 rz 实现的握手串

                        // 尽早打开旁路：选文件前 `rz` 发出的 ZRQINIT 若只进 VTE，`start_send` 会永远等下一轮超时。
                        if is_rz_prompt && self.connected {
                            if let Some(path) = self.zmodem_upload_after_rz_path.take() {
                                log::info!(
                                    "rz 握手就绪，开始 ZMODEM 上传（预排队）{}",
                                    path.display()
                                );
                                self.pending_rz_upload = false;
                                self.rz_control_mode_until =
                                    Some(Instant::now() + Duration::from_secs(90));
                                self.lrzsz.begin_rz_handshake_capture();
                                self.lrzsz.feed_send_pty_output(&data);
                                if let Some(ref h) = self.ssh_handle {
                                    self.lrzsz.register_shell_pump_upload_feed(h);
                                }
                                if Self::mirror_rz_text_to_vte_enabled()
                                    && Self::pty_chunk_safe_to_mirror_vte(&data)
                                {
                                    self.terminal.feed(&data);
                                    vte_dirty = true;
                                }
                                if let Err(e) = self.start_rz_upload(path.as_path()) {
                                    self.error_message = Some(e);
                                    self.lrzsz.end_rz_handshake_capture();
                                }
                                continue;
                            }
                            if !self.pending_rz_upload {
                            log::info!("检测到 rz 命令，弹出上传文件选择");
                            self.pending_rz_upload = true;
                            self.rz_control_mode_until = Some(Instant::now() + Duration::from_secs(20));
                            self.lrzsz.begin_rz_handshake_capture();
                            // 首段触发文本必须先入队；随后注册 shell 泵同步旁路（避免仅 UI 路径一帧延迟）。
                            self.lrzsz.feed_send_pty_output(&data);
                            if let Some(ref h) = self.ssh_handle {
                                self.lrzsz.register_shell_pump_upload_feed(h);
                            }
                            if Self::mirror_rz_text_to_vte_enabled()
                                && Self::pty_chunk_safe_to_mirror_vte(&data)
                            {
                                self.terminal.feed(&data);
                                vte_dirty = true;
                            }
                            continue; // 不显示在终端
                            }
                        }

                        self.lrzsz.feed_send_pty_output(&data);
                        if self.lrzsz.is_upload_pty_capture() {
                            // 与 `rz_control_mode` 里「误判 shell 提示符」无关：二进制 ZACK 等绝不能进 VTE（否则 0x18 被当 C1）。
                            if Self::mirror_rz_text_to_vte_enabled()
                                && Self::pty_chunk_safe_to_mirror_vte(&data)
                            {
                                self.terminal.feed(&data);
                                vte_dirty = true;
                            }
                            continue;
                        }

                        let display_data: Vec<u8> = data;
                        if let Some(until) = self.rz_control_mode_until {
                            if Instant::now() <= until {
                                // 控制模式内默认吞掉输出，避免显示 **B0... / ccc|... 等握手噪音。
                                // 仅在看到 shell 提示符时退出控制模式并恢复正常显示。
                                let raw_text = String::from_utf8_lossy(&display_data);
                                if Self::contains_shell_prompt_fragment(&raw_text) {
                                    self.rz_control_mode_until = None;
                                } else {
                                    continue;
                                }
                            } else {
                                self.rz_control_mode_until = None;
                            }
                        }
                        if !display_data.is_empty() {
                            self.pending_log_output.push(display_data.clone());
                            self.terminal.feed(&display_data);
                            vte_dirty = true;
                        }
                    }
                    SshMessage::Connected { .. } => {
                        if let Some((_, host)) = &self.connection_target {
                            self.pending_connect_audit = Some((true, host.clone()));
                        }
                        self.connected = true;
                        self.connected_at = Some(Instant::now());
                        self.terminal_focused = true;
                        self.pending_focus_terminal = true;
                        self.auto_follow_output = true;
                        
                        // 启动交互式 shell
                        if let Some(ref manager) = self.ssh_manager {
                            if let Some(session_id) = self.session_id {
                                match manager.start_interactive_shell(session_id, self.cols, self.rows) {
                                    Ok(handle) => {
                                        self.ssh_handle = Some(handle);
                                        if !self.disconnected_input_buffer.is_empty() {
                                            self.resend_offline_input_dialog_open = true;
                                        }
                                    }
                                    Err(e) => {
                                        self.error_message = Some(format_ssh_connect_error(&format!(
                                            "Failed to start shell: {}",
                                            e
                                        )));
                                    }
                                }
                            }
                        }
                    }
                    SshMessage::Error { error, .. } => {
                        let msg = format_ssh_connect_error(&error);
                        if let Some((_, host)) = &self.connection_target {
                            self.pending_connect_audit = Some((false, host.clone()));
                        }
                        self.error_message = Some(msg.clone());
                        self.connected_at = None;
                        self.auto_follow_output = true;
                    }
                    SshMessage::Disconnected { .. } => {
                        if self.local_disconnect_intent {
                            self.local_disconnect_intent = false;
                        } else {
                            self.unexpected_disconnect_notified = true;
                        }
                        self.connected = false;
                        self.terminal_focused = false;
                        self.connected_at = None;
                        self.auto_follow_output = true;
                        self.resend_offline_input_dialog_open = false;
                    }
                    SshMessage::UserCommand { command, .. } => {
                        *self.command_usage.entry(command).or_insert(0) += 1;
                    }
                }
            }
        }
        vte_dirty
    }

    pub fn command_usage_snapshot(&self) -> Vec<(String, u64)> {
        self.command_usage
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    /// 处理文件传输事件
    fn process_transfer_events(&mut self, theme: &Theme, ctx: &egui::Context) {
        while let Some(event) = self.lrzsz.try_recv_event() {
            match event {
                TransferEvent::FileStart {
                    filename,
                    size,
                    outgoing,
                } => {
                    self.transfer_outgoing = outgoing;
                    self.transfer_progress = Some((filename.clone(), 0, size));
                    ctx.request_repaint();
                    // 开始状态由底部进度条展示，不再写入 VTE，避免插在 rz 行与远端回显之间
                }
                TransferEvent::FileProgress { received, total, .. } => {
                    if let Some(ref mut progress) = self.transfer_progress {
                        *progress = (progress.0.clone(), received, total);
                    }
                    ctx.request_repaint();
                }
                TransferEvent::FileComplete { filename, path } => {
                    self.transfer_progress = None;
                    let title = if self.transfer_outgoing {
                        "上传完成"
                    } else {
                        "接收完成"
                    };
                    self.transfer_outgoing = false;
                    // 完成后多一空行，与后续 shell 提示符/命令拉开
                    self.feed_user_success_line(theme, &format!(
                        "OK {}：{} -> {}",
                        title,
                        filename,
                        path.display()
                    ));
                }
                TransferEvent::FileError { filename, error } => {
                    if let Some(ref h) = self.ssh_handle {
                        self.lrzsz.unregister_shell_pump_upload_feed(h);
                    }
                    self.transfer_progress = None;
                    self.transfer_outgoing = false;
                    self.feed_user_error_line(theme, &format!("传输失败 {}: {}", filename, error));
                    self.flush_ssh_pty_size_after_transfer();
                }
                TransferEvent::TransferComplete => {
                    if let Some(ref h) = self.ssh_handle {
                        self.lrzsz.unregister_shell_pump_upload_feed(h);
                    }
                    // 传输完成，恢复终端交互状态
                    self.auto_follow_output = true;
                    self.rz_control_mode_until = None;
                    self.transfer_outgoing = false;
                    self.terminal.feed(b"\r\n");
                    // 上传期间可能暂缓了 resize_pty，此处与远端对齐当前 UI 网格
                    self.flush_ssh_pty_size_after_transfer();
                }
            }
        }
    }

    /// 断线重连时由宿主在 `disconnect` / `connect` 之间保留/恢复（`connect()` 会清空缓存）。
    pub fn offline_input_snapshot(&self) -> (Vec<u8>, bool) {
        (
            self.disconnected_input_buffer.clone(),
            self.buffer_input_while_disconnected,
        )
    }

    pub fn restore_offline_input_snapshot(&mut self, buf: Vec<u8>, flag: bool) {
        self.disconnected_input_buffer = buf;
        self.buffer_input_while_disconnected = flag;
    }

    fn append_offline_bytes(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let cap = Self::OFFLINE_INPUT_CAP;
        if self.disconnected_input_buffer.len() >= cap {
            return;
        }
        let take = (cap - self.disconnected_input_buffer.len()).min(data.len());
        self.disconnected_input_buffer.extend_from_slice(&data[..take]);
    }

    /// 断线保留画面时：把按键写入本地缓冲（与 PTY 路径相同的字节序列，便于重发）。
    fn capture_inline_input_disconnected(&mut self, ui: &egui::Ui) {
        if self.resend_offline_input_dialog_open {
            return;
        }
        let mut pending_paste: Option<String> = None;

        ui.input_mut(|i| {
            let tab_plain = i.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
            let tab_shift = i.consume_key(
                egui::Modifiers {
                    shift: true,
                    ..Default::default()
                },
                egui::Key::Tab,
            );
            if tab_plain || tab_shift {
                self.append_offline_bytes(b"\t");
            }
            let up = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
            let down = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
            let left = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft);
            let right = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight);
            let home = i.consume_key(egui::Modifiers::NONE, egui::Key::Home);
            let end = i.consume_key(egui::Modifiers::NONE, egui::Key::End);
            let page_up = i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp);
            let page_down = i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown);
            if up {
                self.append_offline_bytes(b"\x1b[A");
            }
            if down {
                self.append_offline_bytes(b"\x1b[B");
            }
            if right {
                self.append_offline_bytes(b"\x1b[C");
            }
            if left {
                self.append_offline_bytes(b"\x1b[D");
            }
            if home {
                self.append_offline_bytes(b"\x1b[H");
            }
            if end {
                self.append_offline_bytes(b"\x1b[F");
            }
            if page_up {
                self.append_offline_bytes(b"\x1b[5~");
            }
            if page_down {
                self.append_offline_bytes(b"\x1b[6~");
            }

            let mut backspace_key = false;
            let mut delete_key = false;
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    ..
                } = event
                {
                    if *key == egui::Key::Backspace {
                        backspace_key = true;
                    }
                    if *key == egui::Key::Delete {
                        delete_key = true;
                    }
                }
            }

            for event in &i.events {
                match event {
                    egui::Event::Text(text) => {
                        if text == "\n" || text == "\r" {
                            continue;
                        }
                        if i.modifiers.command || i.modifiers.ctrl {
                            continue;
                        }
                        if text == "\t" {
                            continue;
                        }
                        if text == "\u{7f}" {
                            if backspace_key || delete_key {
                                continue;
                            }
                            #[cfg(target_os = "macos")]
                            {
                                self.append_offline_bytes(b"\x1b[3~");
                                continue;
                            }
                            #[cfg(not(target_os = "macos"))]
                            {
                                self.append_offline_bytes(&[0x7f]);
                                continue;
                            }
                        }
                        self.append_offline_bytes(text.as_bytes());
                    }
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        if modifiers.command {
                            // 仅拦截 ⌘V→离线缓冲；⌘C/⌘A 等留给侧栏/弹窗内 TextEdit
                            if *key == egui::Key::V {
                                if let Ok(mut clip) = Clipboard::new() {
                                    if let Ok(text) = clip.get_text() {
                                        pending_paste = Some(text);
                                    }
                                }
                            }
                        } else {
                        match key {
                            egui::Key::V if modifiers.ctrl && modifiers.shift => {
                                if let Ok(mut clip) = Clipboard::new() {
                                    if let Ok(text) = clip.get_text() {
                                        pending_paste = Some(text);
                                    }
                                }
                            }
                            egui::Key::Enter => {
                                self.append_offline_bytes(b"\r");
                            }
                            egui::Key::Backspace => {
                                self.append_offline_bytes(&[0x7f]);
                            }
                            egui::Key::Delete => {
                                self.append_offline_bytes(b"\x1b[3~");
                            }
                            egui::Key::Tab => {}
                            _ => {}
                        }
                        }
                    }
                    _ => {}
                }
            }
        });
        if let Some(text) = pending_paste {
            let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            self.append_offline_bytes(normalized.as_bytes());
        }
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }

    fn render_resend_offline_dialog(&mut self, ctx: &egui::Context, theme: &Theme) {
        if !self.resend_offline_input_dialog_open {
            return;
        }
        let n = self.disconnected_input_buffer.len();
        if n == 0 {
            self.resend_offline_input_dialog_open = false;
            return;
        }
        let preview = String::from_utf8_lossy(&self.disconnected_input_buffer[..n.min(200)]);
        let preview_esc = preview.replace('\r', "\\r").replace('\n', "\\n");

        let mut open = true;
        let mut close_via_header = false;
        crate::ui::chrome::modal_window("resend_offline_input", theme)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .resizable(false)
            .show(ctx, |ui| {
                crate::ui::chrome::modal_content_frame(theme).show(ui, |ui| {
                if crate::ui::chrome::modal_header(
                    ui,
                    theme,
                    "断线期间暂存的输入",
                    crate::ui::chrome::modal_title_font_size(theme),
                ) {
                    close_via_header = true;
                }
                ui.label(
                    egui::RichText::new(format!("共 {} 字节，是否发送到当前远程 shell？", n))
                        .size(theme.font_size_medium())
                        .color(theme.text_secondary()),
                );
                if !preview_esc.is_empty() {
                    ui.add_space(theme.spacing_sm());
                    ui.label(
                        egui::RichText::new(format!("预览：{}", preview_esc))
                            .monospace()
                            .size(theme.font_size_panel_title())
                            .color(theme.text_tertiary()),
                    );
                }
                ui.add_space(theme.spacing_list_item_x());
                ui.horizontal(|ui| {
                    if crate::ui::chrome::modal_primary_button(ui, theme, "发送到远端").clicked() {
                        if let Some(handle) = self.ssh_handle.clone() {
                            for chunk in self.disconnected_input_buffer.chunks(4096) {
                                let _ = handle.send_input(chunk);
                            }
                        }
                        self.disconnected_input_buffer.clear();
                        self.buffer_input_while_disconnected = false;
                        self.resend_offline_input_dialog_open = false;
                    }
                    if crate::ui::chrome::modal_secondary_button(ui, theme, "丢弃缓存").clicked() {
                        self.disconnected_input_buffer.clear();
                        self.buffer_input_while_disconnected = false;
                        self.resend_offline_input_dialog_open = false;
                    }
                });
                });
            });
        if !open || close_via_header {
            self.resend_offline_input_dialog_open = false;
            if close_via_header {
                self.disconnected_input_buffer.clear();
                self.buffer_input_while_disconnected = false;
            }
        }
    }

    /// 将键盘事件直接写入 PTY，由远端 shell 回显，避免「本地预览 + 回显」叠字（如 lsls）
    fn capture_inline_input(&mut self, ui: &egui::Ui) {
        if self.resend_offline_input_dialog_open {
            return;
        }
        if self.buffer_input_while_disconnected && !self.connected {
            if self.terminal_focused {
                self.capture_inline_input_disconnected(ui);
            }
            return;
        }
        if !self.connected {
            return;
        }
        let Some(handle) = self.ssh_handle.clone() else {
            return;
        };
        if !self.terminal_focused {
            return;
        }

        let mut pending_paste: Option<String> = None;

        ui.input_mut(|i| {
            // 强制拦截 Tab 焦点遍历，把 Tab/Shift+Tab 交给终端
            let tab_plain = i.consume_key(egui::Modifiers::NONE, egui::Key::Tab);
            let tab_shift = i.consume_key(
                egui::Modifiers {
                    shift: true,
                    ..Default::default()
                },
                egui::Key::Tab,
            );
            if tab_plain || tab_shift {
                let _ = handle.send_input(b"\t");
            }
            // 历史命令 / 光标移动等：方向键与常见导航键映射为 ANSI 序列
            let up = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp);
            let down = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown);
            let left = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft);
            let right = i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight);
            let home = i.consume_key(egui::Modifiers::NONE, egui::Key::Home);
            let end = i.consume_key(egui::Modifiers::NONE, egui::Key::End);
            let page_up = i.consume_key(egui::Modifiers::NONE, egui::Key::PageUp);
            let page_down = i.consume_key(egui::Modifiers::NONE, egui::Key::PageDown);
            if up {
                let _ = handle.send_input(b"\x1b[A");
            }
            if down {
                let _ = handle.send_input(b"\x1b[B");
            }
            if right {
                let _ = handle.send_input(b"\x1b[C");
            }
            if left {
                let _ = handle.send_input(b"\x1b[D");
            }
            if home {
                let _ = handle.send_input(b"\x1b[H");
            }
            if end {
                let _ = handle.send_input(b"\x1b[F");
            }
            if page_up {
                let _ = handle.send_input(b"\x1b[5~");
            }
            if page_down {
                let _ = handle.send_input(b"\x1b[6~");
            }
            // 同一帧内可能既有 Key 又有 Text（如 Delete / 退格），避免重复或错发
            let mut backspace_key = false;
            let mut delete_key = false;
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    ..
                } = event
                {
                    if *key == egui::Key::Backspace {
                        backspace_key = true;
                    }
                    if *key == egui::Key::Delete {
                        delete_key = true;
                    }
                }
            }

            for event in &i.events {
                match event {
                    egui::Event::Text(text) => {
                        // Enter 由 Key::Enter 统一发 \r，避免与 Text 里的 \n 重复
                        if text == "\n" || text == "\r" {
                            continue;
                        }
                        if i.modifiers.command || i.modifiers.ctrl {
                            continue;
                        }
                        // Tab 已在 consume_key 阶段处理，避免重复
                        if text == "\t" {
                            continue;
                        }
                        // U+007F：常与 Forward Delete 或退格绑定；已由 Key 处理则不再发
                        if text == "\u{7f}" {
                            if backspace_key || delete_key {
                                continue;
                            }
                            // macOS 上 Fn+Delete 等常只来 Text(DEL)，应发 xterm「向前删」而非裸 0x7f（易被当成怪字符/像空格）
                            #[cfg(target_os = "macos")]
                            {
                                if let Err(e) = handle.send_input(b"\x1b[3~") {
                                    log::error!("PTY write (delete seq): {}", e);
                                }
                                continue;
                            }
                            #[cfg(not(target_os = "macos"))]
                            {
                                if let Err(e) = handle.send_input(&[0x7f]) {
                                    log::error!("PTY write (del): {}", e);
                                }
                                continue;
                            }
                        }
                        self.track_typed_text(text);
                        if let Err(e) = handle.send_input(text.as_bytes()) {
                            log::error!("PTY write (text): {}", e);
                        }
                    }
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        if modifiers.command {
                            // 仅 ⌘V→PTY；⌘C/⌘A 等留给表单/侧栏 TextEdit（勿 continue 整段 command）
                            if *key == egui::Key::V {
                                if let Ok(mut clip) = Clipboard::new() {
                                    if let Ok(text) = clip.get_text() {
                                        pending_paste = Some(text);
                                    }
                                }
                            }
                        } else {
                        match key {
                            egui::Key::V if modifiers.ctrl && modifiers.shift => {
                                if let Ok(mut clip) = Clipboard::new() {
                                    if let Ok(text) = clip.get_text() {
                                        pending_paste = Some(text);
                                    }
                                }
                            }
                            egui::Key::C if modifiers.ctrl && modifiers.shift => {
                                if let Ok(mut clip) = Clipboard::new() {
                                    let _ = clip.set_text(self.terminal.get_formatted_output());
                                }
                            }
                            egui::Key::Enter => {
                                self.commit_typed_line_on_enter();
                                if let Err(e) = handle.send_input(b"\r") {
                                    log::error!("PTY write (enter): {}", e);
                                }
                            }
                            egui::Key::Backspace => {
                                self.track_backspace();
                                if let Err(e) = handle.send_input(&[0x7f]) {
                                    log::error!("PTY write (bs): {}", e);
                                }
                            }
                            // xterm / xterm-256color：Forward Delete 为 CSI 3 ~
                            egui::Key::Delete => {
                                if let Err(e) = handle.send_input(b"\x1b[3~") {
                                    log::error!("PTY write (delete): {}", e);
                                }
                            }
                            egui::Key::Tab => {
                                // Tab 已在 consume_key 阶段统一发送
                            }
                            egui::Key::C if modifiers.ctrl && !modifiers.shift => {
                                if self.lrzsz.is_active() {
                                    self.lrzsz.cancel_active_transfer();
                                }
                                let _ = handle.send_input(&[0x03]);
                            }
                            egui::Key::D if modifiers.ctrl => {
                                let _ = handle.send_input(&[0x04]);
                            }
                            _ => {}
                        }
                        }
                    }
                    _ => {}
                }
            }
        });
        if let Some(text) = pending_paste {
            self.paste_text(&text, ui.ctx());
        }
    }

    /// 粘贴或执行一整段命令：只写 PTY，不把内容再写入本地 buffer（回显由远端负责）
    pub fn send_command(&mut self, command: &str) {
        if !self.connected {
            return;
        }
        self.typed_line_buffer.clear();
        let normalized = command.replace("\r\n", "\n").replace('\r', "\n");
        let lines: Vec<&str> = normalized.lines().collect();
        for line in &lines {
            self.enqueue_log_command(line);
        }
        let Some(handle) = self.ssh_handle.as_ref() else {
            return;
        };
        for line in lines {
            let mut payload = line.to_string();
            payload.push('\r');
            if let Err(e) = handle.send_input(payload.as_bytes()) {
                log::error!("PTY write (command): {}", e);
            }
        }
    }

    /// 粘贴文本到终端：原样发到 PTY，不自动补回车；超长内容分片发送（FUNCTIONAL_SPEC §2.4）。
    fn paste_text(&mut self, text: &str, ctx: &egui::Context) {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if !self.connected {
            if self.buffer_input_while_disconnected {
                self.append_offline_bytes(normalized.as_bytes());
                ctx.request_repaint_after(Duration::from_millis(50));
            }
            return;
        }
        let Some(handle) = self.ssh_handle.as_ref() else {
            return;
        };
        const LONG_PASTE: usize = 10 * 1024;
        if normalized.len() <= LONG_PASTE {
            if let Err(e) = handle.send_input(normalized.as_bytes()) {
                log::error!("PTY write (paste): {}", e);
            }
            return;
        }
        self.paste_pending.extend_from_slice(normalized.as_bytes());
        self.flush_paste_queue(ctx);
    }

    pub fn clear_screen(&mut self) {
        // 清空滚动历史，保留当前屏幕内容（类似 tmux clear-history）
        // 同时把光标移到屏幕顶部，方便继续输入
        self.terminal.clear_history();
        self.terminal.feed(b"\x1b[H");  // 光标移到左上角
        self.vt_visual_generation = self.vt_visual_generation.wrapping_add(1);
        self.visual_layout_cache = None;
        self.selection.clear();
    }

    /// 菜单「复制」：优先选区，否则复制当前屏格式化输出。
    pub(crate) fn menu_copy_to_clipboard(&self) -> bool {
        let text = self.get_selected_text();
        let text = if text.is_empty() {
            self.terminal.get_formatted_output()
        } else {
            text
        };
        if text.is_empty() {
            return false;
        }
        Clipboard::new()
            .ok()
            .and_then(|mut c| c.set_text(text).ok())
            .is_some()
    }

    /// 菜单「粘贴」：从系统剪贴板粘贴到 PTY。
    pub(crate) fn menu_paste_from_clipboard(&mut self, ctx: &egui::Context) {
        if let Ok(mut clip) = Clipboard::new() {
            if let Ok(text) = clip.get_text() {
                self.paste_text(&text, ctx);
            }
        }
    }

    /// 菜单「全选」：选中当前屏全部内容。
    pub(crate) fn menu_select_all(&mut self) {
        let rows = self.rows.max(1) as usize;
        let cols = self.cols.max(1) as usize;
        self.selection.start_line = 0;
        self.selection.start_col = 0;
        self.selection.end_line = rows.saturating_sub(1);
        self.selection.end_col = cols.saturating_sub(1);
        self.selection.active = true;
    }

    /// 获取选中的文本
    fn get_selected_text(&self) -> String {
        if self.selection.is_empty() {
            return String::new();
        }
        let text = self.terminal.get_formatted_output();
        let lines: Vec<&str> = text.lines().collect();
        let (start_l, start_c, end_l, end_c) = self.selection.normalize();
        
        let mut result = String::new();
        for line_idx in start_l..=end_l {
            if line_idx >= lines.len() {
                break;
            }
            let line = lines[line_idx];
            let chars: Vec<char> = line.chars().collect();
            let line_len = chars.len();
            
            let c_start = if line_idx == start_l { start_c } else { 0 };
            let c_end = if line_idx == end_l { end_c } else { line_len };
            let c_start = c_start.min(line_len);
            let c_end = c_end.min(line_len);
            
            if c_start < c_end {
                let selected: String = chars[c_start..c_end].iter().collect();
                result.push_str(&selected);
            }
            if line_idx < end_l {
                result.push('\n');
            }
        }
        result
    }

    pub fn send_ctrl_c(&self) -> Result<(), String> {
        if !self.connected {
            return Err("当前未连接".to_string());
        }
        let Some(handle) = self.ssh_handle.as_ref() else {
            return Err("PTY 未就绪".to_string());
        };
        if self.lrzsz.is_active() {
            self.lrzsz.cancel_active_transfer();
        }
        handle.send_input(&[0x03])
    }

    pub fn clear_rz_control_mode(&mut self) {
        self.rz_control_mode_until = None;
    }

    /// 用户在文件选择器中取消 rz 上传后，强制恢复交互态（含 Ctrl+C 终止远端 rz）
    pub fn cancel_rz_upload_selection(&mut self) {
        self.pending_rz_upload = false;
        self.end_rz_handshake_capture();
        self.clear_rz_control_mode();
        self.transfer_progress = None;
        self.transfer_outgoing = false;
        self.auto_follow_output = true;
        // 文件对话框关闭后主动抢回终端焦点，避免「取消后不能输入」
        self.pending_focus_terminal = true;
        self.terminal_focused = true;
        if self.lrzsz.is_active() {
            self.lrzsz.cancel_active_transfer();
        }
        if let Some(handle) = self.ssh_handle.as_ref() {
            let _ = handle.send_input(&[0x03]);
        }
    }

    /// 用户取消 rz 文件选择时关闭 PTY 旁路（与 `app` 里取消 pick 配套）
    pub fn end_rz_handshake_capture(&mut self) {
        if let Some(ref h) = self.ssh_handle {
            self.lrzsz.unregister_shell_pump_upload_feed(h);
        }
        self.lrzsz.end_rz_handshake_capture();
    }

    /// 基于当前交互式 PTY 通道执行 rz 对端对应的 ZMODEM 发送
    pub fn start_rz_upload(&mut self, path: &Path) -> Result<(), String> {
        if !self.connected {
            return Err("当前未连接".to_string());
        }
        let path_str = path
            .to_str()
            .ok_or_else(|| "文件路径包含非法字符".to_string())?;
        let handle = self
            .ssh_handle
            .as_ref()
            .ok_or_else(|| "PTY 未就绪".to_string())?;
        let pump_tx = handle.shell_pump_tx();
        self.rz_control_mode_until = Some(Instant::now() + Duration::from_secs(90));
        log::info!(
            "start_rz_upload: path={} session_id={:?}",
            path.display(),
            handle.session_id
        );
        match self.lrzsz.start_send(path_str, pump_tx) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.lrzsz.end_rz_handshake_capture();
                Err(e)
            }
        }
    }

    /// 轮询后台上传结果（有结果时返回并清空任务）
    pub fn poll_upload_result(&mut self) -> Option<Result<String, String>> {
        let Some(rx) = self.upload_result_rx.as_ref() else {
            return None;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.upload_result_rx = None;
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.upload_result_rx = None;
                Some(Err("上传任务异常中断".to_string()))
            }
        }
    }

    pub fn start_upload(&mut self, path: &Path) -> Result<(), String> {
        if self.upload_result_rx.is_some() {
            return Err("已有上传任务正在进行中".to_string());
        }

        let session_id = self.session_id
            .ok_or_else(|| "没有 SSH 会话".to_string())?;
        let session = self.ssh_manager.as_ref()
            .and_then(|m| m.get_session(session_id))
            .ok_or_else(|| "获取 SSH 会话失败".to_string())?;

        let file_name = path.file_name()
            .ok_or_else(|| "无效的文件路径".to_string())?
            .to_string_lossy()
            .to_string();

        let path_buf = path.to_path_buf();
        let remote_path = format!("./{}", file_name);
        let (tx, rx) = mpsc::channel::<Result<String, String>>();
        self.upload_result_rx = Some(rx);

        thread::spawn(move || {
            let result = (|| -> Result<String, String> {
                let data = std::fs::read(&path_buf)
                    .map_err(|e| format!("读取文件失败：{}", e))?;
                let total_size = data.len();
                log::info!(
                    "开始 SSH SCP 直传上传: {} ({} bytes)",
                    path_buf.display(),
                    total_size
                );
                // 用 SCP 直传替代 cat >，避免 wait_close 卡住导致无回执
                let mut scp = session
                    .scp_send(Path::new(&remote_path), 0o644, total_size as u64, None)
                    .map_err(|e| format!("创建 SCP 通道失败：{}", e))?;
                use std::io::Write;
                scp.write_all(&data)
                    .map_err(|e| format!("SCP 写入失败：{}", e))?;
                scp.send_eof()
                    .map_err(|e| format!("SCP 发送 EOF 失败：{}", e))?;
                scp.wait_eof()
                    .map_err(|e| format!("SCP 等待 EOF 失败：{}", e))?;
                scp.close()
                    .map_err(|e| format!("SCP 关闭失败：{}", e))?;
                scp.wait_close()
                    .map_err(|e| format!("SCP 等待关闭失败：{}", e))?;
                Ok(remote_path.clone())
            })();

            let _ = tx.send(result);
        });

        Ok(())
    }

    pub fn start_upload_to_remote(&mut self, local_path: &Path, remote_path: &str) -> Result<(), String> {
        let session_id = self.session_id
            .ok_or_else(|| "没有 SSH 会话".to_string())?;
        let session = self.ssh_manager.as_ref()
            .and_then(|m| m.get_session(session_id))
            .ok_or_else(|| "获取 SSH 会话失败".to_string())?;

        let data = std::fs::read(local_path)
            .map_err(|e| format!("读取本地文件失败：{}", e))?;
        let sftp = Self::retry_sftp_op(|| session.sftp(), "创建 SFTP 通道失败")?;
        let mut remote = Self::retry_sftp_op(
            || sftp.create(Path::new(remote_path)),
            "创建远端文件失败",
        )?;

        use std::io::Write;
        let mut written = 0usize;
        while written < data.len() {
            match remote.write(&data[written..]) {
                Ok(0) => return Err("SFTP 上传中断：远端未继续接收数据".to_string()),
                Ok(n) => written += n,
                Err(e) => {
                    if Self::is_would_block_like(&e) {
                        thread::sleep(Duration::from_millis(8));
                        continue;
                    }
                    return Err(format!("SFTP 上传写入失败：{}", e));
                }
            }
        }
        Self::retry_sftp_op(|| remote.flush(), "SFTP 上传 flush 失败")?;
        Ok(())
    }

    /// SFTP 等：`SshManager` 克隆后与后台线程配合使用（避免阻塞 UI）。
    pub fn sftp_session_for_ops(&self) -> Option<(SshSessionId, SshManager)> {
        if !self.connected {
            return None;
        }
        let id = self.session_id?;
        let mgr = self.ssh_manager.as_ref()?.clone();
        Some((id, mgr))
    }

    pub fn download_dir(&self) -> &str {
        &self.download_dir
    }

    pub fn set_font_size(&mut self, size: f32) {
        let n = size.clamp(10.0, 24.0);
        if (n - self.font_size).abs() > f32::EPSILON {
            self.visual_layout_cache = None;
        }
        self.font_size = n;
    }

    pub fn font_size(&self) -> f32 {
        self.font_size
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// 终端网格是否持有键盘焦点（用于全局快捷键与 Delete 等不与 PTY 抢键）
    pub fn is_terminal_focused(&self) -> bool {
        self.terminal_focused
    }

    pub fn ssh_session_handle(&self) -> Option<SshSessionHandle> {
        self.ssh_handle.clone()
    }

    /// 克隆 SSH 管理器，供监控面板 exec 采集等与 PTY 并行的操作使用。
    pub fn ssh_manager_clone(&self) -> Option<SshManager> {
        self.ssh_manager.as_ref().cloned()
    }

    /// 是否处于连接建立中（认证/启动 shell 阶段）
    pub fn is_connecting(&self) -> bool {
        !self.connected && self.error_message.is_none() && self.ssh_manager.is_some()
    }

    /// 底栏连接状态（不写入终端 scrollback）。
    pub fn connection_status_for_bar(&self, theme: &Theme) -> Option<ConnectionBarStatus> {
        if self.ssh_manager.is_none() && self.connection_target.is_none() {
            return None;
        }
        let host_line = self.connection_server_text();
        let (state_line, state_color) = if let Some(err) = self.error_message.as_deref() {
            (
                truncate_connection_status(err, 36),
                theme.red_color(),
            )
        } else if self.connected {
            (
                self.connection_duration_text(),
                theme.green_color(),
            )
        } else if self.is_connecting() {
            ("正在连接…".to_string(), theme.accent_color())
        } else {
            ("已断开".to_string(), theme.amber_color())
        };
        Some(ConnectionBarStatus {
            host_line,
            state_line,
            state_color,
        })
    }

    /// README §2.6 连接时长格式
    pub fn connection_duration_text(&self) -> String {
        let Some(connected_at) = self.connected_at else {
            return "未连接".to_string();
        };
        let elapsed = connected_at.elapsed().as_secs();
        let mins = elapsed / 60;
        let hours = elapsed / 3600;
        let days = hours / 24;
        if days > 0 {
            format!("已连接 {}d {}h", days, hours % 24)
        } else if hours > 0 {
            format!("已连接 {}h {}m", hours, mins % 60)
        } else if mins > 0 {
            format!("已连接 {} 分钟", mins)
        } else {
            "刚连接".to_string()
        }
    }

    /// 连接错误摘要（供主窗口状态栏展示）
    pub fn connection_error_text(&self) -> Option<&str> {
        self.error_message.as_deref()
    }

    pub fn connection_server_text(&self) -> String {
        if let Some((username, host)) = &self.connection_target {
            return format!("{}@{}", username, host);
        }
        "未选择会话".to_string()
    }

    /// 将当前 `cols`/`rows` 同步到 SSH PTY（上传中跳过 `resize_pty` 后由传输结束路径调用）
    fn flush_ssh_pty_size_after_transfer(&mut self) {
        if let Some(ref handle) = self.ssh_handle {
            if let Err(e) = handle.resize_pty(self.cols, self.rows) {
                log::error!("传输结束后 resize_pty 失败: {}", e);
            } else {
                log::debug!(
                    "传输结束后已同步 resize_pty {}x{}",
                    self.cols,
                    self.rows
                );
            }
        }
    }

    /// 调整终端尺寸
    pub fn resize(&mut self, cols: u32, rows: u32) {
        self.cols = cols;
        self.rows = rows;
        self.terminal.resize(cols as usize, rows as usize);

        if let Some(ref handle) = self.ssh_handle {
            // 文件选择关闭等会导致 UI 行数突变并立刻 resize_pty；与 ZMODEM 首轮帧并发时，
            // 远端 `rz` 易在握手中被 SIGWINCH 打乱，表现为约 10s 无入站再续（见用户日志）。
            if self.lrzsz.is_upload_pty_capture() {
                log::debug!(
                    "暂缓 resize_pty（ZMODEM→rz 上传中），UI 已切到 {}x{}，待传输结束后再同步",
                    cols,
                    rows
                );
                return;
            }
            if let Err(e) = handle.resize_pty(cols, rows) {
                log::error!("Failed to resize PTY: {}", e);
            }
        }
    }

    /// 断开连接（关闭 SSH 并清空本地终端网格，用于移除标签等场景）
    pub fn disconnect(&mut self) {
        self.local_disconnect_intent = true;
        if let Some(ref h) = self.ssh_handle {
            self.lrzsz.unregister_shell_pump_upload_feed(h);
        }
        self.pending_rz_upload = false;
        self.end_rz_handshake_capture();
        self.clear_rz_control_mode();
        self.connected = false;
        self.ssh_handle = None;
        self.ssh_manager = None;
        self.ssh_rx = None;
        self.session_id = None;
        self.terminal = VtTerminal::new(self.cols as usize, self.rows as usize);
        self.vt_visual_generation = self.vt_visual_generation.wrapping_add(1);
        self.visual_layout_cache = None;
        self.error_message = None;
        self.transfer_progress = None;
        self.transfer_outgoing = false;
        self.pending_focus_terminal = false;
        self.connected_at = None;
        self.terminal_focused = false;
        self.paste_pending.clear();
        self.paste_next_chunk_at = None;
        self.buffer_input_while_disconnected = false;
        self.disconnected_input_buffer.clear();
        self.resend_offline_input_dialog_open = false;
    }

    /// 仅断开 SSH，保留当前屏幕与滚动缓冲（FUNCTIONAL_SPEC §1.3.5：Tab 保留、输出冻结、不可再输入）
    pub fn disconnect_ssh_keep_buffer(&mut self) {
        self.local_disconnect_intent = true;
        if let Some(ref h) = self.ssh_handle {
            self.lrzsz.unregister_shell_pump_upload_feed(h);
        }
        self.pending_rz_upload = false;
        self.end_rz_handshake_capture();
        self.clear_rz_control_mode();
        self.connected = false;
        self.ssh_handle = None;
        self.ssh_manager = None;
        self.ssh_rx = None;
        self.session_id = None;
        self.error_message = None;
        self.transfer_progress = None;
        self.transfer_outgoing = false;
        self.pending_focus_terminal = false;
        self.connected_at = None;
        self.terminal_focused = false;
        self.paste_pending.clear();
        self.paste_next_chunk_at = None;
        self.buffer_input_while_disconnected = true;
        self.resend_offline_input_dialog_open = false;
        self.terminal.feed(
            "\r\n\x1b[33m[已断开 SSH；键盘输入将暂存，重连后可选择是否发送到远端]\x1b[0m\r\n"
                .as_bytes(),
        );
    }

    /// 插入命令片段（自动添加回车）
    pub fn insert_fragment(&mut self, command: &str) -> Result<(), String> {
        if !self.connected {
            return Err("终端未连接".to_string());
        }
        let Some(handle) = self.ssh_handle.as_ref() else {
            return Err("连接句柄不可用".to_string());
        };
        let input = format!("{}\r", command);
        handle
            .send_input(input.as_bytes())
            .map_err(|e| format!("发送失败: {}", e))
    }

    /// 取走拖入终端区域的上传路径（宿主每帧至多处理一次）。
    pub fn take_drop_upload_paths(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.pending_drop_upload_paths)
    }

    /// 非用户主动断开时为 `true` 一次（供自动重连等）；读取后清除。
    pub fn take_unexpected_disconnect_notified(&mut self) -> bool {
        std::mem::take(&mut self.unexpected_disconnect_notified)
    }

    /// 取走待上报的连接结果（`success`, `host`）。
    pub fn take_connect_audit(&mut self) -> Option<(bool, String)> {
        self.pending_connect_audit.take()
    }

    /// 大文件走 ZMODEM：向 PTY 发送 `rz -y` 并在握手就绪后用 `path` 启动上传（FUNCTIONAL_SPEC §4.3）。
    pub fn queue_zmodem_upload_after_rz(&mut self, path: PathBuf) {
        self.zmodem_upload_after_rz_path = Some(path);
        self.pending_rz_upload = false;
        if self.connected {
            if let Some(ref h) = self.ssh_handle {
                let _ = h.send_input(b"\nrz -y\r");
            }
        }
    }

    /// 在完整终端缓冲（含 scrollback）中搜索。
    pub fn search_all(&self, query: &str, ignore_case: bool) -> Vec<crate::terminal::SearchHit> {
        self.terminal.search_all(query, ignore_case)
    }

    /// 滚到命中行并返回视口内高亮坐标（1-based）。
    pub fn reveal_search_hit(
        &mut self,
        hit: crate::terminal::SearchHit,
    ) -> Option<(usize, usize)> {
        self.visual_layout_cache = None;
        self.terminal.reveal_search_hit(hit)
    }
}

/// 人类可读的文件大小格式
fn human_readable_size(size: u64) -> String {
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
        format!("{} B", size)
    }
}

impl Default for TerminalView {
    fn default() -> Self {
        Self::new()
    }
}
