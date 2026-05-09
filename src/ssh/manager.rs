//! SSH 管理器 - 管理多个 SSH 会话
#![allow(dead_code)]

use super::client::{SshClient, SshConfig};
use crate::ssh::lrzsz::UploadPtyBypass;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::mpsc::{self, sync_channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;
use ssh2::Channel;

/// SSH 会话 ID
pub type SshSessionId = usize;

/// Shell 泵命令（经 `std::sync::mpsc::sync_channel` 入队，**专用 OS 线程**顺序执行 PTY I/O）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellPumpCommand {
    /// 用户键盘 → PTY
    PtyInput(Vec<u8>),
    /// 本机 ZMODEM 上传（sz→rz）二进制帧 → 同一 PTY
    ZmodemWrite(Vec<u8>),
}

/// 有界同步队列发送端（与 [`SHELL_PUMP_QUEUE_CAP`] 一致；任意线程 `send` 阻塞直至泵取走）
pub type ShellPumpTx = std::sync::mpsc::SyncSender<ShellPumpCommand>;

/// shell 泵命令队列容量（条）
const SHELL_PUMP_QUEUE_CAP: usize = 512;
const RESIZE_QUEUE_CAP: usize = 16;

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
    /// 用户在终端回车提交的一条命令
    UserCommand {
        session_id: SshSessionId,
        command: String,
    },
}

/// SSH 会话句柄
#[derive(Clone)]
pub struct SshSessionHandle {
    pub session_id: SshSessionId,
    pump_tx: ShellPumpTx,
    resize_tx: std::sync::mpsc::SyncSender<(u32, u32)>,
    upload_bypass_slot: Arc<Mutex<Option<Arc<UploadPtyBypass>>>>,
}

impl SshSessionHandle {
    fn pump_send(&self, cmd: ShellPumpCommand) -> Result<(), String> {
        self.pump_tx
            .send(cmd)
            .map_err(|e| format!("Send failed: {}", e))
    }

    /// 发送输入数据（与 ZMODEM 写入同一 FIFO，由 shell 泵线程顺序执行）
    pub fn send_input(&self, data: &[u8]) -> Result<(), String> {
        self.pump_send(ShellPumpCommand::PtyInput(data.to_vec()))
    }

    pub fn send_zmodem(&self, data: Vec<u8>) -> Result<(), String> {
        self.pump_send(ShellPumpCommand::ZmodemWrite(data))
    }

    pub fn shell_pump_tx(&self) -> ShellPumpTx {
        self.pump_tx.clone()
    }

    pub fn resize_pty(&self, cols: u32, rows: u32) -> Result<(), String> {
        let cols = cols.clamp(20, 512);
        let rows = rows.clamp(5, 256);
        self.resize_tx
            .send((cols, rows))
            .map_err(|e| format!("Resize failed: {}", e))
    }

    /// ZMODEM→`rz` 上传：注册后 shell 泵在每次 `channel.read` 时同步旁路到 `upload_pty_rx`（见 [`crate::ssh::lrzsz::LrzszTransfer`]）。
    pub fn set_upload_pty_bypass(&self, bypass: Option<Arc<UploadPtyBypass>>) {
        if let Ok(mut g) = self.upload_bypass_slot.lock() {
            *g = bypass;
        }
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
        match err.kind() {
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted => true,
            _ => {
                let msg = err.to_string().to_lowercase();
                msg.contains("would block")
                    || msg.contains("try again")
                    || msg.contains("eagain")
                    || msg.contains("resource temporarily unavailable")
                    // libssh2：远端窗口满时常见，须先读入站再重试写
                    || msg.contains("unable to send")
                    || msg.contains("window")
                    || msg.contains("flow")
            }
        }
    }

    fn is_retryable_read_error(err: &std::io::Error) -> bool {
        let msg = err.to_string().to_lowercase();
        msg.contains("would block") || msg.contains("try again")
    }

