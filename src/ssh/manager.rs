//! SSH 管理器 - 管理多个 SSH 会话

use super::client::{SshClient, SshConfig};
use ssh2::Channel;
use std::collections::{HashMap, VecDeque};
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

/// `drain_channel_reads_nonblocking` 的结束原因（EOF / 致命读错误）
enum SshReadStop {
    Disconnected,
    Fatal(String),
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

    /// 通知远端 PTY 尺寸变化（列、行），与 UI 终端网格一致
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

    /// 写队列有积压且超过 2s 无读写进展时，按「每多停滞约 2s」升一级 strike（避免 16ms 轮询刷满阈值）。
    fn write_backlog_strike_level(has_pending: bool, since_progress: Duration) -> usize {
        if !has_pending || since_progress <= Duration::from_secs(2) {
            return 0;
        }
        ((since_progress.as_secs() / 2) as usize).max(1)
    }

    /// 非阻塞下读尽 channel 上当前可读数据（含促使 libssh2 消化入站包），直到 WouldBlock。
    fn drain_channel_reads_nonblocking(
        channel: &mut Channel,
        read_buffer: &mut [u8],
        message_tx: &Sender<SshMessage>,
        session_id: SshSessionId,
        last_progress: &mut Instant,
    ) -> Result<(), SshReadStop> {
        loop {
            match channel.read(read_buffer) {
                Ok(0) => return Err(SshReadStop::Disconnected),
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
                    *last_progress = Instant::now();
                }
                Err(e) => {
                    if Self::is_retryable_read_error(&e) {
                        return Ok(());
                    }
                    return Err(SshReadStop::Fatal(format!("Read error: {}", e)));
                }
            }
        }
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

    /// 启动会话的交互式 shell（`initial_cols`/`initial_rows` 为 PTY 字符网格初始大小）
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
        
        // 获取已连接的 session 并克隆 channel
        let channel = {
            let mut sessions = sessions.lock().unwrap();
            let session = sessions
                .get_mut(&session_id)
                .ok_or_else(|| format!("Session {} not found", session_id))?;
            if !session.is_connected() {
                return Err(format!("Session {} is not connected", session_id));
            }
            
            // 打开 shell 通道
            let channel = session.open_shell(initial_cols, initial_rows)?;
            log::info!(
                "Shell channel opened for session {} (pty {}x{})",
                session_id,
                initial_cols,
                initial_rows
            );
            channel
        };
        
