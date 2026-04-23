//! 主应用 UI

use crate::core::{ConnectionManager, ConnectionState, SessionConfig, SessionManager};
use crate::ssh::SshMessage;
use eframe::egui;

/// 主应用
pub struct MistTermApp {
    /// 会话管理器
    session_manager: SessionManager,
    /// 连接管理器
    connection_manager: Option<ConnectionManager>,
    /// 消息接收器
    message_rx: Option<std::sync::mpsc::Receiver<SshMessage>>,
    /// 选中的会话索引
    selected_session: Option<usize>,
    /// 是否显示连接对话框
    showing_connect_dialog: bool,
    /// 新连接配置
    new_config: SessionConfig,
}

impl Default for MistTermApp {
    fn default() -> Self {
        let session_manager = SessionManager::new();
        let (connection_manager, rx) = ConnectionManager::new();
        
        let mut app = Self {
            session_manager,
            connection_manager: Some(connection_manager),
            message_rx: Some(rx),
            selected_session: None,
            showing_connect_dialog: false,
            new_config: SessionConfig::default(),
        };
        
        // 从 SessionManager 加载的会话创建连接状态
        for config in app.session_manager.get_sessions().iter() {
            if let Some(ref mut conn_mgr) = app.connection_manager {
                conn_mgr.add_session(config.clone());
            }
        }
        
        app
    }
}

impl MistTermApp {
    fn is_terminal_connected(&self) -> bool {
        let Some(idx) = self.selected_session else {
            return false;
        };
        let Some(conn_mgr) = self.connection_manager.as_ref() else {
            return false;
        };
        conn_mgr
            .get_session(idx)
            .map(|s| matches!(s.lock().state, ConnectionState::Connected))
            .unwrap_or(false)
    }

    fn normalize_terminal_text(text: &str) -> String {
        text.chars()
            .map(|c| match c {
                // 全角空格 -> 半角空格
                '\u{3000}' => ' ',
                // 全角 ASCII 可打印区间（！到～）统一映射到半角
                '\u{FF01}'..='\u{FF5E}' => {
                    char::from_u32((c as u32) - 0xFEE0).unwrap_or(c)
                }
                '。' => '.',
                '【' => '[',
                '】' => ']',
                _ => c,
            })
            .collect()
    }

    fn send_bytes_to_session(&mut self, idx: usize, data: &[u8]) {
        let Some(ref conn_mgr) = self.connection_manager else {
            return;
        };
        if let Some(session) = conn_mgr.get_session(idx) {
            let sess = session.lock();
            if let Some(h) = &sess.handle {
                if let Err(e) = h.send_input(data) {
                    log::error!("[UI-INPUT] Failed to queue direct input: {}", e);
                }
            }
        }
    }

    fn ctrl_byte_from_key(key: egui::Key) -> Option<u8> {
        use egui::Key::*;
        let ch = match key {
            A => b'a',
            B => b'b',
            C => b'c',
            D => b'd',
            E => b'e',
            F => b'f',
            G => b'g',
            H => b'h',
            I => b'i',
            J => b'j',
            K => b'k',
            L => b'l',
            M => b'm',
            N => b'n',
            O => b'o',
            P => b'p',
            Q => b'q',
            R => b'r',
            S => b's',
            T => b't',
            U => b'u',
            V => b'v',
            W => b'w',
            X => b'x',
            Y => b'y',
            Z => b'z',
            _ => return None,
        };
        Some(ch & 0x1f)
    }