    /// 非阻塞读：直到 EAGAIN / WouldBlock，把数据发到 UI（libssh2 写之前必须先排空入站）
    pub(crate) fn pump_channel_reads(
        channel: &mut Channel,
        read_buffer: &mut [u8],
        message_tx: &Sender<SshMessage>,
        session_id: SshSessionId,
        upload_bypass: &Arc<Mutex<Option<Arc<UploadPtyBypass>>>>,
    ) -> Result<(), ()> {
        loop {
            match channel.read(read_buffer) {
                Ok(0) => {
                    let _ = message_tx.send(SshMessage::Disconnected { session_id });
                    return Err(());
                }
                Ok(n) => {
                    if let Ok(guard) = upload_bypass.lock() {
                        if let Some(ref b) = *guard {
                            b.feed_from_shell_pump(&read_buffer[..n]);
                        }
                    }
                    let _ = message_tx.send(SshMessage::Output {
                        session_id,
                        data: read_buffer[..n].to_vec(),
                    });
                }
                Err(e) if Self::is_retryable_read_error(&e) => return Ok(()),
                Err(e) => {
                    let _ = message_tx.send(SshMessage::Error {
                        session_id,
                        error: format!("Read error: {}", e),
                    });
                    return Err(());
                }
            }
        }
    }

    /// 写入 PTY：按 libssh2 **写窗口**分块，遇窗口满 / EAGAIN 时先读入站再短睡，避免 ZMODEM 大包死循环。
    pub(crate) fn write_pty_with_drain(
        channel: &mut Channel,
        data: &[u8],
        read_buffer: &mut [u8],
        message_tx: &Sender<SshMessage>,
        session_id: SshSessionId,
        upload_bypass: &Arc<Mutex<Option<Arc<UploadPtyBypass>>>>,
    ) -> std::io::Result<()> {
        const MAX_NO_PROGRESS: usize = 60_000;
        const CHUNK_CEIL: usize = 256 * 1024;
        let mut rest = data;
        let mut no_progress = 0usize;

        while !rest.is_empty() {
            let len_before = rest.len();

            let win = channel.write_window().remaining as usize;
            if win == 0 {
                if Self::pump_channel_reads(
                    channel,
                    read_buffer,
                    message_tx,
                    session_id,
                    upload_bypass,
                )
                .is_err()
                {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        "channel closed",
                    ));
                }
            } else {
                let chunk = rest.len().min(win).min(CHUNK_CEIL).max(1);
                let chunk = chunk.min(rest.len());

                match channel.write(&rest[..chunk]) {
                    Ok(0) => {
                        if Self::pump_channel_reads(
                            channel,
                            read_buffer,
                            message_tx,
                            session_id,
                            upload_bypass,
                        )
                        .is_err()
                        {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::ConnectionAborted,
                                "channel closed",
                            ));
                        }
                    }
                    Ok(raw_n) => {
                        let n = raw_n.min(chunk);
                        if raw_n > chunk {
                            log::warn!(
                                "channel.write 声称写入 {} bytes，本段请求仅 {} bytes，按请求长度截断",
                                raw_n,
                                chunk
                            );
                        }
                        if n > 0 {
                            rest = &rest[n..];
                            if !rest.is_empty() {
                                let _ = Self::pump_channel_reads(
                                    channel,
                                    read_buffer,
                                    message_tx,
                                    session_id,
                                    upload_bypass,
                                );
                            }
                        } else if Self::pump_channel_reads(
                            channel,
                            read_buffer,
                            message_tx,
                            session_id,
                            upload_bypass,
                        )
                        .is_err()
                        {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::ConnectionAborted,
                                "channel closed",
                            ));
                        }
                    }
                    Err(e) if Self::is_retryable_write_error(&e) => {
                        if Self::pump_channel_reads(
                            channel,
                            read_buffer,
                            message_tx,
                            session_id,
                            upload_bypass,
                        )
                        .is_err()
                        {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::ConnectionAborted,
                                "channel closed",
                            ));
                        }
                    }
                    Err(e) => return Err(e),
                }
            }

            if rest.len() < len_before {
                no_progress = 0;
            } else {
                no_progress += 1;
                if no_progress > MAX_NO_PROGRESS {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!(
                            "write_pty_with_drain: 长时间无进展（写窗口或 write 阻塞），剩 {} bytes",
                            rest.len()
                        ),
                    ));
                }
                thread::sleep(Duration::from_micros(150));
            }
        }

        let mut flush_no_progress = 0usize;
        loop {
            match channel.flush() {
                Ok(()) => break,
                Err(e) if Self::is_retryable_write_error(&e) => {
                    flush_no_progress += 1;
                    if flush_no_progress > MAX_NO_PROGRESS {
                        return Err(e);
                    }
                    if Self::pump_channel_reads(
                        channel,
                        read_buffer,
                        message_tx,
                        session_id,
                        upload_bypass,
                    )
                    .is_err()
                    {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::ConnectionAborted,
                            "channel closed",
                        ));
                    }
                    thread::sleep(Duration::from_micros(150));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
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

    /// 启动交互式 shell：泵在**专用线程**内顺序执行 PTY 读写（`sync_channel` 与 UI/上传线程解耦）。
    pub fn start_interactive_shell(
        &self,
        session_id: SshSessionId,
        initial_cols: u32,
        initial_rows: u32,
    ) -> Result<SshSessionHandle, String> {
        let message_tx = self.message_tx.clone();
        let sessions = self.sessions.clone();

        let (pump_tx, pump_rx) = sync_channel::<ShellPumpCommand>(SHELL_PUMP_QUEUE_CAP);
        let (resize_tx, resize_rx) = sync_channel::<(u32, u32)>(RESIZE_QUEUE_CAP);
        let upload_bypass_slot = Arc::new(Mutex::new(None::<Arc<UploadPtyBypass>>));

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

        shell_pump::spawn_shell_pump(
            channel,
            pump_rx,
            resize_rx,
            message_tx,
            session_id,
            upload_bypass_slot.clone(),
        );

        Ok(SshSessionHandle {
            session_id,
            pump_tx,
            resize_tx,
            upload_bypass_slot,
        })
    }

    pub fn session_count(&self) -> usize {
        self.sessions.lock().unwrap().len()
    }

    pub fn get_session(&self, session_id: SshSessionId) -> Option<::ssh2::Session> {
        let sessions = self.sessions.lock().unwrap();
        sessions.get(&session_id).map(|c| c.get_session().clone())
    }

    /// 在独立 exec 通道执行远程命令（与交互式 shell 并存）。
    ///
    /// 与 SFTP/SCP 相同，使用 [`get_session`] 克隆的会话句柄并短期切为阻塞模式完成读写，
    /// 执行结束后恢复非阻塞供 shell 泵使用。
    pub fn exec_remote(&self, session_id: SshSessionId, command: &str) -> Result<String, String> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| format!("会话 {} 不可用（未连接或已移除）", session_id))?;
        Self::exec_on_cloned_session(&session, command)
    }

    fn exec_on_cloned_session(session: &ssh2::Session, command: &str) -> Result<String, String> {
        use std::io::Read;
        session.set_blocking(true);
        let result = (|| {
            let mut channel = session
                .channel_session()
                .map_err(|e| format!("打开 exec 通道失败: {}", e))?;
            channel
                .exec(command)
                .map_err(|e| format!("exec 失败: {} — {}", command, e))?;
            let mut output = Vec::new();
            channel
                .read_to_end(&mut output)
                .map_err(|e| format!("读取命令输出失败: {}", e))?;
            let _ = channel.wait_close();
            String::from_utf8(output).map_err(|e| format!("输出非 UTF-8: {}", e))
        })();
        session.set_blocking(false);
        result
    }
}