        // 在后台线程中处理读写
        thread::spawn(move || {
            const MAX_PENDING_CHUNKS: usize = 256;
            const MAX_PENDING_BYTES: usize = 64 * 1024;
            const STALL_REOPEN_THRESHOLD: usize = 8;

            let mut read_buffer = [0u8; 4096];
            let mut channel = channel;
            let mut pty_cols = initial_cols.clamp(20, 512);
            let mut pty_rows = initial_rows.clamp(5, 256);
            let mut pending_writes: VecDeque<Vec<u8>> = VecDeque::new();
            let mut pending_bytes: usize = 0;
            let mut last_progress = Instant::now();
            let mut prev_stall_strike_level: usize = 0;

            loop {
                while let Ok((c, r)) = resize_rx.try_recv() {
                    pty_cols = c.clamp(20, 512);
                    pty_rows = r.clamp(5, 256);
                    if let Err(e) = channel.request_pty_size(pty_cols, pty_rows, None, None) {
                        log::debug!(
                            "[SSH-IO] Session {} request_pty_size {}x{}: {}",
                            session_id,
                            pty_cols,
                            pty_rows,
                            e
                        );
                    } else {
                        log::info!(
                            "[SSH-IO] Session {} PTY size -> {}x{}",
                            session_id,
                            pty_cols,
                            pty_rows
                        );
                    }
                }

                // 处理输入（非阻塞）
                while let Ok(data) = input_rx.try_recv() {
                    log::info!(
                        "[SSH-IO] Session {} dequeued input ({} bytes)",
                        session_id,
                        data.len()
                    );
                    // 合并小包输入，减少 1 字节频繁写导致的拥塞和可重试错误
                    if let Some(last) = pending_writes.back_mut() {
                        if last.len() + data.len() <= 4096 {
                            last.extend_from_slice(&data);
                            pending_bytes += data.len();
                            continue;
                        }
                    }
                    pending_bytes += data.len();
                    pending_writes.push_back(data);

                    // 背压控制：队列过长时丢弃最旧输入，优先保证“最新操作可执行”
                    while pending_writes.len() > MAX_PENDING_CHUNKS || pending_bytes > MAX_PENDING_BYTES {
                        if let Some(dropped) = pending_writes.pop_front() {
                            pending_bytes = pending_bytes.saturating_sub(dropped.len());
                            log::warn!(
                                "[SSH-IO] Session {} input backlog overflow, dropped {} bytes (chunks={}, bytes={})",
                                session_id,
                                dropped.len(),
                                pending_writes.len(),
                                pending_bytes
                            );
                        } else {
                            break;
                        }
                    }
                }

                // 先读再写：非阻塞下写返回 WouldBlock / draining incoming flow 时必须先排空入站。
                if let Err(stop) = Self::drain_channel_reads_nonblocking(
                    &mut channel,
                    &mut read_buffer,
                    &message_tx,
                    session_id,
                    &mut last_progress,
                ) {
                    match stop {
                        SshReadStop::Disconnected => {
                            let _ = message_tx.send(SshMessage::Disconnected { session_id });
                            return;
                        }
                        SshReadStop::Fatal(msg) => {
                            let _ = message_tx.send(SshMessage::Error { session_id, error: msg });
                            return;
                        }
                    }
                }

                // 用 `front()` 切片写，避免在持有 `front_mut` 时无法 `read` 与 libssh2 要求冲突。
                while !pending_writes.is_empty() {
                    let front_len = pending_writes.front().map(|v| v.len()).unwrap_or(0);
                    let write_res = {
                        let front_slice = pending_writes.front().unwrap().as_slice();
                        channel.write(front_slice)
                    };
                    match write_res {
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
                            if n >= front_len {
                                if let Some(written_chunk) = pending_writes.pop_front() {
                                    pending_bytes = pending_bytes.saturating_sub(written_chunk.len());
                                }
                            } else if let Some(v) = pending_writes.front_mut() {
                                v.drain(..n);
                                pending_bytes = pending_bytes.saturating_sub(n);
                            }
                            let _ = channel.flush();
                            last_progress = Instant::now();
                            if n < front_len {
                                break;
                            }
                        }
                        Err(e) => {
                            if Self::is_retryable_write_error(&e) {
                                log::debug!(
                                    "[SSH-IO] Session {} retryable write error: {}",
                                    session_id,
                                    e
                                );
                                if let Err(stop) = Self::drain_channel_reads_nonblocking(
                                    &mut channel,
                                    &mut read_buffer,
                                    &message_tx,
                                    session_id,
                                    &mut last_progress,
                                ) {
                                    match stop {
                                        SshReadStop::Disconnected => {
                                            let _ =
                                                message_tx.send(SshMessage::Disconnected { session_id });
                                            return;
                                        }
                                        SshReadStop::Fatal(msg) => {
                                            let _ = message_tx.send(SshMessage::Error {
                                                session_id,
                                                error: msg,
                                            });
                                            return;
                                        }
                                    }
                                }
                                let mut blocking_on = false;
                                {
                                    let mut all = sessions.lock().unwrap_or_else(|e| e.into_inner());
                                    if let Some(cli) = all.get_mut(&session_id) {
                                        blocking_on = cli.set_blocking(true).is_ok();
                                    }
                                }
                                if blocking_on {
                                    let bw = {
                                        let front_slice = pending_writes.front().unwrap().as_slice();
                                        channel.write(front_slice)
                                    };
                                    {
                                        let mut all = sessions.lock().unwrap_or_else(|e| e.into_inner());
                                        if let Some(cli) = all.get_mut(&session_id) {
                                            let _ = cli.set_blocking(false);
                                        }
                                    }
                                    match bw {
                                        Ok(0) => {
                                            let _ = message_tx.send(SshMessage::Disconnected { session_id });
                                            return;
                                        }
                                        Ok(n) => {
                                            log::info!(
                                                "[SSH-IO] Session {} wrote {} bytes (blocking retry)",
                                                session_id,
                                                n
                                            );
                                            let fl = pending_writes.front().map(|v| v.len()).unwrap_or(0);
                                            if n >= fl {
                                                if let Some(written_chunk) = pending_writes.pop_front() {
                                                    pending_bytes =
                                                        pending_bytes.saturating_sub(written_chunk.len());
                                                }
                                            } else if let Some(v) = pending_writes.front_mut() {
                                                v.drain(..n);
                                                pending_bytes = pending_bytes.saturating_sub(n);
                                            }
                                            let _ = channel.flush();
                                            last_progress = Instant::now();
                                            continue;
                                        }
                                        Err(_) => {}
                                    }
                                }
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

                if let Err(stop) = Self::drain_channel_reads_nonblocking(
                    &mut channel,
                    &mut read_buffer,
                    &message_tx,
                    session_id,
                    &mut last_progress,
                ) {
                    match stop {
                        SshReadStop::Disconnected => {
                            let _ = message_tx.send(SshMessage::Disconnected { session_id });
                            return;
                        }
                        SshReadStop::Fatal(msg) => {
                            let _ = message_tx.send(SshMessage::Error { session_id, error: msg });
                            return;
                        }
                    }
                }

                let idle = last_progress.elapsed();
                let stall_strike = Self::write_backlog_strike_level(!pending_writes.is_empty(), idle);
                if stall_strike != prev_stall_strike_level {
                    prev_stall_strike_level = stall_strike;
                    if stall_strike > 0 {
                        log::warn!(
                            "[SSH-IO] Session {} write backlog stalled for {:?}, pending_chunks={}, pending_bytes={}, strike={}",
                            session_id,
                            idle,
                            pending_writes.len(),
                            pending_bytes,
                            stall_strike
                        );
                    }
                }

                if stall_strike >= STALL_REOPEN_THRESHOLD {
                    log::warn!(
                        "[SSH-IO] Session {} stalled repeatedly, trying to reopen shell channel",
                        session_id
                    );
                    // 先尝试关闭旧 channel，避免服务端侧 channel 资源占用导致 reopen 卡住
                    let _ = channel.close();
                    let _ = channel.wait_close();
                    // 必须在同一会话上 open 新 channel 之前释放旧 Channel，否则 libssh2 常报无法发送 channel-open
                    drop(channel);

                    let reopen_result = {
                        let mut all = sessions.lock().unwrap();
                        let sess = all
                            .get_mut(&session_id)
                            .ok_or_else(|| format!("Session {} not found during reopen", session_id));
                        match sess {
                            Ok(sess) if sess.is_connected() => {
                                sess.open_shell(pty_cols, pty_rows)
                            }
                            Ok(_) => Err("Session disconnected during reopen".to_string()),
                            Err(e) => Err(e),
                        }
                    };

                    match reopen_result {
                        Ok(new_channel) => {
                            channel = new_channel;
                            prev_stall_strike_level = 0;
                            last_progress = Instant::now();
                            log::info!("[SSH-IO] Session {} shell channel reopened", session_id);
                        }
                        Err(e) => {
                            log::error!(
                                "[SSH-IO] Session {} reopen shell failed after stall: {}",
                                session_id,
                                e
                            );
                            let _ = message_tx.send(SshMessage::Disconnected { session_id });
                            let _ = message_tx.send(SshMessage::Error {
                                session_id,
                                error: format!("Stalled and reopen failed: {}", e),
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
            resize_tx,
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
    fn test_write_backlog_strike_level_no_pending() {
        assert_eq!(
            SshManager::write_backlog_strike_level(false, Duration::from_secs(100)),
            0
        );
    }

    #[test]
    fn test_write_backlog_strike_level_respects_two_second_gate() {
        assert_eq!(
            SshManager::write_backlog_strike_level(true, Duration::from_secs(2)),
            0
        );
        assert_eq!(
            SshManager::write_backlog_strike_level(true, Duration::from_secs(2) + Duration::from_nanos(1)),
            1
        );
    }

    #[test]
    fn test_write_backlog_strike_level_one_strike_per_two_seconds_idle() {
        assert_eq!(
            SshManager::write_backlog_strike_level(true, Duration::from_secs(3)),
            1
        );
        assert_eq!(
            SshManager::write_backlog_strike_level(true, Duration::from_secs(4)),
            2
        );
        assert_eq!(
            SshManager::write_backlog_strike_level(true, Duration::from_secs(15)),
            7
        );
        assert_eq!(
            SshManager::write_backlog_strike_level(true, Duration::from_secs(16)),
            8
        );
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