    fn key_to_symbol_bytes(key: egui::Key, shift: bool) -> Option<&'static [u8]> {
        match key {
            egui::Key::Minus if shift => Some(b"_"),
            egui::Key::Minus => Some(b"-"),
            egui::Key::PlusEquals if shift => Some(b"+"),
            egui::Key::PlusEquals => Some(b"="),
            egui::Key::Num0 if shift => Some(b")"),
            egui::Key::Num1 if shift => Some(b"!"),
            egui::Key::Num2 if shift => Some(b"@"),
            egui::Key::Num3 if shift => Some(b"#"),
            egui::Key::Num4 if shift => Some(b"$"),
            egui::Key::Num5 if shift => Some(b"%"),
            egui::Key::Num6 if shift => Some(b"^"),
            egui::Key::Num7 if shift => Some(b"&"),
            egui::Key::Num8 if shift => Some(b"*"),
            egui::Key::Num9 if shift => Some(b"("),
            _ => None,
        }
    }

    fn handle_direct_terminal_input(&mut self, ctx: &egui::Context) {
        if self.showing_connect_dialog {
            return;
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        let Some(idx) = self.selected_session else {
            return;
        };

        let connected = self
            .connection_manager
            .as_ref()
            .and_then(|m| m.get_session(idx))
            .map(|s| matches!(s.lock().state, ConnectionState::Connected))
            .unwrap_or(false);
        if !connected {
            return;
        }

        // 关键修复：一次性取走输入事件，避免同一批事件在多次 update 中被重复发送
        let events = ctx.input_mut(|i| std::mem::take(&mut i.events));
        let has_text_input = events.iter().any(|event| match event {
            egui::Event::Text(text) => {
                let normalized = Self::normalize_terminal_text(text);
                normalized.chars().any(|c| !c.is_control())
            }
            egui::Event::Paste(text) => !text.is_empty(),
            _ => false,
        });
        for event in events {
            match event {
                egui::Event::Text(text) => {
                    // 过滤控制字符，避免把 IME/组合态字符直接写进远端导致方块噪声
                    let normalized = Self::normalize_terminal_text(&text);
                    let filtered: String = normalized.chars().filter(|c| !c.is_control()).collect();
                    if !filtered.is_empty() {
                        log::info!("[UI-INPUT] direct text event: {:?}", filtered);
                        self.send_bytes_to_session(idx, filtered.as_bytes());
                    }
                }
                egui::Event::Paste(text) => {
                    let normalized = Self::normalize_terminal_text(&text);
                    if !normalized.is_empty() {
                        self.send_bytes_to_session(idx, normalized.as_bytes());
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    log::info!(
                        "[UI-INPUT] key event: {:?}, shift={}, ctrl={}, alt={}",
                        key, modifiers.shift, modifiers.ctrl, modifiers.alt
                    );
                    if modifiers.command || modifiers.mac_cmd {
                        continue;
                    }
                    if modifiers.ctrl {
                        if let Some(ctrl) = Self::ctrl_byte_from_key(key) {
                            self.send_bytes_to_session(idx, &[ctrl]);
                        }
                        continue;
                    }
                    // 若本帧已有 Text 事件，符号键优先走 Text，避免同一按键重复发送（如 '-' -> '--'）
                    if !has_text_input {
                        if let Some(symbol) = Self::key_to_symbol_bytes(key, modifiers.shift) {
                            self.send_bytes_to_session(idx, symbol);
                            continue;
                        }
                    }
                    match key {
                        egui::Key::Enter => self.send_bytes_to_session(idx, b"\r\n"),
                        egui::Key::Tab => self.send_bytes_to_session(idx, b"\t"),
                        egui::Key::Backspace => self.send_bytes_to_session(idx, b"\x7f"),
                        egui::Key::ArrowUp => self.send_bytes_to_session(idx, b"\x1b[A"),
                        egui::Key::ArrowDown => self.send_bytes_to_session(idx, b"\x1b[B"),
                        egui::Key::ArrowRight => self.send_bytes_to_session(idx, b"\x1b[C"),
                        egui::Key::ArrowLeft => self.send_bytes_to_session(idx, b"\x1b[D"),
                        egui::Key::Escape => self.send_bytes_to_session(idx, b"\x1b"),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    fn connect_session(&mut self, idx: usize) {
        let Some(ref mut conn_mgr) = self.connection_manager else {
            log::error!("Connection manager is None");
            return;
        };

        let Some(session) = conn_mgr.get_session(idx) else {
            log::error!("Session {} not found", idx);
            return;
        };

        let config = {
            let sess = session.lock();
            sess.config.clone()
        };

        // 设置连接状态
        {
            let mut sess = session.lock();
            sess.state = ConnectionState::Connecting;
            // 开始新连接前清理旧句柄，避免输入仍发往旧 SSH 会话
            sess.handle = None;
            sess.ssh_session_id = None;
        }

        let mut ssh_manager = conn_mgr.get_ssh_manager().clone();
        
        std::thread::spawn(move || {
            log::info!("[CONNECT] Connecting to {}@{}:{}", config.username, config.host, config.port);
            
            // 转换为 SSH 层的 SshConfig
            let ssh_config = crate::ssh::SshConfig {
                host: config.host.clone(),
                port: config.port,
                username: config.username.clone(),
                password: config.password.clone(),
            };
            
            match ssh_manager.create_session_async(ssh_config) {
                Ok(ssh_session_id) => {
                    log::info!(
                        "[CONNECT] Started async connection for ssh_session_id={}, ui_session_id={}",
                        ssh_session_id,
                        idx
                    );
                    {
                        let mut sess = session.lock();
                        sess.ssh_session_id = Some(ssh_session_id);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1000));
                    
                    // 启动 shell 时必须使用 SSH 层 session_id，避免开到错误连接
                    match ssh_manager.start_interactive_shell(ssh_session_id) {
                        Ok(handle) => {
                            let mut sess = session.lock();
                            sess.handle = Some(handle);
                            sess.state = ConnectionState::Connected;
                            log::info!("[CONNECT] Shell started for ssh_session_id={}", ssh_session_id);
                        }
                        Err(e) => {
                            log::error!("[CONNECT] Failed to start shell: {}", e);
                            let mut sess = session.lock();
                            sess.handle = None;
                            sess.ssh_session_id = None;
                            sess.state = ConnectionState::Error(format!("Shell failed: {}", e));
                        }
                    }
                }
                Err(e) => {
                    log::error!("[CONNECT] Failed to create session: {}", e);
                    let mut sess = session.lock();
                    sess.handle = None;
                    sess.ssh_session_id = None;
                    sess.state = ConnectionState::Error(e);
                }
            }
        });
    }
}

impl eframe::App for MistTermApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 持续刷新，确保后台线程收到的 SSH 输出能及时渲染
        ctx.request_repaint_after(std::time::Duration::from_millis(16));

        // 设置深色主题样式
        let mut style = (*ctx.style()).clone();
        
        // 深色配色方案
        style.visuals.panel_fill = egui::Color32::from_rgb(28, 28, 28);
        style.visuals.window_fill = egui::Color32::from_rgb(32, 32, 32);
        style.visuals.extreme_bg_color = egui::Color32::from_rgb(22, 22, 22);
        style.visuals.faint_bg_color = egui::Color32::from_rgb(38, 38, 38);
        style.visuals.override_text_color = Some(egui::Color32::from_rgb(225, 225, 225));
        style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(45, 45, 45);
        style.visuals.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(45, 45, 45);
        style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(60, 60, 60);
        style.visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(60, 60, 60);
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(70, 70, 70);
        style.visuals.widgets.active.weak_bg_fill = egui::Color32::from_rgb(70, 70, 70);
        style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(220, 220, 220));
        style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(235, 235, 235));
        style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(250, 250, 250));
        style.visuals.widgets.noninteractive.fg_stroke =
            egui::Stroke::new(1.0, egui::Color32::from_rgb(210, 210, 210));
        
        // 选中状态 - 用柔和的蓝色
        style.visuals.selection.bg_fill = egui::Color32::from_rgb(66, 100, 150);
        style.visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(100, 140, 200));
        
        ctx.set_style(style);
        
        // 处理 SSH 消息
        if let Some(ref rx) = self.message_rx {
            while let Ok(msg) = rx.try_recv() {
                if let Some(ref conn_mgr) = self.connection_manager {
                    conn_mgr.handle_ssh_message(msg, self.selected_session);
                }
            }
        }

        // 直接终端键盘输入（类似 iTerm）
        self.handle_direct_terminal_input(ctx);
        
        // 主界面
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(egui::Color32::from_rgb(30, 30, 30)))
            .show(ctx, |ui| {
                self.render_header(ui);
                self.render_session_list(ui);
                self.render_terminal(ui);
            });
        
        // 连接对话框
        if self.showing_connect_dialog {
            self.render_connect_dialog(ctx);
        }
    }
}

