//! MistTerm - 异步 SSH 终端

mod ssh;

use eframe::egui;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc as sync_mpsc;
use std::thread;
use std::time::Duration;
use std::fs;
use std::path::PathBuf;
use ssh::{SshConfig, SshManager, SshMessage};

const SESSIONS_FILE: &str = "sessions.json";

/// 会话配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            name: "New Session".to_string(),
            host: "localhost".to_string(),
            port: 22,
            username: String::new(),
            password: String::new(),
        }
    }
}

/// 会话输出
#[derive(Debug, Clone, Default)]
pub struct SessionOutput {
    pub lines: Vec<String>,
    pub current_line: String,
}

/// SSH 会话状态
pub struct SshSessionState {
    pub config: SessionConfig,
    pub connected: bool,
    pub connecting: bool,
    pub error: Option<String>,
    pub output: SessionOutput,
    pub handle: Option<ssh::SshSessionHandle>,
}

impl SshSessionState {
    pub fn new(config: SessionConfig) -> Self {
        Self {
            config,
            connected: false,
            connecting: true,
            error: None,
            output: SessionOutput::default(),
            handle: None,
        }
    }
}

/// 应用状态
struct MistTermApp {
    sessions: Vec<Arc<Mutex<SshSessionState>>>,
    selected_session: Option<usize>,
    showing_connect_dialog: bool,
    new_config: SessionConfig,
    message_rx: Option<sync_mpsc::Receiver<SshMessage>>,
    ssh_manager: Option<SshManager>,
    input_buffer: HashMap<String, String>,
}

impl Default for MistTermApp {
    fn default() -> Self {
        let (manager, rx) = SshManager::new();
        let mut app = Self {
            sessions: Vec::new(),
            selected_session: None,
            showing_connect_dialog: false,
            new_config: SessionConfig::default(),
            message_rx: Some(rx),
            ssh_manager: Some(manager),
            input_buffer: HashMap::new(),
        };
        app.load_sessions();
        app
    }
}

impl MistTermApp {
    fn get_input_text(&mut self, key: &str) -> String {
        self.input_buffer.get(key).cloned().unwrap_or_default()
    }
    
    fn set_input_text(&mut self, key: &str, text: &str) {
        self.input_buffer.insert(key.to_string(), text.to_string());
    }
    
    fn get_sessions_path() -> PathBuf {
        let mut path = std::env::current_dir().unwrap_or_default();
        path.push(SESSIONS_FILE);
        path
    }
    
    fn load_sessions(&mut self) {
        let path = Self::get_sessions_path();
        if !path.exists() {
            return;
        }
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(configs) = serde_json::from_str::<Vec<SessionConfig>>(&content) {
                for config in configs {
                    let session_state = SshSessionState::new(config);
                    self.sessions.push(Arc::new(Mutex::new(session_state)));
                }
                log::info!("Loaded {} saved sessions", self.sessions.len());
            }
        }
    }
    
    fn save_sessions(&self) {
        let path = Self::get_sessions_path();
        let configs: Vec<SessionConfig> = self.sessions.iter().map(|s| s.lock().config.clone()).collect();
        if let Ok(content) = serde_json::to_string_pretty(&configs) {
            let _ = fs::write(&path, content);
            log::info!("Saved {} sessions", configs.len());
        }
    }
}

