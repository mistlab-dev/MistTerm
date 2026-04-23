//! SSH 模块 - 独立的 SSH 连接和会话管理

use ssh2::Session;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// SSH 配置
#[derive(Debug, Clone)]
pub struct SshConfig {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            host: String::new(),
            port: 22,
            username: String::new(),
            password: String::new(),
        }
    }
}

/// SSH 会话消息
#[derive(Debug, Clone)]
pub enum SshMessage {
    /// 接收到的输出数据
    Output(Vec<u8>),
    /// 连接成功
    Connected,
    /// 连接失败
    Error(String),
    /// 断开连接
    Disconnected,
}

/// SSH 会话句柄
pub struct SshSessionHandle {
    session_id: usize,
    input_tx: Sender<Vec<u8>>,
}

impl SshSessionHandle {
    pub fn send_input(&self, data: &[u8]) -> Result<(), String> {
        self.input_tx.send(data.to_vec())
            .map_err(|e| format!("Failed to send input: {}", e))
    }
}

/// SSH 会话
pub struct SshSession {
    pub session_id: usize,
    pub config: SshConfig,
    session: Option<Session>,
    connected: bool,
    input_tx: Option<Sender<Vec<u8>>>,
}

impl SshSession {
    pub fn new(session_id: usize, config: SshConfig) -> Self {
        Self {
            session_id,
            config,
            session: None,
            connected: false,
            input_tx: None,
        }
    }

    /// 连接到 SSH 服务器
    pub fn connect(&mut self) -> Result<(), String> {
        let addr = format!("{}:{}", self.config.host, self.config.port);
        
        // 建立 TCP 连接
        let stream = TcpStream::connect(&addr)
            .map_err(|e| format!("TCP connection failed: {}", e))?;
        
        stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
        
        // 创建 SSH 会话
        let mut session = Session::new()
            .map_err(|e| format!("Failed to create SSH session: {}", e))?;
        
        session.set_tcp_stream(stream);
        
        // SSH 握手
        session.handshake()
            .map_err(|e| format!("SSH handshake failed: {}", e))?;
        
        // 尝试密钥认证
        let private_key_path = std::path::Path::new("/Users/tianguangyu/.ssh/id_rsa");
        if session.userauth_pubkey_file(&self.config.username, None, private_key_path, None).is_ok() {
            log::info!("Session {} authenticated with SSH key", self.session_id);
        } else {
            // 密码认证
            session.userauth_password(&self.config.username, &self.config.password)
                .map_err(|e| format!("Authentication failed: {}", e))?;
            log::info!("Session {} authenticated with password", self.session_id);
        }
        
        self.session = Some(session);
        self.connected = true;
        
        Ok(())
    }

    /// 打开交互式 shell 通道
    pub fn open_shell_channel(&mut self) -> Result<ssh2::Channel, String> {
        let session = self.session.as_mut()
            .ok_or("Not connected")?;
        
        let mut channel = session.channel_session()
            .map_err(|e| format!("Failed to open channel: {}", e))?;
        
        // 请求 PTY
        channel.request_pty("xterm-256color", None, Some((80, 24, 800, 600)))
            .map_err(|e| format!("Failed to request PTY: {}", e))?;
        
        // 启动 shell
        channel.shell()
            .map_err(|e| format!("Failed to start shell: {}", e))?;
        
        Ok(channel)
    }

    /// 执行单条命令
    pub fn execute(&mut self, command: &str) -> Result<String, String> {
        let session = self.session.as_mut()
            .ok_or("Not connected")?;
        
        let mut channel = session.channel_session()
            .map_err(|e| format!("Failed to open channel: {}", e))?;
        
        channel.exec(command)
            .map_err(|e| format!("Exec failed: {}", e))?;
        
        let mut output = String::new();
        channel.read_to_string(&mut output)
            .map_err(|e| format!("Read failed: {}", e))?;
        
        Ok(output)
    }

    /// 断开连接
    pub fn disconnect(&mut self) {
        self.connected = false;
        self.session = None;
    }
}

/// SSH 管理器 - 管理多个 SSH 会话
pub struct SshManager {
    sessions: Arc<Mutex<Vec<SshSession>>>,
    message_tx: Sender<SshMessage>,
    next_session_id: usize,
}