impl MistTermApp {
    fn render_header(&mut self, ui: &mut egui::Ui) {
        ui.heading(egui::RichText::new("MistTerm - SSH Terminal").color(egui::Color32::from_rgb(220, 220, 220)));
        
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // 终端交互中禁用“新建连接”，避免误触弹窗打断输入
                let allow_new_connect = !self.showing_connect_dialog && !self.is_terminal_connected();
                let btn = ui.add_enabled(allow_new_connect, egui::Button::new("Connect"));
                if btn.clicked_by(egui::PointerButton::Primary) {
                    self.showing_connect_dialog = true;
                    self.new_config = SessionConfig::default();
                }
            });
        });
        
        ui.add_space(10.0);
        ui.separator();
    }

    fn render_session_list(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Sessions:").color(egui::Color32::from_rgb(180, 180, 180)));
        
        let Some(ref conn_mgr) = self.connection_manager else {
            ui.label("Connection manager not initialized");
            return;
        };

        if conn_mgr.get_sessions().is_empty() {
            ui.label("No sessions. Click 'Connect' to add one.");
        }
        
        let mut delete_idx: Option<usize> = None;
        let mut select_idx: Option<usize> = None;
        let mut connect_idx: Option<usize> = None;
        
        for (idx, session) in conn_mgr.get_sessions().iter().enumerate() {
            let sess = session.lock();
            let status = sess.status_text();
            
            ui.horizontal(|ui| {
                if ui.selectable_label(self.selected_session == Some(idx), 
                    format!("{} - {} ({})", sess.config.name, sess.config.host, status)).clicked() {
                    select_idx = Some(idx);
                }
                if ui.small_button("Connect").clicked_by(egui::PointerButton::Primary) {
                    connect_idx = Some(idx);
                }
                if matches!(sess.state, ConnectionState::Connected)
                    && ui.small_button("X").clicked_by(egui::PointerButton::Primary)
                {
                    delete_idx = Some(idx);
                }
            });
        }
        
        if let Some(idx) = delete_idx {
            self.session_manager.remove_session(idx);
            if let Some(ref mut conn_mgr) = self.connection_manager {
                conn_mgr.remove_session(idx);
            }
            if self.selected_session == Some(idx) {
                self.selected_session = None;
            }
        }
        if let Some(idx) = select_idx {
            self.selected_session = Some(idx);
        }
        if let Some(idx) = connect_idx {
            self.selected_session = Some(idx);
            self.connect_session(idx);
        }
        
        ui.separator();
    }

    fn render_terminal(&mut self, ui: &mut egui::Ui) {
        let Some(ref conn_mgr) = self.connection_manager else {
            return;
        };

        if let Some(idx) = self.selected_session {
            if let Some(session) = conn_mgr.get_session(idx) {
                let sess = session.lock();
                
                ui.horizontal(|ui| {
                    ui.label(format!("{}@{}:{}", sess.config.username, sess.config.host, sess.config.port));
                    
                    match &sess.state {
                        ConnectionState::Connected => {
                            ui.label(egui::RichText::new("✓ Connected").color(egui::Color32::from_rgb(76, 175, 80)));
                        }
                        ConnectionState::Connecting => {
                            ui.label(egui::RichText::new("Connecting...").color(egui::Color32::from_rgb(255, 193, 7)));
                        }
                        ConnectionState::Error(e) => {
                            ui.label(egui::RichText::new(format!("Error: {}", e)).color(egui::Color32::from_rgb(244, 67, 54)));
                        }
                        ConnectionState::Disconnected => {}
                    }
                });
                
                ui.add_space(10.0);
                
                // 终端输出区域
                let terminal_height = ui.available_height().max(200.0);
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .max_height(terminal_height)
                    .show(ui, |ui| {
                        ui.set_min_height((terminal_height - 8.0).max(120.0));
                        let output = sess.terminal.get_formatted_output();
                        ui.label(egui::RichText::new(&output)
                            .family(egui::FontFamily::Monospace)
                            .color(egui::Color32::from_rgb(230, 235, 230)));
                    });
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("Select a session or click 'Connect' to start.").size(18.0));
            });
        }
    }

    fn render_connect_dialog(&mut self, ctx: &egui::Context) {
        egui::Window::new("Connect to Server")
            .resizable(true)
            .show(ctx, |ui| {
                ui.label("Name:");
                ui.text_edit_singleline(&mut self.new_config.name);
                
                ui.label("Host:");
                ui.text_edit_singleline(&mut self.new_config.host);
                
                ui.horizontal(|ui| {
                    ui.label("Port:");
                    ui.add(egui::DragValue::new(&mut self.new_config.port));
                });
                
                ui.label("Username:");
                ui.text_edit_singleline(&mut self.new_config.username);
                
                ui.label("Password:");
                ui.add(egui::TextEdit::singleline(&mut self.new_config.password).password(true));
                
                ui.separator();
                
                ui.horizontal(|ui| {
                    if ui.button("Connect").clicked() {
                        if self.new_config.host.is_empty() || self.new_config.username.is_empty() {
                            ui.label("Please fill in Host and Username");
                        } else {
                            let config = self.new_config.clone();
                            
                            // 保存到 SessionManager
                            self.session_manager.add_session(config.clone());
                            
                            // 添加到 ConnectionManager
                            if let Some(ref mut conn_mgr) = self.connection_manager {
                                let idx = conn_mgr.add_session(config.clone());
                                self.selected_session = Some(idx);
                            }
                            
                            self.showing_connect_dialog = false;
                            
                            // 连接
                            if let Some(idx) = self.selected_session {
                                self.connect_session(idx);
                            }
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.showing_connect_dialog = false;
                    }
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_fullwidth_punctuation() {
        let input = "ｖｉｍ　ａ。ｔｘｔ：ｗｑ！";
        let normalized = MistTermApp::normalize_terminal_text(input);
        assert_eq!(normalized, "vim a.txt:wq!");
    }

    #[test]
    fn key_symbol_mapping_basic_pairs() {
        assert_eq!(MistTermApp::key_to_symbol_bytes(egui::Key::Minus, false), Some(&b"-"[..]));
        assert_eq!(MistTermApp::key_to_symbol_bytes(egui::Key::Minus, true), Some(&b"_"[..]));
        assert_eq!(
            MistTermApp::key_to_symbol_bytes(egui::Key::PlusEquals, false),
            Some(&b"="[..])
        );
        assert_eq!(
            MistTermApp::key_to_symbol_bytes(egui::Key::PlusEquals, true),
            Some(&b"+"[..])
        );
    }

    #[test]
    fn key_symbol_mapping_shift_numbers() {
        assert_eq!(MistTermApp::key_to_symbol_bytes(egui::Key::Num1, true), Some(&b"!"[..]));
        assert_eq!(MistTermApp::key_to_symbol_bytes(egui::Key::Num2, true), Some(&b"@"[..]));
        assert_eq!(MistTermApp::key_to_symbol_bytes(egui::Key::Num9, true), Some(&b"("[..]));
        assert_eq!(MistTermApp::key_to_symbol_bytes(egui::Key::Num0, true), Some(&b")"[..]));
    }
}