/// Shell 泵：专用 OS 线程 + `sync_channel`，避免 Tokio `block_on`/`recv` 与上传线程的调度死锁。
mod shell_pump {
    use super::{ShellPumpCommand, SshManager, SshMessage, SshSessionId, UploadPtyBypass};
    use ssh2::Channel;
    use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    pub(super) fn spawn_shell_pump(
        channel: Channel,
        pump_rx: Receiver<ShellPumpCommand>,
        resize_rx: Receiver<(u32, u32)>,
        message_tx: Sender<SshMessage>,
        session_id: SshSessionId,
        upload_bypass_slot: Arc<Mutex<Option<Arc<UploadPtyBypass>>>>,
    ) {
        let channel = Arc::new(Mutex::new(channel));
        thread::Builder::new()
            .name(format!("mistterm-shell-pump-{}", session_id))
            .spawn(move || {
                log::info!(
                    "shell 泵线程启动 session_id={} queue_cap={}",
                    session_id,
                    super::SHELL_PUMP_QUEUE_CAP
                );
                shell_pump_loop(
                    channel,
                    pump_rx,
                    resize_rx,
                    message_tx,
                    session_id,
                    upload_bypass_slot,
                );
                log::warn!("shell 泵线程退出 session_id={}", session_id);
            })
            .expect("spawn shell pump thread");
    }

