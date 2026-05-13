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
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use crate::ssh::{SshManager, SshConfig, SshMessage, SshSessionHandle, SshSessionId, LrzszTransfer, TransferEvent, format_ssh_connect_error};
use crate::terminal::Terminal as VtTerminal;
use crate::ui::theme::Theme;

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
}

impl TerminalView {
    /// 断线缓存输入上限（字节）
    const OFFLINE_INPUT_CAP: usize = 64 * 1024;

    const SFTP_RETRY_ATTEMPTS: usize = 160;
    const SFTP_RETRY_SLEEP_MS: u64 = 8;
    /// 终端区与外边：左/上/下各 1px；右侧不留（滚动条已隐藏，勿再挤占一列）
    const TERMINAL_CONTENT_INSET: egui::Margin = egui::Margin {
        left: 1.0,
        right: 0.0,
        top: 1.0,
        bottom: 1.0,
    };
    /// Scroll 内容与视口边框的极小余量，避免偶发裁切一个字形
    const INNER_TEXT_SLACK: f32 = 0.0;
    /// ScrollArea **内容区内宽**（已不含纵向滚动条）→ TextEdit.desired_width
    #[inline]
    fn text_width_in_scroll_viewport(scroll_inner_width: f32) -> f32 {
        (scroll_inner_width - Self::INNER_TEXT_SLACK).max(64.0)
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
        }
    }

    /// 连接到 SSH 服务器
    pub fn connect(&mut self, host: &str, port: u16, username: &str, password: &str) {
        let config = SshConfig {
            host: host.to_string(),
            port,
            username: username.to_string(),
            password: password.to_string(),
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
                self.terminal.feed(b"Connecting...\r\n");
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
    pub fn show(&mut self, ui: &mut egui::Ui, theme: &Theme, column_width: f32) {
        // 先处理网络与键盘，再绘制，避免输入/输出滞后一帧
        self.process_ssh_messages();
        self.flush_paste_queue(ui.ctx());
        self.process_transfer_events(ui.ctx());

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

        self.capture_inline_input(ui);
        if self.connected {
            // ZMODEM 上传时 PTY 回包经 UI 旁路进 lrzsz；须高频重绘以免 `mpsc` 积压导致「等 ZACK」式卡顿。
            if self.lrzsz.is_upload_pty_capture() {
                ui.ctx().request_repaint();
            } else {
                // 保持动态程序（top/vim）持续刷新
                ui.ctx().request_repaint_after(Duration::from_millis(33));
            }
        }

        // 吃满宿主 allocate 的矩形；否则 Frame 随内容收缩，父区露出 bg_body（像左边/上边一条灰）并易触发中央区滚动条
        let fill_region = ui.available_size();
        if fill_region.x.is_finite()
            && fill_region.y.is_finite()
            && fill_region.x > 0.0
            && fill_region.y > 0.0
        {
            ui.set_min_size(fill_region);
        }

        let _ = column_width.max(1.0).min(16_384.0);
        // 进度条在 Frame 内先占位；若仍用全高算行列，网格会高于 ScrollArea，滚动与「│」光标错位
        // 与 Frame 内底部进度条占位一致（分隔线 + 两行文案 + ProgressBar）
        const TRANSFER_FOOTER_H: f32 = 72.0;
        // README §2.4：主底栏承担状态；PTY 尺寸在 ScrollArea 内容区内按真实 viewport 同步（见下方）
        egui::Frame::none()
            .fill(theme.bg_terminal_color())
            .inner_margin(Self::TERMINAL_CONTENT_INSET)
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
                    let col_w = ui.available_width();
                    let col_h = ui.available_height();
                    ui.set_min_width(col_w.max(1.0));
                    ui.set_min_height(col_h.max(1.0));
                    let footer_h = if self.transfer_progress.is_some() {
                        TRANSFER_FOOTER_H
                    } else {
                        0.0
                    };
                    let scroll_h = (ui.available_height() - footer_h).max(80.0);

                    // 终端内容区在上，ZMODEM 进度条固定在底部，避免插在命令与 shell 输出之间
                    let terminal_bg = theme.bg_terminal_color();
                    let layout_job = self.terminal.get_layout_job(
                        self.font_size,
                        theme.fg_medium_color(),
                        terminal_bg,
                    );
                    let scroll_output = egui::ScrollArea::vertical()
                        .stick_to_bottom(self.auto_follow_output)
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(
                            egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden,
                        )
                        .max_height(scroll_h)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                            ui.set_min_width(ui.available_width().max(1.0));
                            let vw = ui.available_width().max(1.0);
                            // 先铺与格子一致的底色，避免 PTY 行高取整后 galley 略矮露出「另一块」底色
                            let pre = ui.available_rect_before_wrap();
                            if pre.width() > 0.0 && pre.height() > 0.0 {
                                ui.painter().rect_filled(pre, 0.0, terminal_bg);
                            }
                            // 与 TextEdit 同宽：Scroll 内容区已不含纵向条，勿再扣 SCROLL_VBAR_RESERVE（否则会窄一截、右侧露 extreme_bg）
                            self.sync_pty_size_with_ui(ui, egui::vec2(vw, scroll_h.max(1.0)), theme);
                            let edit_w = Self::text_width_in_scroll_viewport(vw);
                            // `String` + 可编辑会触发 egui 插入/IME 光标，与 VT 里「│」叠成双光标；用 `&str` 只读缓冲只保留 PTY 光标
                            let display_owned = self.terminal.get_formatted_output();
                            let mut display_view: &str = display_owned.as_str();
                            let mut layouter = |ui: &egui::Ui, text: &str, _wrap_width: f32| {
                                let _ = text;
                                let job = layout_job.clone();
                                ui.ctx().fonts(|f| f.layout_job(job))
                            };
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
                                    // 终端输入场景下，Tab/方向键应优先发给 PTY，不应被 egui 焦点遍历抢走。
                                    .lock_focus(true)
                                    .interactive(true)
                                    .frame(false)
                                    .layouter(&mut layouter),
                            );
                            if response.clicked() {
                                response.request_focus();
                            }
                            if self.pending_focus_terminal {
                                response.request_focus();
                                self.pending_focus_terminal = false;
                            }
                            self.terminal_focused = response.has_focus();
                            response.context_menu(|ui| {
                                if ui.button("复制全部").clicked() {
                                    if let Ok(mut clip) = Clipboard::new() {
                                        let _ = clip.set_text(self.terminal.get_formatted_output());
                                    }
                                    ui.close_menu();
                                }
                                if ui.button("粘贴").clicked() {
                                    if let Ok(mut clip) = Clipboard::new() {
                                        if let Ok(text) = clip.get_text() {
                                            self.paste_text(&text, ui.ctx());
                                        }
                                    }
                                    ui.close_menu();
                                }
                                ui.separator();
                                if ui.button("清屏").clicked() {
                                    self.clear_screen();
                                    ui.close_menu();
                                }
                                if ui.button("字体 +").clicked() {
                                    self.set_font_size(self.font_size + 1.0);
                                    ui.close_menu();
                                }
                                if ui.button("字体 -").clicked() {
                                    self.set_font_size(self.font_size - 1.0);
                                    ui.close_menu();
                                }
                            });
                        });

                    // iTerm2 风格：离开底部则停止自动跟随，回到底部恢复跟随
                    let viewport_h = scroll_output.inner_rect.height();
                    let content_h = scroll_output.content_size.y;
                    let max_offset_y = (content_h - viewport_h).max(0.0);
                    let offset_y = scroll_output.state.offset.y.max(0.0);
                    let at_bottom = (max_offset_y - offset_y) <= 2.0;
                    self.auto_follow_output = at_bottom;

                    if let Some(ref progress) = self.transfer_progress {
                        ui.add_space(4.0);
                        ui.separator();
                        let dir = if self.transfer_outgoing {
                            "本机 → 远端"
                        } else {
                            "远端 → 本机"
                        };
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("📁 ZMODEM")
                                    .strong()
                                    .color(theme.accent_color()),
                            );
                            ui.label(
                                egui::RichText::new(dir).color(theme.fg_medium_color()),
                            );
                            ui.label(egui::RichText::new("·").color(theme.fg_low_color()));
                            ui.label(
                                egui::RichText::new(&progress.0)
                                    .monospace()
                                    .color(theme.fg_high_color()),
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
                                .text(egui::RichText::new(detail).color(theme.fg_high_color())),
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
                fonts.layout_no_wrap("W".to_string(), font_id, theme.fg_high_color());
            (galley.size().x.max(6.0), galley.size().y.max(12.0))
        });

        let cols = (usable_width / cell_w).floor().clamp(20.0, 512.0) as u32;
        let rows = (usable_height / cell_h).floor().clamp(5.0, 256.0) as u32;

        if cols != self.cols || rows != self.rows {
            self.resize(cols, rows);
        }
    }

    /// 非活动标签仅消费 SSH 接收队列；若有新内容写入 VTE 则返回 `true`（用于低频重绘）。
    pub fn pump_ssh_only(&mut self) -> bool {
        self.process_ssh_messages()
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
    fn process_ssh_messages(&mut self) -> bool {
        let mut vte_dirty = false;
        if let Some(ref rx) = self.ssh_rx {
            for msg in rx.try_iter() {
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
                        if is_rz_prompt && !self.pending_rz_upload && self.connected {
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
                            self.terminal.feed(&display_data);
                            vte_dirty = true;
                        }
                    }
                    SshMessage::Connected { .. } => {
                        self.connected = true;
                        self.connected_at = Some(Instant::now());
                        self.terminal_focused = true;
                        self.pending_focus_terminal = true;
                        self.terminal.feed(b"\r\nConnected!\r\n\r\n");
                        vte_dirty = true;
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
                        self.error_message = Some(msg.clone());
                        self.connected_at = None;
                        self.terminal.feed(format!("Error: {}\r\n", msg).as_bytes());
                        vte_dirty = true;
                        self.auto_follow_output = true;
                    }
                    SshMessage::Disconnected { .. } => {
                        self.connected = false;
                        self.terminal_focused = false;
                        self.connected_at = None;
                        self.terminal.feed(b"\r\nDisconnected\r\n");
                        vte_dirty = true;
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
    fn process_transfer_events(&mut self, ctx: &egui::Context) {
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
                    self.terminal.feed(
                        format!(
                            "\r\n✅ {}：{} -> {}\r\n\r\n",
                            title,
                            filename,
                            path.display()
                        )
                        .as_bytes(),
                    );
                }
                TransferEvent::FileError { filename, error } => {
                    if let Some(ref h) = self.ssh_handle {
                        self.lrzsz.unregister_shell_pump_upload_feed(h);
                    }
                    self.transfer_progress = None;
                    self.transfer_outgoing = false;
                    self.terminal.feed(
                        format!("\r\n❌ 传输失败 {}: {}\r\n", filename, error).as_bytes()
                    );
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
                            if *key == egui::Key::V {
                                if let Ok(mut clip) = Clipboard::new() {
                                    if let Ok(text) = clip.get_text() {
                                        pending_paste = Some(text);
                                    }
                                }
                            }
                            continue;
                        }
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
        egui::Window::new("resend_offline_input")
            .open(&mut open)
            .title_bar(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .frame(
                egui::Frame::popup(&ctx.style())
                    .fill(theme.bg_window_color())
                    .stroke(egui::Stroke::new(1.0, theme.border_color()))
                    .rounding(8.0)
                    .inner_margin(egui::Margin::symmetric(16.0, 14.0)),
            )
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("检测到断线期间暂存的输入")
                        .size(15.0)
                        .strong()
                        .color(theme.fg_high_color()),
                );
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!("共 {} 字节，是否发送到当前远程 shell？", n))
                        .size(13.0)
                        .color(theme.fg_medium_color()),
                );
                if !preview_esc.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(format!("预览：{}", preview_esc))
                            .monospace()
                            .size(11.0)
                            .color(theme.fg_low_color()),
                    );
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(egui::RichText::new("发送到远端").color(theme.accent_color()))
                        .clicked()
                    {
                        if let Some(handle) = self.ssh_handle.clone() {
                            for chunk in self.disconnected_input_buffer.chunks(4096) {
                                let _ = handle.send_input(chunk);
                            }
                        }
                        self.disconnected_input_buffer.clear();
                        self.buffer_input_while_disconnected = false;
                        self.resend_offline_input_dialog_open = false;
                    }
                    if ui.button("丢弃缓存").clicked() {
                        self.disconnected_input_buffer.clear();
                        self.buffer_input_while_disconnected = false;
                        self.resend_offline_input_dialog_open = false;
                    }
                });
            });
        if !open {
            self.resend_offline_input_dialog_open = false;
        }
    }

    /// 将键盘事件直接写入 PTY，由远端 shell 回显，避免「本地预览 + 回显」叠字（如 lsls）
    fn capture_inline_input(&mut self, ui: &egui::Ui) {
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
                            // macOS: ⌘V 粘贴到远端 shell（终端区选中复制仍交给 TextEdit 默认行为）
                            if *key == egui::Key::V {
                                if let Ok(mut clip) = Clipboard::new() {
                                    if let Ok(text) = clip.get_text() {
                                        pending_paste = Some(text);
                                    }
                                }
                            }
                            continue;
                        }
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
                                if let Err(e) = handle.send_input(b"\r") {
                                    log::error!("PTY write (enter): {}", e);
                                }
                            }
                            egui::Key::Backspace => {
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
        let Some(handle) = self.ssh_handle.as_ref() else {
            return;
        };
        let normalized = command.replace("\r\n", "\n").replace('\r', "\n");
        for line in normalized.lines() {
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
        self.terminal = VtTerminal::new(self.cols as usize, self.rows as usize);
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
        self.font_size = size.clamp(10.0, 24.0);
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
        } else {
            format!("已连接 {}m", mins)
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

    /// 在当前视口文本中搜索（与 [`Self::show`] 所用 `get_formatted_output` 同源；不含卷动区历史）。
    ///
    /// 返回每个匹配位置：**(行号从 1 起, 列号从 1 起，按 Unicode 标量值计)**。
    pub fn search_viewport(&self, query: &str, ignore_case: bool) -> Vec<(usize, usize)> {
        if query.is_empty() {
            return Vec::new();
        }
        let text = self.terminal.get_formatted_output();
        let q_len = query.chars().count();
        if q_len == 0 {
            return Vec::new();
        }
        let q_cmp = if ignore_case {
            query.to_lowercase()
        } else {
            query.to_string()
        };
        let mut out = Vec::new();
        for (line_idx, line) in text.lines().enumerate() {
            let line_chars: Vec<char> = line.chars().collect();
            let n = line_chars.len();
            if n < q_len {
                continue;
            }
            for start in 0..=(n - q_len) {
                let window: String = line_chars[start..start + q_len].iter().collect();
                let ok = if ignore_case {
                    window.to_lowercase() == q_cmp
                } else {
                    window == q_cmp
                };
                if ok {
                    out.push((line_idx + 1, start + 1));
                }
            }
        }
        out
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
