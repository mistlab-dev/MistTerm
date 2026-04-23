//! SSH 管理器 - 管理多个 SSH 会话

use super::client::{SshClient, SshConfig};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{Sender, Receiver};
use std::sync::mpsc;
use std::io::{Write, Read};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

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
}

impl SshSessionHandle {
    /// 发送输入数据
    pub fn send_input(&self, data: &[u8]) -> Result<(), String> {
        log::info!(
            "[SSH-HANDLE] Queue input for session {} ({} bytes)",
            self.session_id,
            data.len()
        );
        self.input_tx.send(data.to_vec())
            .map_err(|e| format!("Send failed: {}", e))
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
    pub fn create_session_async(&mut self, config: SshConfig) -> Result<SshSessionId, String> {
        let session_id = self.allocate_session_id();
        
        let sessions = self.sessions.clone();
        let message_tx = self.message_tx.clone();
        
        // 在后台线程中执行连接
        thread::spawn(move || {
            let mut client = SshClient::new(config);
            
            match client.connect() {
                Ok(_) => {
                    // 连接成功，加入会话列表
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
    pub fn start_interactive_shell(&self, session_id: SshSessionId) -> Result<SshSessionHandle, String> {
        let message_tx = self.message_tx.clone();
        
        let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>();
        
        // 获取已连接的 session 并克隆 channel
        let channel = {
            let mut sessions = self.sessions.lock().unwrap();
            let session = sessions
                .get_mut(&session_id)
                .ok_or_else(|| format!("Session {} not found", session_id))?;
            if !session.is_connected() {
                return Err(format!("Session {} is not connected", session_id));
            }
            
            // 打开 shell 通道
            let channel = session.open_shell()?;
            log::info!("Shell channel opened for session {}", session_id);
            channel
        };
        
        // 在后台线程中处理读写
        thread::spawn(move || {
            let mut read_buffer = [0u8; 4096];
            let mut channel = channel;
            let mut pending_writes: VecDeque<Vec<u8>> = VecDeque::new();

            loop {
                // 处理输入（非阻塞）
                while let Ok(data) = input_rx.try_recv() {
                    log::info!(
                        "[SSH-IO] Session {} dequeued input ({} bytes)",
                        session_id,
                        data.len()
                    );
                    pending_writes.push_back(data);
                }

                while let Some(front) = pending_writes.front_mut() {
                    match channel.write(front) {
                        Ok(0) => {
                            let _ = message_tx.send(SshMessage::Disconnected { session_id });
                            return;
                        }
                        Ok(n) => {
                            log::info!(
                                "[SSH-IO] Session {} wrote {} bytes to remote channel",
                                session_id,
                                n
                            );
                            if n >= front.len() {
                                pending_writes.pop_front();
                            } else {
                                front.drain(..n);
                                break;
                            }
                            let _ = channel.flush();
                        }
                        Err(e) => {
                            if Self::is_retryable_write_error(&e) {
                                // 可重试错误：先去读远端数据，再下一轮重试写入
                                break;
                            }
                            let _ = message_tx.send(SshMessage::Error {
                                session_id,
                                error: format!("Write error: {}", e),
                            });
                            return;
                        }
                    }
                }

                // 读取输出（非阻塞）
                loop {
                    match channel.read(&mut read_buffer) {
                        Ok(0) => {
                            let _ = message_tx.send(SshMessage::Disconnected { session_id });
                            return;
                        }
                        Ok(n) => {
                            let output = read_buffer[..n].to_vec();
                            let preview = String::from_utf8_lossy(&output)
                                .replace('\r', "\\r")
                                .replace('\n', "\\n");
                            log::info!(
                                "[SSH-IO] Session {} read {} bytes from remote: {}",
                                session_id,
                                n,
                                preview
                            );
                            let _ = message_tx.send(SshMessage::Output {
                                session_id,
                                data: output,
                            });
                        }
                        Err(e) => {
                            if Self::is_retryable_read_error(&e) {
                                // 非阻塞读下常见可重试错误，不应中断会话
                                break;
                            }
                            let _ = message_tx.send(SshMessage::Error {
                                session_id,
                                error: format!("Read error: {}", e),
                            });
                            return;
                        }
                    }
                }

                thread::sleep(Duration::from_millis(16));
            }
        });
        
        Ok(SshSessionHandle {
            session_id,
            input_tx,
        })
    }

    /// 获取会话数量
    pub fn session_count(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::client::SshConfig;

    #[test]
    fn test_manager_creation() {
        let (manager, _rx) = SshManager::new();
        assert_eq!(manager.session_count(), 0);
    }

    #[test]
    fn test_manager_clone() {
        let (manager, _rx) = SshManager::new();
        let cloned = manager.clone();
        assert_eq!(cloned.session_count(), 0);
    }

    #[test]
    fn test_session_id_counter_shared_across_clones() {
        let (manager, _rx) = SshManager::new();
        let cloned = manager.clone();

        let id0 = manager.allocate_session_id();
        let id1 = cloned.allocate_session_id();
        let id2 = manager.allocate_session_id();

        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn test_retryable_write_error_detection() {
        let would_block = std::io::Error::new(std::io::ErrorKind::WouldBlock, "would block");
        assert!(SshManager::is_retryable_write_error(&would_block));

        let draining = std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failure while draining incoming flow",
        );
        assert!(SshManager::is_retryable_write_error(&draining));

        let fatal = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "connection reset");
        assert!(!SshManager::is_retryable_write_error(&fatal));
    }

    #[test]
    fn test_retryable_read_error_detection() {
        let would_block = std::io::Error::new(std::io::ErrorKind::WouldBlock, "would block");
        assert!(SshManager::is_retryable_read_error(&would_block));

        let transport_read =
            std::io::Error::new(std::io::ErrorKind::Other, "transport read error");
        assert!(SshManager::is_retryable_read_error(&transport_read));

        let temporary = std::io::Error::new(
            std::io::ErrorKind::Other,
            "resource temporarily unavailable",
        );
        assert!(SshManager::is_retryable_read_error(&temporary));

        let fatal = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "broken pipe");
        assert!(!SshManager::is_retryable_read_error(&fatal));
    }

    #[test]
    fn test_session_storage_uses_session_id_key_not_index() {
        let (manager, _rx) = SshManager::new();
        let mut sessions = manager.sessions.lock().unwrap();

        // 模拟“前一个 session 失败没有入表，后一个 session 成功入表”的跳号场景
        let cfg = SshConfig {
            host: "127.0.0.1".to_string(),
            port: 22,
            username: "u".to_string(),
            password: "p".to_string(),
        };
        sessions.insert(3, SshClient::new(cfg));

        // 若按 Vec 索引逻辑会误以为 "3 out of range"；HashMap 键控应能命中
        assert!(sessions.get(&3).is_some());
        assert!(sessions.get(&0).is_none());
        assert_eq!(sessions.len(), 1);
    }
}
