use eframe::egui;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use ssh2::Session;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

/// Session configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionConfig {
    name: String,
    host: String,
    port: u16,
    username: String,
}

/// App configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AppConfig {
    sessions: Vec<SessionConfig>,
}

/// SSH Session
struct SshSession {
    config: SessionConfig,
    connected: bool,
    session: Option<Session>,
    channel: Option<ssh2::Channel>,
    output: String,
    error: Option<String>,
    needs_password: bool,
}

impl SshSession {
    fn new(config: SessionConfig) -> Self {
        Self {
            config,
            connected: false,
            session: None,
            channel: None,
            output: String::new(),
            error: None,
            needs_password: false,
        }
    }

    fn connect_with_password(&mut self, password: &str) -> Result<(), String> {
        let tcp = std::net::TcpStream::connect(format!("{}:{}", self.config.host, self.config.port))
            .map_err(|e| format!("Failed to connect: {}", e))?;

        let mut session = Session::new().unwrap();
        session.set_tcp_stream(tcp);
        session.handshake().map_err(|e| format!("SSH handshake failed: {}", e))?;

        // Try agent authentication first
        if session.userauth_agent(&self.config.username).is_ok() {
            // Success with agent
        } else {
            // Try password
            session.userauth_password(&self.config.username, password)
                .map_err(|e| format!("Authentication failed: {}", e))?;
        }

        let channel = session.channel_session()
            .map_err(|e| format!("Failed to open channel: {}", e))?;

        self.session = Some(session);
        self.channel = Some(channel);
        self.connected = true;
        self.error = None;
        self.needs_password = false;
        self.output.push_str("Connected!\r\n");

        Ok(())
    }

