//! 终端视图
#![allow(dead_code)]
//!
//! 显示终端模拟器、处理输入输出、集成 SSH 连接

use eframe::egui;
use arboard::Clipboard;
use std::path::Path;
use std::sync::mpsc::Receiver;
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
    
    /// 文件传输进度
    transfer_progress: Option<(String, u64, u64)>,
    
    /// 下载目录
    download_dir: String,
    font_size: f32,
    connected_at: Option<Instant>,
    connection_target: Option<(String, String)>,
    auto_follow_output: bool,
    terminal_focused: bool,
}

impl TerminalView {
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
            transfer_progress: None,
            download_dir: download_dir.to_string_lossy().to_string(),
            font_size: 14.0,
            connected_at: None,
            connection_target: None,
            auto_follow_output: true,
            terminal_focused: false,
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
        self.process_transfer_events();
        self.capture_inline_input(ui);
        if self.connected {
            // 保持动态程序（top/vim）持续刷新
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        }

        let available_size = ui.available_size();
        self.sync_pty_size_with_ui(ui, available_size);
        
        // README §2.4 终端区域：背景 #1e1e1e、内边距 16px（不在终端内再放一条状态栏，由主窗口底栏承担）
        egui::Frame::none()
            .fill(egui::Color32::from_rgb(30, 30, 30)) // #1e1e1e
            .inner_margin(egui::Margin::same(16.0))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    // 文件传输进度
                    if let Some(ref progress) = self.transfer_progress {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label("📁 文件传输:");
                            ui.label(&progress.0);
                        });
                        
                        let percent = (progress.1 as f32 / progress.2 as f32 * 100.0).min(100.0);
                        ui.add(
                            egui::ProgressBar::new(percent / 100.0)
                                .text(format!("{:.1}%", percent))
                                .show_percentage()
                        );
                    }

                    // 终端内容区：仅展示远端 PTY 回显（按键即时写入 PTY，避免本地再拼一层导致重复）
                    let scroll_h = ui.available_height().max(80.0);
                    let layout_job =
                        self.terminal
                            .get_layout_job(self.font_size, egui::Color32::from_rgb(212, 212, 212));
                    let scroll_output = egui::ScrollArea::vertical()
                        .stick_to_bottom(self.auto_follow_output)
                        .auto_shrink([false, false])
                        .max_height(scroll_h)
                        .show(ui, |ui| {
                            // 用只读 TextEdit 支持鼠标选中/系统复制
                            let mut display_text = self.terminal.get_formatted_output();
                            let mut layouter = |ui: &egui::Ui, text: &str, _wrap_width: f32| {
                                let _ = text;
                                let job = layout_job.clone();
                                ui.ctx().fonts(|f| f.layout_job(job))
                            };
                            let response = ui.add(
                                egui::TextEdit::multiline(&mut display_text)
                                    .id_source("terminal_text_area")
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
                                    .code_editor()
                                    .lock_focus(true)
                                    .interactive(true)
                                    .frame(false)
                                    .layouter(&mut layouter),
                            );
                            if response.clicked() {
                                response.request_focus();
                            }
                            if self.terminal_focused {
                                response.request_focus();
                            }
                            self.terminal_focused = response.has_focus() || response.clicked();
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
                        // 如果 lrzsz 正在传输中，数据喂给 lrzsz 而不是终端
                        if self.lrzsz.is_active() {
                            log::info!("lrzsz 激活中，尝试 feed 数据 ({} bytes)", data.len());
                            let consumed = self.lrzsz.feed_receive_data(&data);
                            if consumed {
                                log::info!("数据已被 lrzsz 消费");
                                continue;
                            } else {
                                log::info!("lrzsz 未消费数据，传给终端");
                            }
                        }
                        
                        // 检测 lrzsz 命令
                        if self.lrzsz.detect_rz_command(&data) && !self.lrzsz.is_active() {
                            log::info!("检测到 rz 命令，启动文件接收 ({} bytes)", data.len());
                            // 获取 SSH 通道
                            if let Some(ref handle) = self.ssh_handle {
                                if let Some(channel) = handle.get_channel() {
                                    // 使用 feed_data 模式启动接收
                                    if let Err(e) = self.lrzsz.start_receive(channel) {
                                        log::error!("启动文件接收失败：{}", e);
                                    } else {
                                        // 数据已被 lrzsz 消费，不传给终端
                                        continue;
                                    }
                                }
                            }
                        }
                        
                        self.terminal.feed(&data);
                    }
                    SshMessage::Connected { .. } => {
                        self.connected = true;
                        self.connected_at = Some(Instant::now());
                        self.terminal_focused = true;
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
                }
            }
        }
    }

    /// 处理文件传输事件
    fn process_transfer_events(&mut self) {
        while let Some(event) = self.lrzsz.try_recv_event() {
            match event {
                TransferEvent::FileStart { filename, size } => {
                    self.transfer_progress = Some((filename.clone(), 0, size));
                    self.terminal.feed(
                        format!("\r\n📥 开始接收：{} ({})\r\n", filename, 
                            human_readable_size(size)).as_bytes()
                    );
                }
                TransferEvent::FileProgress { received, total, .. } => {
                    if let Some(ref mut progress) = self.transfer_progress {
                        *progress = (progress.0.clone(), received, total);
                    }
                }
                TransferEvent::FileComplete { filename, path } => {
                    self.transfer_progress = None;
                    self.terminal.feed(
                        format!("\r\n✅ 接收完成：{} -> {}\r\n", filename, path.display()).as_bytes()
                    );
                }
                TransferEvent::FileError { filename, error } => {
                    self.transfer_progress = None;
                    self.terminal.feed(
                        format!("\r\n❌ 传输失败 {}: {}\r\n", filename, error).as_bytes()
                    );
                }
                TransferEvent::TransferComplete => {
                    // 传输完成，恢复终端交互状态
                    self.auto_follow_output = true;
                    self.terminal.feed(b"\r\n");
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

    pub fn start_upload(&mut self, path: &Path) -> Result<(), String> {
        if let Some(ref handle) = self.ssh_handle {
            if let Some(channel) = handle.get_channel() {
                return self.lrzsz.start_send(&path.to_string_lossy(), channel);
            }
        }
        Err("没有活动的 SSH 连接".to_string())
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

    /// 调整终端尺寸
    pub fn resize(&mut self, cols: u32, rows: u32) {
        self.cols = cols;
        self.rows = rows;
        self.terminal.resize(cols as usize, rows as usize);
        
        if let Some(ref handle) = self.ssh_handle {
            if let Err(e) = handle.resize_pty(cols, rows) {
                log::error!("Failed to resize PTY: {}", e);
            }
        }
    }

    /// 断开连接
    pub fn disconnect(&mut self) {
        self.connected = false;
        self.ssh_handle = None;
        self.ssh_manager = None;
        self.ssh_rx = None;
        self.session_id = None;
        self.terminal = VtTerminal::new(self.cols as usize, self.rows as usize);
        self.error_message = None;
        self.transfer_progress = None;
        self.connected_at = None;
        self.terminal_focused = false;
    }

    /// 插入命令片段（自动添加回车）
    pub fn insert_fragment(&mut self, command: &str) {
        if self.connected {
            if let Some(ref handle) = self.ssh_handle {
                let input = format!("{}\r", command);
                if let Err(e) = handle.send_input(input.as_bytes()) {
                    log::error!("Failed to send fragment: {}", e);
                }
            }
        }
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
