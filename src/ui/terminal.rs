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
use crate::ssh::{SshManager, SshConfig, SshMessage, SshSessionHandle, LrzszTransfer, TransferEvent};
use crate::terminal::Terminal as VtTerminal;

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
}

#[derive(Debug, Clone)]
pub struct RemotePathEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

impl TerminalView {
    const SFTP_RETRY_ATTEMPTS: usize = 160;
    const SFTP_RETRY_SLEEP_MS: u64 = 8;

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
                self.error_message = Some(format!("Failed to create session: {}", e));
            }
        }
    }

    /// 显示终端视图
    pub fn show(&mut self, ui: &mut egui::Ui) {
        // 先处理网络与键盘，再绘制，避免输入/输出滞后一帧
        self.process_ssh_messages();
        self.process_transfer_events(ui.ctx());
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

        let available_size = ui.available_size();
        // 进度条在 Frame 内先占位；若仍用全高算行列，网格会高于 ScrollArea，滚动与「│」光标错位
        // 与 Frame 内底部进度条占位一致（分隔线 + 两行文案 + ProgressBar）
        const TRANSFER_FOOTER_H: f32 = 72.0;
        let progress_reserve_y = if self.transfer_progress.is_some() {
            TRANSFER_FOOTER_H
        } else {
            0.0
        };
        let pty_sync_size = egui::vec2(
            available_size.x,
            (available_size.y - progress_reserve_y).max(80.0),
        );
        self.sync_pty_size_with_ui(ui, pty_sync_size);
        
        // 设计稿终端区域：背景 #0a0a12、内边距 16px（状态栏由主窗口统一渲染）
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(10, 10, 18)) // #0a0a12
            .inner_margin(egui::Margin::same(16.0))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    let footer_h = if self.transfer_progress.is_some() {
                        TRANSFER_FOOTER_H
                    } else {
                        0.0
                    };
                    let scroll_h = (ui.available_height() - footer_h).max(80.0);

                    // 终端内容区在上，ZMODEM 进度条固定在底部，避免插在命令与 shell 输出之间
                    let layout_job =
                        self.terminal
                            .get_layout_job(self.font_size, egui::Color32::from_rgb(212, 212, 212));
                    let scroll_output = egui::ScrollArea::vertical()
                        .stick_to_bottom(self.auto_follow_output)
                        .auto_shrink([false, false])
                        .max_height(scroll_h)
                        .show(ui, |ui| {
                            // `String` + 可编辑会触发 egui 插入/IME 光标，与 VT 里「│」叠成双光标；用 `&str` 只读缓冲只保留 PTY 光标
                            let display_owned = self.terminal.get_formatted_output();
                            let mut display_view: &str = display_owned.as_str();
                            let mut layouter = |ui: &egui::Ui, text: &str, _wrap_width: f32| {
                                let _ = text;
                                let job = layout_job.clone();
                                ui.ctx().fonts(|f| f.layout_job(job))
                            };
                            let response = ui.add(
                                egui::TextEdit::multiline(&mut display_view)
                                    .id_source("terminal_text_area")
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
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
                                            self.paste_text(&text);
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
                            ui.label(egui::RichText::new("📁 ZMODEM").strong());
                            ui.label(dir);
                            ui.label("·");
                            ui.monospace(&progress.0);
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
                        ui.add(egui::ProgressBar::new(percent / 100.0).text(detail));
                    }
                });
            });
    }

    fn sync_pty_size_with_ui(&mut self, ui: &egui::Ui, available_size: egui::Vec2) {
        // 预留状态栏、进度条、边距；输入已并入滚动区，不再单独预留一行
        let usable_width = (available_size.x - 32.0).max(120.0);
        // 无终端内状态栏后略减预留（进度条等仍占高）
        let usable_height = (available_size.y - 72.0).max(80.0);

        // 用真实字体测量单字符网格尺寸，避免 80x24 误差
        let font_id = egui::FontId::monospace(self.font_size);
        let (cell_w, cell_h) = ui.ctx().fonts(|fonts| {
            let galley = fonts.layout_no_wrap("W".to_string(), font_id, egui::Color32::WHITE);
            (galley.size().x.max(6.0), galley.size().y.max(12.0))
        });

        let cols = (usable_width / cell_w).floor().clamp(20.0, 512.0) as u32;
        let rows = (usable_height / cell_h).floor().clamp(5.0, 256.0) as u32;

        if cols != self.cols || rows != self.rows {
            self.resize(cols, rows);
        }
    }

    /// 处理 SSH 消息
    fn process_ssh_messages(&mut self) {
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
                        }
                    }
                    SshMessage::Connected { .. } => {
                        self.connected = true;
                        self.connected_at = Some(Instant::now());
                        self.terminal_focused = true;
                        self.pending_focus_terminal = true;
                        self.terminal.feed(b"\r\nConnected!\r\n\r\n");
                        self.auto_follow_output = true;
                        
                        // 启动交互式 shell
                        if let Some(ref manager) = self.ssh_manager {
                            if let Some(session_id) = self.session_id {
                                match manager.start_interactive_shell(session_id, self.cols, self.rows) {
                                    Ok(handle) => {
                                        self.ssh_handle = Some(handle);
                                    }
                                    Err(e) => {
                                        self.error_message = Some(format!("Failed to start shell: {}", e));
                                    }
                                }
                            }
                        }
                    }
                    SshMessage::Error { error, .. } => {
                        self.error_message = Some(error.clone());
                        self.connected_at = None;
                        self.terminal.feed(format!("Error: {}\r\n", error).as_bytes());
                        self.auto_follow_output = true;
                    }
                    SshMessage::Disconnected { .. } => {
                        self.connected = false;
                        self.terminal_focused = false;
                        self.connected_at = None;
                        self.terminal.feed(b"\r\nDisconnected\r\n");
                        self.auto_follow_output = true;
                    }
                    SshMessage::UserCommand { command, .. } => {
                        *self.command_usage.entry(command).or_insert(0) += 1;
                    }
                }
            }
        }
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

    /// 将键盘事件直接写入 PTY，由远端 shell 回显，避免「本地预览 + 回显」叠字（如 lsls）
    fn capture_inline_input(&mut self, ui: &egui::Ui) {
        if !self.connected {
            return;
        }
        let Some(handle) = self.ssh_handle.as_ref() else {
            return;
        };
        if !self.terminal_focused {
            return;
        }

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
                                        self.paste_text(&text);
                                    }
                                }
                            }
                            continue;
                        }
                        match key {
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
                            egui::Key::C if modifiers.ctrl => {
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

    /// 粘贴文本到终端：原样发到 PTY，不自动补回车
    fn paste_text(&self, text: &str) {
        if !self.connected {
            return;
        }
        let Some(handle) = self.ssh_handle.as_ref() else {
            return;
        };
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        if let Err(e) = handle.send_input(normalized.as_bytes()) {
            log::error!("PTY write (paste): {}", e);
        }
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

    pub fn download_remote_file(&mut self, remote_path: &str, local_path: &Path) -> Result<(), String> {
        let session_id = self.session_id
            .ok_or_else(|| "没有 SSH 会话".to_string())?;
        let session = self.ssh_manager.as_ref()
            .and_then(|m| m.get_session(session_id))
            .ok_or_else(|| "获取 SSH 会话失败".to_string())?;
        let sftp = Self::retry_sftp_op(|| session.sftp(), "创建 SFTP 通道失败")?;
        let mut remote = Self::retry_sftp_op(
            || sftp.open(Path::new(remote_path)),
            "打开远端文件失败",
        )?;

        let mut buf = Vec::new();
        use std::io::Read;
        let mut chunk = [0u8; 16 * 1024];
        loop {
            match remote.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) => {
                    if Self::is_would_block_like(&e) {
                        thread::sleep(Duration::from_millis(8));
                        continue;
                    }
                    return Err(format!("SFTP 下载读取失败：{}", e));
                }
            }
        }
        std::fs::write(local_path, &buf)
            .map_err(|e| format!("保存本地文件失败：{}", e))?;
        Ok(())
    }

    pub fn list_remote_dir(&self, remote_dir: &str) -> Result<Vec<RemotePathEntry>, String> {
        let session_id = self.session_id
            .ok_or_else(|| "没有 SSH 会话".to_string())?;
        let session = self.ssh_manager.as_ref()
            .and_then(|m| m.get_session(session_id))
            .ok_or_else(|| "获取 SSH 会话失败".to_string())?;

        let sftp = Self::retry_sftp_op(|| session.sftp(), "创建 SFTP 失败")?;
        let path = PathBuf::from(remote_dir);
        let entries = Self::retry_sftp_op(
            || sftp.readdir(path.as_path()),
            "读取远端目录失败",
        )?;

        let mut items = entries
            .into_iter()
            .filter_map(|(p, stat)| {
                let name = p.file_name()?.to_string_lossy().to_string();
                if name == "." || name == ".." {
                    return None;
                }
                let is_dir = stat.is_dir();
                Some(RemotePathEntry {
                    name,
                    is_dir,
                    size: stat.size,
                })
            })
            .collect::<Vec<_>>();
        items.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(items)
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

    /// 断开连接
    pub fn disconnect(&mut self) {
        if let Some(ref h) = self.ssh_handle {
            self.lrzsz.unregister_shell_pump_upload_feed(h);
        }
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