    fn shell_pump_loop(
        channel: Arc<Mutex<Channel>>,
        pump_rx: Receiver<ShellPumpCommand>,
        resize_rx: Receiver<(u32, u32)>,
        message_tx: Sender<SshMessage>,
        session_id: SshSessionId,
        upload_bypass_slot: Arc<Mutex<Option<Arc<UploadPtyBypass>>>>,
    ) {
        let mut read_buffer = [0u8; 16384];
        let mut input_line_buf: Vec<u8> = Vec::new();
        let mut esc_state = InputEscState::None;
        loop {
            while let Ok((c, r)) = resize_rx.try_recv() {
                let pty_cols = c.clamp(20, 512);
                let pty_rows = r.clamp(5, 256);
                log::debug!("Resize to {}x{}", pty_cols, pty_rows);
                if let Ok(mut ch) = channel.lock() {
                    let px_w = pty_cols.saturating_mul(9);
                    let px_h = pty_rows.saturating_mul(16);
                    if let Err(e) =
                        ch.request_pty_size(pty_cols, pty_rows, Some(px_w), Some(px_h))
                    {
                        log::warn!("request_pty_size: {}", e);
                    }
                }
            }

            match pump_rx.recv_timeout(Duration::from_millis(8)) {
                Ok(cmd) => {
                    if let ShellPumpCommand::PtyInput(data) = &cmd {
                        let commands = capture_and_log_user_command(
                            session_id,
                            data,
                            &mut input_line_buf,
                            &mut esc_state,
                        );
                        for command in commands {
                            let _ = message_tx.send(SshMessage::UserCommand { session_id, command });
                        }
                    }
                    if !process_one_command_sync(
                        &channel,
                        &message_tx,
                        session_id,
                        cmd,
                        &mut read_buffer,
                        &upload_bypass_slot,
                    ) {
                        return;
                    }
                    while let Ok(more) = pump_rx.try_recv() {
                        if !process_one_command_sync(
                            &channel,
                            &message_tx,
                            session_id,
                            more,
                            &mut read_buffer,
                            &upload_bypass_slot,
                        ) {
                            return;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    if !process_idle_read_sync(
                        &channel,
                        &message_tx,
                        session_id,
                        &mut read_buffer,
                        &upload_bypass_slot,
                    ) {
                        return;
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    log::warn!(
                        "shell 泵 session={} pump_rx Disconnected，线程结束",
                        session_id
                    );
                    return;
                }
            }
        }
    }

    fn process_one_command_sync(
        channel: &Arc<Mutex<Channel>>,
        message_tx: &Sender<SshMessage>,
        session_id: SshSessionId,
        cmd: ShellPumpCommand,
        read_buffer: &mut [u8; 16384],
        upload_bypass: &Arc<Mutex<Option<Arc<UploadPtyBypass>>>>,
    ) -> bool {
        let mut ch = match channel.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!("shell pump: channel mutex poisoned: {}", e);
                return false;
            }
        };
        match cmd {
            ShellPumpCommand::PtyInput(data) => {
                if SshManager::pump_channel_reads(
                    &mut *ch,
                    read_buffer,
                    message_tx,
                    session_id,
                    upload_bypass,
                )
                .is_err()
                {
                    return false;
                }
                if let Err(e) = SshManager::write_pty_with_drain(
                    &mut *ch,
                    &data,
                    read_buffer,
                    message_tx,
                    session_id,
                    upload_bypass,
                ) {
                    log::error!("Write error: {}", e);
                }
            }
            ShellPumpCommand::ZmodemWrite(data) => {
                let n = data.len();
                let w = SshManager::write_pty_with_drain(
                    &mut *ch,
                    &data,
                    read_buffer,
                    message_tx,
                    session_id,
                    upload_bypass,
                );
                if let Err(e) = w {
                    log::error!(
                        "shell 泵 session={} ZmodemWrite 失败 n={} {}",
                        session_id,
                        n,
                        e
                    );
                }
            }
        }
        true
    }

    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    enum InputEscState {
        None,
        Esc,
        Csi,
        Ss3,
        Osc,
        OscEsc,
    }

    fn capture_and_log_user_command(
        session_id: SshSessionId,
        data: &[u8],
        line_buf: &mut Vec<u8>,
        esc_state: &mut InputEscState,
    ) -> Vec<String> {
        let mut commands = Vec::new();
        for b in data {
            match *esc_state {
                InputEscState::Esc => {
                    *esc_state = match *b {
                        b'[' => InputEscState::Csi,
                        b'O' => InputEscState::Ss3,
                        b']' => InputEscState::Osc,
                        _ => InputEscState::None,
                    };
                    continue;
                }
                InputEscState::Csi => {
                    // CSI 结束字节范围 0x40..=0x7E，期间全部忽略（方向键、Home/End 等）
                    if (0x40..=0x7e).contains(b) {
                        *esc_state = InputEscState::None;
                    }
                    continue;
                }
                InputEscState::Ss3 => {
                    // SS3 序列通常只有一个终止字节
                    *esc_state = InputEscState::None;
                    continue;
                }
                InputEscState::Osc => {
                    if *b == 0x07 {
                        *esc_state = InputEscState::None;
                    } else if *b == 0x1b {
                        *esc_state = InputEscState::OscEsc;
                    }
                    continue;
                }
                InputEscState::OscEsc => {
                    *esc_state = if *b == b'\\' {
                        InputEscState::None
                    } else {
                        InputEscState::Osc
                    };
                    continue;
                }
                InputEscState::None => {}
            }

            match *b {
                0x1b => *esc_state = InputEscState::Esc,
                b'\r' | b'\n' => {
                    if !line_buf.is_empty() {
                        let cmd = String::from_utf8_lossy(line_buf).trim().to_string();
                        // 过滤残留控制序列碎片，避免把全屏程序内部按键当命令。
                        if !cmd.is_empty() && !cmd.contains('[') && !cmd.contains('\u{1b}') {
                            log::info!("shell 输入命令 session={} cmd={}", session_id, cmd);
                            commands.push(cmd);
                        }
                        line_buf.clear();
                    }
                }
                0x08 | 0x7f => {
                    let _ = line_buf.pop();
                }
                b' '..=b'~' => line_buf.push(*b),
                _ => {}
            }
        }
        if line_buf.len() > 4096 {
            line_buf.clear();
            *esc_state = InputEscState::None;
        }
        commands
    }

    fn process_idle_read_sync(
        channel: &Arc<Mutex<Channel>>,
        message_tx: &Sender<SshMessage>,
        session_id: SshSessionId,
        read_buffer: &mut [u8; 16384],
        upload_bypass: &Arc<Mutex<Option<Arc<UploadPtyBypass>>>>,
    ) -> bool {
        let mut ch = match channel.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!("shell pump idle: mutex poisoned: {}", e);
                return false;
            }
        };
        SshManager::pump_channel_reads(
            &mut *ch,
            read_buffer,
            message_tx,
            session_id,
            upload_bypass,
        )
        .is_ok()
    }

    #[cfg(test)]
    mod tests {
        use super::super::ShellPumpCommand;
        use std::sync::mpsc::sync_channel;
        use std::thread;
        use std::time::Duration;

        #[test]
        fn pump_command_queue_fifo_order() {
            let (tx, rx) = sync_channel::<ShellPumpCommand>(16);
            tx.send(ShellPumpCommand::PtyInput(vec![0x61])).unwrap();
            tx.send(ShellPumpCommand::ZmodemWrite(vec![0x2a, 0x2a]))
                .unwrap();
            drop(tx);
            assert_eq!(
                rx.recv().unwrap(),
                ShellPumpCommand::PtyInput(vec![0x61])
            );
            assert_eq!(
                rx.recv().unwrap(),
                ShellPumpCommand::ZmodemWrite(vec![0x2a, 0x2a])
            );
            assert!(rx.recv().is_err());
        }

        #[test]
        fn bounded_sync_pump_queue_backpressure() {
            let (tx, rx) = sync_channel::<ShellPumpCommand>(2);
            tx.send(ShellPumpCommand::PtyInput(vec![1])).unwrap();
            tx.send(ShellPumpCommand::PtyInput(vec![2])).unwrap();
            let tx_c = tx.clone();
            let fill = thread::spawn(move || {
                tx_c.send(ShellPumpCommand::PtyInput(vec![3])).unwrap();
            });
            thread::sleep(Duration::from_millis(20));
            assert_eq!(rx.recv().unwrap(), ShellPumpCommand::PtyInput(vec![1]));
            assert_eq!(rx.recv().unwrap(), ShellPumpCommand::PtyInput(vec![2]));
            fill.join().unwrap();
            assert_eq!(rx.recv().unwrap(), ShellPumpCommand::PtyInput(vec![3]));
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn manager_new_drops_cleanly() {
        let (mgr, _rx) = super::SshManager::new();
        drop(mgr);
    }
}
