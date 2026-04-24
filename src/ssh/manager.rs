//! SSH 管理器 - 管理多个 SSH 会话

use super::client::{SshClient, SshConfig};
use ssh2::Channel;
use std::collections::HashMap;
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;
use std::io::{Write, Read};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// SSH 会话 ID
pub type SshSessionId = usize;

/// SSH 消息类型
#[derive(Debug, Clone)]
pub enum SshMessage {
    /// 终端输出数据
    Output {
        session_id: SshSessionId,
        data: Vec<u8>,
    },
    /// 连接成功
    Connected {
        session_id: SshSessionId,
    },
    /// 连接错误
    Error {
        session_id: SshSessionId,
        error: String,
    },
    /// 断开连接
    Disconnected {
        session_id: SshSessionId,
    },
}

/// SSH 会话句柄
pub struct SshSessionHandle {
    pub session_id: SshSessionId,
    input_tx: Sender<Vec<u8>>,
    resize_tx: Sender<(u32, u32)>,
}

impl SshSessionHandle {
    /// 发送输入数据
    pub fn send_input(&self, data: &[u8]) -> Result<(), String> {
        self.input_tx.send(data.to_vec())
            .map_err(|e| format!("Send failed: {}", e))
    }

    /// 通知远端 PTY 尺寸变化
    pub fn resize_pty(&self, cols: u32, rows: u32) -> Result<(), String> {
        let cols = cols.clamp(20, 512);
        let rows = rows.clamp(5, 256);
        self.resize_tx
            .send((cols, rows))
            .map_err(|e| format!("Resize send failed: {}", e))
    }
}

/// SSH 管理器
pub struct SshManager {
    sessions: Arc<Mutex<HashMap<SshSessionId, SshClient>>>,
    message_tx: Sender<SshMessage>,
    next_session_id: Arc<AtomicUsize>,
}

impl Clone for SshManager {
    fn clone(&self) -> Self {
        Self {
            sessions: self.sessions.clone(),
            message_tx: self.message_tx.clone(),
            next_session_id: self.next_session_id.clone(),
        }
    }
}

impl SshManager {
    fn allocate_session_id(&self) -> SshSessionId {
        self.next_session_id.fetch_add(1, Ordering::Relaxed)
    }

    fn is_retryable_write_error(err: &std::io::Error) -> bool {
        let msg = err.to_string().to_lowercase();
        err.kind() == std::io::ErrorKind::WouldBlock
            || msg.contains("would block")
            || msg.contains("draining incoming flow")
    }

    fn is_retryable_read_error(err: &std::io::Error) -> bool {
        let msg = err.to_string().to_lowercase();
        err.kind() == std::io::ErrorKind::WouldBlock
            || msg.contains("would block")
            || msg.contains("transport read")
            || msg.contains("resource temporarily unavailable")
    }

    /// 创建新的 SSH 管理器
    pub fn new() -> (Self, Receiver<SshMessage>) {
        let (tx, rx) = mpsc::channel();
        
        let manager = Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            message_tx: tx,
            next_session_id: Arc::new(AtomicUsize::new(0)),
        };
        
        (manager, rx)
    }

    /// 创建新的 SSH 连接（异步）
    pub fn create_session_async(&self, config: SshConfig) -> Result<SshSessionId, String> {
        let session_id = self.allocate_session_id();
        
        let sessions = self.sessions.clone();
        let message_tx = self.message_tx.clone();
        
        thread::spawn(move || {
            let mut client = SshClient::new(config);
            
            match client.connect() {
                Ok(_) => {
                    {
                        let mut sess_list = sessions.lock().unwrap();
                        sess_list.insert(session_id, client);
                    }
                    let _ = message_tx.send(SshMessage::Connected { session_id });
                    log::info!("Session {} connected successfully", session_id);
                }
                Err(e) => {
                    log::error!("Session {} connection failed: {}", session_id, e);
                    let _ = message_tx.send(SshMessage::Error {
                        session_id,
                        error: e,
                    });
                }
            }
        });
        
        Ok(session_id)
    }

    /// 启动会话的交互式 shell
    pub fn start_interactive_shell(
        &self,
        session_id: SshSessionId,
        initial_cols: u32,
        initial_rows: u32,
    ) -> Result<SshSessionHandle, String> {
        let message_tx = self.message_tx.clone();
        let sessions = self.sessions.clone();
        
        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>();
        let (resize_tx, resize_rx) = mpsc::channel::<(u32, u32)>();
        
        let channel = {
            let mut sessions = sessions.lock().unwrap();
            let session = sessions
                .get_mut(&session_id)
                .ok_or_else(|| format!("Session {} not found", session_id))?;
            if !session.is_connected() {
                return Err(format!("Session {} is not connected", session_id));
            }
            session.open_shell(initial_cols, initial_rows)?
        };
        
        thread::spawn(move || {
            let mut read_buffer = [0u8; 4096];
            let mut channel = channel;
            let mut pty_cols = initial_cols.clamp(20, 512);
            let mut pty_rows = initial_rows.clamp(5, 256);
            let mut last_progress = Instant::now();

            loop {
                while let Ok((c, r)) = resize_rx.try_recv() {
                    pty_cols = c.clamp(20, 512);
                    pty_rows = r.clamp(5, 256);
                    // 简化：暂不实现动态调整 PTY 大小
                    log::debug!("Resize to {}x{}", pty_cols, pty_rows);
                }

                while let Ok(data) = input_rx.try_recv() {
                    if let Err(e) = channel.write_all(&data) {
                        log::error!("Write error: {}", e);
                    }
                    let _ = channel.flush();
                }

                match channel.read(&mut read_buffer) {
                    Ok(0) => {
                        let _ = message_tx.send(SshMessage::Disconnected { session_id });
                        return;
                    }
                    Ok(n) => {
                        let _ = message_tx.send(SshMessage::Output {
                            session_id,
                            data: read_buffer[..n].to_vec(),
                        });
                        last_progress = Instant::now();
                    }
                    Err(e) => {
                        if Self::is_retryable_read_error(&e) {
                            thread::sleep(Duration::from_millis(16));
                            continue;
                        }
                        let _ = message_tx.send(SshMessage::Error {
                            session_id,
                            error: format!("Read error: {}", e),
                        });
                        return;
                    }
                }
            }
        });
        
        Ok(SshSessionHandle {
            session_id,
            input_tx,
            resize_tx,
        })
    }

    /// 获取会话数量
    pub fn session_count(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }
}