impl eframe::App for MistTermApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 处理 SSH 消息
        if let Some(ref rx) = self.message_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    SshMessage::Output(data) => {
                        if let Some(idx) = self.selected_session {
                            if let Some(session) = self.sessions.get(idx) {
                                let mut sess = session.lock();
                                let output_str = String::from_utf8_lossy(&data);
                                let current_line = std::mem::take(&mut sess.output.current_line);
                                let new_line = current_line + &output_str;
                                if new_line.contains('\n') {
                                    let parts: Vec<&str> = new_line.split('\n').collect();
                                    if parts.len() > 1 {
                                        for part in &parts[..parts.len()-1] {
                                            sess.output.lines.push(part.to_string());
                                        }
                                        sess.output.current_line = parts.last().unwrap().to_string();
                                    }
                                } else {
                                    sess.output.current_line = new_line;
                                }
                                if sess.output.lines.len() > 1000 {
                                    sess.output.lines.drain(..500);
                                }
                            }
                        }
                    }
                    SshMessage::Connected => {}
                    SshMessage::Error(e) => {
                        if let Some(idx) = self.selected_session {
                            if let Some(session) = self.sessions.get(idx) {
                                let mut sess = session.lock();
                                sess.connecting = false;
                                sess.error = Some(e);
                            }
                        }
                    }
                    SshMessage::Disconnected => {
                        if let Some(idx) = self.selected_session {
                            if let Some(session) = self.sessions.get(idx) {
                                let mut sess = session.lock();
                                sess.connected = false;
                                sess.connecting = false;
                            }
                        }
                    }
                }
            }
        }
        
        // 主界面
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("MistTerm - SSH Terminal");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Save Sessions").clicked() {
                        self.save_sessions();
                    }
                    if ui.button("Connect").clicked() {
                        self.showing_connect_dialog = true;
                        self.new_config = SessionConfig::default();
                    }
                });
            });
            
            ui.separator();
            ui.label("Sessions:");
            
            if self.sessions.is_empty() {
                ui.label("No sessions. Click 'Connect' to add one.");
            }
            
            let mut delete_idx: Option<usize> = None;
            let mut select_idx: Option<usize> = None;
            
            for (idx, session) in self.sessions.iter().enumerate() {
                let sess = session.lock();
                let status = if sess.connected { "Connected" }
                    else if sess.connecting { "Connecting..." }
                    else if sess.error.is_some() { "Error" }
                    else { "Disconnected" };
                
                ui.horizontal(|ui| {
                    if ui.selectable_label(self.selected_session == Some(idx), 
                        format!("{} - {} ({})", sess.config.name, sess.config.host, status)).clicked() {
                        select_idx = Some(idx);
                    }
                    if sess.connected && ui.small_button("X").clicked() {
                        delete_idx = Some(idx);
                    }
                });
            }
            
            if let Some(idx) = delete_idx {
                self.sessions.remove(idx);
                if self.selected_session == Some(idx) { self.selected_session = None; }
            }
            if let Some(idx) = select_idx {
                self.selected_session = Some(idx);
            }
            
            ui.separator();
            
            if let Some(idx) = self.selected_session {
                if let Some(session) = self.sessions.get(idx) {
                    let sess = session.lock();
                    
                    ui.horizontal(|ui| {
                        ui.label(format!("{}@{}:{}", sess.config.username, sess.config.host, sess.config.port));
                        if sess.connected {
                            ui.label(egui::RichText::new("✓ Connected").color(egui::Color32::GREEN));
                        } else if sess.connecting {
                            ui.label(egui::RichText::new("Connecting...").color(egui::Color32::YELLOW));
                        }
                        if let Some(err) = &sess.error {
                            ui.label(egui::RichText::new(format!("Error: {}", err)).color(egui::Color32::RED));
                        }
                    });
                    
                    ui.add_space(10.0);
                    
                    egui::Frame::none().fill(egui::Color32::from_rgb(30, 30, 30)).inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
                        egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                            let mut output = String::new();
                            for line in &sess.output.lines {
                                output.push_str(line);
                                output.push('\n');
                            }
                            output.push_str(&sess.output.current_line);
                            ui.add(egui::Label::new(egui::RichText::new(&output).family(egui::FontFamily::Monospace).color(egui::Color32::LIGHT_GREEN)).wrap(false));
                        });
                    });
                    
                    ui.add_space(10.0);
                    drop(sess);
                    
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("➤ ").size(18.0).color(egui::Color32::GREEN));
                        let input_key = format!("input_{}", idx);
                        let mut input = self.get_input_text(&input_key);
                        let resp = ui.text_edit_singleline(&mut input);
                        
                        // 按 Enter 发送命令
                        if resp.gained_focus() || resp.has_focus() {
                            if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !input.is_empty() {
                                if let Some(s) = self.sessions.get(idx) {
                                    let sess = s.lock();
                                    if let Some(h) = &sess.handle {
                                        let cmd = format!("{}\n", input);
                                        log::info!("[UI-INPUT] Sending command: {:?}", input);
                                        let _ = h.send_input(cmd.as_bytes());
                                    }
                                }
                                self.set_input_text(&input_key, "");
                            }
                        }
                        self.set_input_text(&input_key, &input);
                    });
                }
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new("Select a session or click 'Connect' to start.").size(18.0));
                });
            }
        });
        
        if self.showing_connect_dialog {
            egui::Window::new("Connect to Server").resizable(true).show(ctx, |ui| {
                ui.label("Name:"); ui.text_edit_singleline(&mut self.new_config.name);
                ui.label("Host:"); ui.text_edit_singleline(&mut self.new_config.host);
                ui.horizontal(|ui| { ui.label("Port:"); ui.add(egui::DragValue::new(&mut self.new_config.port)); });
                ui.label("Username:"); ui.text_edit_singleline(&mut self.new_config.username);
                ui.label("Password:"); ui.text_edit_singleline(&mut self.new_config.password);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Connect").clicked() {
                        if self.new_config.host.is_empty() || self.new_config.username.is_empty() {
                            ui.label("Please fill in Host and Username");
                        } else {
                            let config = self.new_config.clone();
                            let session_state = SshSessionState::new(config.clone());
                            let session_state = Arc::new(Mutex::new(session_state));
                            let session_idx = self.sessions.len();
                            self.sessions.push(session_state);
                            self.selected_session = Some(session_idx);
                            self.showing_connect_dialog = false;
                            self.save_sessions();
                            if let Some(ref mut manager) = self.ssh_manager {
                                match manager.create_session_async(SshConfig {
                                    name: config.name.clone(), host: config.host.clone(),
                                    port: config.port, username: config.username.clone(),
                                    password: config.password.clone(),
                                }) {
                                    Ok(session_id) => {
                                        thread::sleep(Duration::from_millis(500));
                                        if let Ok(handle) = manager.start_interactive_shell(session_id) {
                                            if let Some(session) = self.sessions.get(session_idx) {
                                                let mut sess = session.lock();
                                                sess.handle = Some(handle);
                                                sess.connected = true;
                                                sess.connecting = false;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        if let Some(session) = self.sessions.get(session_idx) {
                                            let mut sess = session.lock();
                                            sess.connecting = false;
                                            sess.error = Some(e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if ui.button("Cancel").clicked() { self.showing_connect_dialog = false; }
                });
            });
        }
    }
}

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let options = eframe::NativeOptions {
        maximized: true,
        ..Default::default()
    };
    eframe::run_native("MistTerm", options, Box::new(|_cc| Box::new(MistTermApp::default())))
}
