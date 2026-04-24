//! 终端视图
//!
//! 显示终端模拟器、处理输入输出、集成 SSH 连接

use eframe::egui;
use std::sync::mpsc::{Receiver, TryIter};
use crate::ssh::{SshManager, SshConfig, SshMessage, SshSessionHandle, LrzszTransfer, TransferEvent};

/// 终端内容缓冲区
struct TerminalBuffer {
    /// 原始字节数据
    data: Vec<u8>,
    /// 最大缓冲区大小
    max_size: usize,
    /// 缓存的字符串
    cached_str: String,
}

impl TerminalBuffer {
    fn new() -> Self {
        Self {
            data: Vec::with_capacity(1024 * 1024),
            max_size: 1024 * 1024,
            cached_str: String::new(),
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
        
        if self.data.len() > self.max_size {
            let remove_count = self.data.len() - self.max_size / 2;
            self.data.drain(..remove_count);
        }
        
        // 更新缓存
        self.cached_str = String::from_utf8_lossy(&self.data).to_string();
    }

    fn as_str(&self) -> &str {
        &self.cached_str
    }

    fn clear(&mut self) {
        self.data.clear();
        self.cached_str.clear();
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
    
    /// 终端内容
    buffer: TerminalBuffer,
    
    /// 连接状态
    connected: bool,
    
    /// 连接错误信息
    error_message: Option<String>,
    
    /// 输入缓冲区
    input_buffer: String,
    
    /// 终端尺寸
    cols: u32,
    rows: u32,
    
    /// lrzsz 文件传输器
    lrzsz: LrzszTransfer,
    
    /// 文件传输进度
    transfer_progress: Option<(String, u64, u64)>,
    
    /// 下载目录
    download_dir: String,
}

impl TerminalView {
    /// 创建新的终端视图
    pub fn new() -> Self {
        Self {
            session_id: None,
            ssh_manager: None,
            ssh_rx: None,
            ssh_handle: None,
            buffer: TerminalBuffer::new(),
            connected: false,
            error_message: None,
            input_buffer: String::new(),
            cols: 80,
            rows: 24,
            lrzsz: LrzszTransfer::new(),
            transfer_progress: None,
            download_dir: std::env::temp_dir().join("mistterm_downloads")
                .to_string_lossy()
                .to_string(),
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
                self.buffer.clear();
                self.buffer.data.extend_from_slice(b"Connecting...\r\n");
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to create session: {}", e));
            }
        }
    }

    /// 显示终端视图
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let _available_size = ui.available_size();
        
        egui::Frame::none()
            .fill(ui.visuals().panel_fill)
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    // 连接状态栏
                    self.show_status_bar(ui);

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

                    // 终端内容区
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(self.buffer.as_str())
                                    .wrap(false)
                                    .sense(egui::Sense::focusable_noninteractive())
                            );
                        });

                    // 输入区
                    ui.add_space(8.0);
                    
                    if self.connected {
                        ui.horizontal(|ui| {
                            ui.label(">");
                            if ui.text_edit_singleline(&mut self.input_buffer).lost_focus() {
                                self.send_input();
                            }
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.label("等待连接...");
                        });
                    }
                });
            });

        // 处理 SSH 消息
        self.process_ssh_messages();
        
        // 处理文件传输事件
        self.process_transfer_events();
    }

    /// 显示状态栏
    fn show_status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.connected {
                ui.colored_label(egui::Color32::GREEN, "● Connected");
            } else if self.error_message.is_some() {
                ui.colored_label(egui::Color32::RED, "● Error");
            } else {
                ui.colored_label(egui::Color32::YELLOW, "○ Connecting...");
            }

            if let Some(ref session_id) = self.session_id {
                ui.label(format!("Session: {}", session_id));
            }

            ui.label(format!("{}x{}", self.cols, self.rows));

            if let Some(ref error) = self.error_message {
                ui.colored_label(egui::Color32::RED, error);
            }
        });
    }

    /// 处理 SSH 消息
    fn process_ssh_messages(&mut self) {
        if let Some(ref rx) = self.ssh_rx {
            for msg in rx.try_iter() {
                match msg {
                    SshMessage::Output { data, .. } => {
                        // 检测 lrzsz 命令
                        if self.lrzsz.detect_rz_command(&data) && !self.lrzsz.is_active() {
                            log::info!("检测到 rz 命令，启动文件接收");
                            if let Err(e) = self.lrzsz.start_receive(&self.download_dir) {
                                log::error!("启动文件接收失败：{}", e);
                            }
                        }
                        
                        self.buffer.push(&data);
                    }
                    SshMessage::Connected { .. } => {
                        self.connected = true;
                        self.buffer.data.extend_from_slice(b"\r\nConnected!\r\n\r\n");
                        
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
                        self.buffer.data.extend_from_slice(format!("Error: {}\r\n", error).as_bytes());
                    }
                    SshMessage::Disconnected { .. } => {
                        self.connected = false;
                        self.buffer.data.extend_from_slice(b"\r\nDisconnected\r\n");
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
                    self.buffer.data.extend_from_slice(
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
                    self.buffer.data.extend_from_slice(
                        format!("\r\n✅ 接收完成：{} -> {}\r\n", filename, path.display()).as_bytes()
                    );
                }
                TransferEvent::FileError { filename, error } => {
                    self.transfer_progress = None;
                    self.buffer.data.extend_from_slice(
                        format!("\r\n❌ 传输失败 {}: {}\r\n", filename, error).as_bytes()
                    );
                }
                TransferEvent::TransferComplete => {
                    // 传输完成
                }
            }
        }
    }

    /// 发送输入
    fn send_input(&mut self) {
        if !self.connected {
            return;
        }

        let input = format!("{}\r", self.input_buffer);
        self.buffer.data.extend_from_slice(input.as_bytes());
        
        if let Some(ref handle) = self.ssh_handle {
            if let Err(e) = handle.send_input(input.as_bytes()) {
                log::error!("Failed to send input: {}", e);
            }
        }

        self.input_buffer.clear();
    }

    /// 调整终端尺寸
    pub fn resize(&mut self, cols: u32, rows: u32) {
        self.cols = cols;
        self.rows = rows;
        
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
        self.buffer.clear();
        self.error_message = None;
        self.transfer_progress = None;
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