    fn disconnect(&mut self) {
        self.channel = None;
        self.session = None;
        self.connected = false;
        self.output.push_str("Disconnected\r\n");
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn write(&mut self, data: &[u8]) -> Result<(), String> {
        if let Some(ref mut channel) = self.channel {
            channel.write_all(data).map_err(|e| format!("Write failed: {}", e))?;
            Ok(())
        } else {
            Err("Not connected".to_string())
        }
    }

    fn read(&mut self) -> Result<(), String> {
        if let Some(ref mut channel) = self.channel {
            let mut buf = [0u8; 4096];
            match channel.read(&mut buf) {
                Ok(0) => Ok(()),
                Ok(n) => {
                    if let Ok(text) = std::str::from_utf8(&buf[..n]) {
                        self.output.push_str(text);
                    }
                    Ok(())
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
                Err(e) => Err(format!("Read failed: {}", e)),
            }
        } else {
            Ok(())
        }
    }
}

/// Main App
struct MistTermApp {
    sessions: Vec<Arc<Mutex<SshSession>>>,
    active_idx: usize,
    input_buffer: String,
    config_path: PathBuf,
    show_connect_dialog: bool,
    show_password_dialog: bool,
    new_session_name: String,
    new_session_host: String,
    new_session_user: String,
    new_session_port: u16,
    password_input: String,
    message: String,
}

impl Default for MistTermApp {
    fn default() -> Self {
        let home_dir = std::env::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let config_path = home_dir.join(".mistterm").join("config.json");

        // Load config
        let config = if config_path.exists() {
            match fs::read_to_string(&config_path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(_) => AppConfig::default(),
            }
        } else {
            AppConfig::default()
        };

        let sessions: Vec<Arc<Mutex<SshSession>>> = config
            .sessions
            .iter()
            .map(|s| Arc::new(Mutex::new(SshSession::new(s.clone()))))
            .collect();

        Self {
            sessions,
            active_idx: 0,
            input_buffer: String::new(),
            config_path,
            show_connect_dialog: false,
            show_password_dialog: false,
            new_session_name: String::new(),
            new_session_host: String::new(),
            new_session_user: String::new(),
            new_session_port: 22,
            password_input: String::new(),
            message: "Welcome to MistTerm GUI".to_string(),
        }
    }
}

impl MistTermApp {
    fn save_config(&self) {
        let config = AppConfig {
            sessions: self.sessions.iter().map(|s| {
                let session = s.lock();
                SessionConfig {
                    name: session.config.name.clone(),
                    host: session.config.host.clone(),
                    port: session.config.port,
                    username: session.config.username.clone(),
                }
            }).collect(),
        };

        if let Some(parent) = self.config_path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        if let Ok(json) = serde_json::to_string_pretty(&config) {
            let _ = fs::write(&self.config_path, json);
        }
    }

    fn add_session(&mut self) {
        if !self.new_session_name.is_empty() && !self.new_session_host.is_empty() {
            let config = SessionConfig {
                name: self.new_session_name.clone(),
                host: self.new_session_host.clone(),
                port: self.new_session_port,
                username: self.new_session_user.clone(),
            };
            self.sessions.push(Arc::new(Mutex::new(SshSession::new(config))));
            self.save_config();
            self.new_session_name.clear();
            self.new_session_host.clear();
            self.new_session_user.clear();
            self.new_session_port = 22;
            self.show_connect_dialog = false;
            self.message = "Session added".to_string();
        }
    }

    fn remove_session(&mut self, idx: usize) {
        if idx < self.sessions.len() {
            self.sessions.remove(idx);
            if self.active_idx >= self.sessions.len() {
                self.active_idx = self.sessions.len().saturating_sub(1);
            }
            self.save_config();
        }
    }

    fn active_session(&self) -> Option<Arc<Mutex<SshSession>>> {
        self.sessions.get(self.active_idx).cloned()
    }
}

impl eframe::App for MistTermApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top bar
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.label("🌫️ MistTerm ");
                ui.separator();
                if ui.button("➕ New Session").clicked() {
                    self.show_connect_dialog = true;
                }
                if ui.button("🗑️ Remove Session").clicked() {
                    if !self.sessions.is_empty() {
                        self.remove_session(self.active_idx);
                        self.message = "Session removed".to_string();
                    }
                }
                ui.separator();
                ui.label(format!("Session {}/{}", self.active_idx + 1, self.sessions.len()));
            });
        });

        // Connect dialog
        if self.show_connect_dialog {
            egui::Window::new("New Session").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut self.new_session_name);
                });
                ui.horizontal(|ui| {
                    ui.label("Host:");
                    ui.text_edit_singleline(&mut self.new_session_host);
                });
                ui.horizontal(|ui| {
                    ui.label("User:");
                    ui.text_edit_singleline(&mut self.new_session_user);
                });
                ui.horizontal(|ui| {
                    ui.label("Port:");
                    ui.add(egui::DragValue::new(&mut self.new_session_port).min_decimals(0).max_decimals(0));
                });
                ui.horizontal(|ui| {
                    if ui.button("Add").clicked() {
                        self.add_session();
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_connect_dialog = false;
                    }
                });
            });
        }

        // Password dialog
        if self.show_password_dialog {
            egui::Window::new("Enter Password").show(ctx, |ui| {
                ui.label(format!("Password for {}@{}:", 
                    self.sessions[self.active_idx].lock().config.username,
                    self.sessions[self.active_idx].lock().config.host
                ));
                ui.horizontal(|ui| {
                    ui.label("Password:");
                    ui.add(egui::TextEdit::singleline(&mut self.password_input)
                        .password(true)
                        .desired_width(200.0));
                });
                ui.horizontal(|ui| {
                    if ui.button("Connect").clicked() {
                        let session = self.active_session();
                        if let Some(session_arc) = session {
                            let mut session_locked = session_arc.lock();
                            match session_locked.connect_with_password(&self.password_input) {
                                Ok(_) => {
                                    self.message = format!("Connected to {}", session_locked.config.host);
                                }
                                Err(e) => {
                                    session_locked.error = Some(e.clone());
                                    self.message = format!("Connection failed: {}", e);
                                }
                            }
                            drop(session_locked);
                        }
                        self.password_input.clear();
                        self.show_password_dialog = false;
                    }
                    if ui.button("Cancel").clicked() {
                        let session = self.active_session();
                        if let Some(session_arc) = session {
                            let mut session_locked = session_arc.lock();
                            session_locked.needs_password = false;
                            drop(session_locked);
                        }
                        self.password_input.clear();
                        self.show_password_dialog = false;
                    }
                });
            });
        }

        // Main terminal panel
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.sessions.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.heading("🌫️ MistTerm");
                    ui.label("No sessions configured");
                    ui.label("Click '➕ New Session' to add one");
                });
            } else {
                // Session tabs
                ui.horizontal(|ui| {
                    for (idx, session) in self.sessions.iter().enumerate() {
                        let session_locked = session.lock();
                        let connected = session_locked.is_connected();
                        let label = format!(
                            "{} {}",
                            if connected { "🟢" } else { "⚪" },
                            session_locked.config.name
                        );
                        drop(session_locked);

                        if ui.selectable_label(self.active_idx == idx, &label).clicked() {
                            self.active_idx = idx;
                        }
                    }
                });

                ui.add_space(10.0);

                // Terminal display
                let session = self.active_session();
                if let Some(session_arc) = session {
                    let session_locked = session_arc.lock();
                    let connected = session_locked.is_connected();
                    let error = session_locked.error.clone();
                    let output = session_locked.output.clone();
                    drop(session_locked);

                    // Terminal output area
                    ui.group(|ui| {
                        ui.heading(if connected {
                            format!("Connected to {}", self.sessions[self.active_idx].lock().config.host)
                        } else {
                            format!("Not connected: {}@{}", 
                                self.sessions[self.active_idx].lock().config.username,
                                self.sessions[self.active_idx].lock().config.host
                            )
                        });
                        
                        if let Some(err) = error {
                            ui.colored_label(egui::Color32::RED, format!("Error: {}", err));
                        }

                        ui.add_space(10.0);

                        // Terminal content
                        let mut output_text = output.clone();
                        egui::ScrollArea::vertical()
                            .show(ui, |ui| {
                                ui.add(egui::TextEdit::multiline(&mut output_text)
                                    .code_editor()
                                    .lock_focus(true));
                            });
                    });

                    ui.add_space(10.0);

                    // Input area
                    ui.horizontal(|ui| {
                        ui.label(">");
                        if ui.text_edit_singleline(&mut self.input_buffer).lost_focus() {
                            if !self.input_buffer.is_empty() {
                                let input = self.input_buffer.clone();
                                
                                // Handle commands
                                if input.starts_with(':') {
                                    let cmd = &input[1..];
                                    if cmd == "connect" || cmd == "c" {
                                        let mut session_locked = session_arc.lock();
                                        // Check if we need password
                                        session_locked.needs_password = true;
                                        drop(session_locked);
                                        
                                        self.show_password_dialog = true;
                                        self.message = "Enter password to connect".to_string();
                                    } else if cmd == "disconnect" || cmd == "d" {
                                        let mut session_locked = session_arc.lock();
                                        session_locked.disconnect();
                                        self.message = "Disconnected".to_string();
                                        drop(session_locked);
                                    } else if cmd == "help" {
                                        self.message = ":connect/:c | :disconnect/:d | :help".to_string();
                                    }
                                } else {
                                    // Send to SSH
                                    let mut session_locked = session_arc.lock();
                                    if session_locked.is_connected() {
                                        let cmd_with_newline = format!("{}\n", input);
                                        if let Err(e) = session_locked.write(cmd_with_newline.as_bytes()) {
                                            self.message = format!("Send failed: {}", e);
                                        } else {
                                            self.message = format!("Sent: {}", input);
                                        }
                                    } else {
                                        self.message = "Not connected. Use :connect or :c".to_string();
                                    }
                                    drop(session_locked);
                                }
                                
                                self.input_buffer.clear();
                            }
                        }
                    });

                    // Status bar
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label(self.message.clone());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label("Ctrl+Q to quit");
                        });
                    });
                }
            }
        });

        // Handle keyboard shortcuts
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Q) && i.modifiers.ctrl {
                // Close window
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        ..Default::default()
    };

    eframe::run_native(
        "MistTerm",
        options,
        Box::new(|_cc| Box::new(MistTermApp::default())),
    )
}