impl SshManager {
    pub fn new() -> (Self, Receiver<SshMessage>) {
        let (tx, rx) = mpsc::channel();
        
        let manager = Self {
            sessions: Arc::new(Mutex::new(Vec::new())),
            message_tx: tx,
            next_session_id: 0,
        };
        
        (manager, rx)
    }

    /// 创建新的 SSH 连接（异步，在后台线程执行）
    pub fn create_session_async(&mut self, config: SshConfig) -> Result<usize, String> {
        let session_id = self.next_session_id;
        self.next_session_id += 1;
        
        let sessions = self.sessions.clone();
        let message_tx = self.message_tx.clone();
        
        // 在后台线程中执行连接
        let sessions_clone = sessions.clone();
        thread::spawn(move || {
            let mut session = SshSession::new(session_id, config);
            
            // 发送连接开始消息
            let _ = message_tx.send(SshMessage::Connected);
            
            match session.connect() {
                Ok(_) => {
                    // 连接成功，加入会话列表
                    {
                        let mut sess_list = sessions_clone.lock().unwrap();
                        sess_list.push(session);
                    }
                    log::info!("Session {} connected successfully", session_id);
                }
                Err(e) => {
                    log::error!("Session {} connection failed: {}", session_id, e);
                    let _ = message_tx.send(SshMessage::Error(e));
                }
            }
        });
        
        Ok(session_id)
    }

    /// 启动会话的交互式 shell
    pub fn start_interactive_shell(&self, session_id: usize) -> Result<SshSessionHandle, String> {
        let _sessions = self.sessions.clone();
        let message_tx = self.message_tx.clone();
        
        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>();
        
        // 克隆 session
        let session_data = {
            let sessions = self.sessions.lock().unwrap();
            sessions.iter()
                .find(|s| s.session_id == session_id)
                .map(|s| s.config.clone())
                .ok_or("Session not found")?
        };
        
        thread::spawn(move || {
            // 重新建立连接
            let mut session = SshSession::new(session_id, session_data);
            if let Err(e) = session.connect() {
                let _ = message_tx.send(SshMessage::Error(format!("Reconnect failed: {}", e)));
                return;
            }
            
            // 打开 shell 通道
            let mut channel = match session.open_shell_channel() {
                Ok(c) => c,
                Err(e) => {
                    let _ = message_tx.send(SshMessage::Error(format!("Shell failed: {}", e)));
                    return;
                }
            };
            
            // 设置非阻塞读取
            let mut read_buffer = [0u8; 4096];
            
            // 读取和输入在同一循环中处理
            while let Ok(data) = input_rx.recv_timeout(Duration::from_millis(100)) {
                // 先处理输入
                if let Err(e) = channel.write_all(&data) {
                    let _ = message_tx.send(SshMessage::Error(format!("Write error: {}", e)));
                    break;
                }
                channel.flush().ok();
                
                // 然后读取输出
                match channel.read(&mut read_buffer) {
                    Ok(0) => {
                        let _ = message_tx.send(SshMessage::Disconnected);
                        break;
                    }
                    Ok(n) => {
                        let output = read_buffer[..n].to_vec();
                        let _ = message_tx.send(SshMessage::Output(output));
                    }
                    Err(e) => {
                        if e.kind() != std::io::ErrorKind::WouldBlock {
                            let _ = message_tx.send(SshMessage::Error(format!("Read error: {}", e)));
                            break;
                        }
                    }
                }
            }
        });
        
        Ok(SshSessionHandle {
            session_id,
            input_tx,
        })
    }

    /// 获取会话列表
    pub fn list_sessions(&self) -> Vec<usize> {
        let sessions = self.sessions.lock().unwrap();
        sessions.iter().map(|s| s.session_id).collect()
    }

    /// 删除会话
    pub fn remove_session(&self, session_id: usize) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|s| s.session_id != session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_connection() {
        let config = SshConfig {
            name: "Test Server".to_string(),
            host: "124.220.224.223".to_string(),
            port: 22,
            username: "ubuntu".to_string(),
            password: "Tian1234".to_string(),
        };
        
        let mut session = SshSession::new(0, config);
        match session.connect() {
            Ok(_) => println!("SSH connection successful!"),
            Err(e) => println!("SSH connection failed: {}", e),
        }
    }
}
