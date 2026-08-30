//! SSH 管理器 - 管理多个 SSH 会话
#![allow(dead_code)]

use super::client::{SshClient, SshConfig};
use super::SessionBlockingGuard;
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
///
/// 说明：`ExecRemote` 必须在泵线程执行，与 PTY `Channel` **互斥**使用同一底层 `Session`，
/// 不得在其他 OS 线程并发 `exec_remote`（否则易出现 PTY「假死」、终端无法交互）。
pub enum ShellPumpCommand {
    /// 用户键盘 → PTY（带序号与入队时间，便于排查「输入晚半拍才回显」）
    PtyInput {
        seq: u64,
        enqueued_at: std::time::Instant,
        data: Vec<u8>,
    },
    /// 本机 ZMODEM 上传（sz→rz）二进制帧 → 同一 PTY
    ZmodemWrite(Vec<u8>),
    ExecRemote {
        cmd: String,
        reply: mpsc::Sender<Result<String, String>>,
    },
    /// 在 shell 泵线程内独占执行的 Session 任务（SFTP 等）。
    ///
    /// shell 泵处理本命令时已 `drop(ch)` 释放 PTY channel 锁，并暂停 PTY 读循环，
    /// 闭包在该期间独占 Session：可放心 `set_blocking(true)`，期间 libssh2 的
    /// 内部 mutex 不会与 PTY 读争抢，避免 `Timeout waiting for status message`。
    /// 闭包负责自己的失败回执（通过捕获的 `mpsc::Sender`）。
    SessionJob(Box<dyn FnOnce(&::ssh2::Session) + Send>),
}

impl ShellPumpCommand {
    /// 只返回变体名（用于日志，避免泄露输入内容 / ZMODEM 字节 / 命令串）。
    pub(crate) fn variant_name(cmd: &ShellPumpCommand) -> &'static str {
        match cmd {
            ShellPumpCommand::PtyInput { .. } => "PtyInput",
            ShellPumpCommand::ZmodemWrite(_) => "ZmodemWrite",
            ShellPumpCommand::ExecRemote { .. } => "ExecRemote",
            ShellPumpCommand::SessionJob(_) => "SessionJob",
        }
    }
}

impl std::fmt::Debug for ShellPumpCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellPumpCommand::PtyInput { seq, data, .. } => f
                .debug_struct("ShellPumpCommand::PtyInput")
                .field("seq", seq)
                .field("n", &data.len())
                .finish(),
            ShellPumpCommand::ZmodemWrite(v) => {
                f.debug_tuple("ShellPumpCommand::ZmodemWrite").field(v).finish()
            }
            ShellPumpCommand::ExecRemote { cmd, .. } => {
                f.debug_struct("ShellPumpCommand::ExecRemote").field("cmd", cmd).finish()
            }
            ShellPumpCommand::SessionJob(_) => {
                f.debug_struct("ShellPumpCommand::SessionJob").finish_non_exhaustive()
            }
        }
    }
}

fn pty_input_kind(data: &[u8]) -> &'static str {
    match data {
        [b'\t'] => "tab",
        [b'\r'] | [b'\n'] | [b'\r', b'\n'] => "enter",
        [0x03] => "ctrl_c",
        [0x04] => "ctrl_d",
        [0x7f] | [0x08] => "bs",
        [b] if b.is_ascii_graphic() || *b == b' ' => "key",
        _ => "bytes",
    }
}

static PTY_INPUT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

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
    interrupt_tx: std::sync::mpsc::SyncSender<Vec<u8>>,
    resize_tx: std::sync::mpsc::SyncSender<(u32, u32)>,
    upload_bypass_slot: Arc<Mutex<Option<Arc<UploadPtyBypass>>>>,
}

impl SshSessionHandle {
    /// 写入 shell 泵命令队列。
    ///
    /// 使用 `try_send`（非阻塞）：当 shell 泵线程被 `write_pty_with_drain` 或长读卡住、队列满时，
    /// **不阻塞调用方（UI 线程）**，而是返回错误让上层决定丢弃或降级。
    /// 这是 Tab 补全触发「UI 整个不动 10+ 秒」的根因之一：旧代码用阻塞 `send`，
    /// 泵一旦慢于 UI 输入（`SHELL_PUMP_QUEUE_CAP=512` 被打满），UI 线程就卡在同步队列上。
    fn pump_send(&self, cmd: ShellPumpCommand) -> Result<(), String> {
        self.pump_tx
            .try_send(cmd)
            .map_err(|e| match e {
                std::sync::mpsc::TrySendError::Full(c) => {
                    format!(
                        "shell pump queue full (cap={}), dropped cmd variant={}",
                        SHELL_PUMP_QUEUE_CAP,
                        ShellPumpCommand::variant_name(&c)
                    )
                }
                std::sync::mpsc::TrySendError::Disconnected(c) => {
                    format!(
                        "shell pump queue disconnected, dropped cmd variant={}",
                        ShellPumpCommand::variant_name(&c)
                    )
                }
            })
    }

    /// 发送输入数据（与 ZMODEM 写入同一 FIFO，由 shell 泵线程顺序执行）。
    ///
    /// 当 shell 泵队列满时本调用**不会阻塞 UI**，而是丢弃该次输入并返回错误 ——
    /// 避免用户在 Tab / 持续按键时 UI 线程同步死在发送端。
    pub fn send_input(&self, data: &[u8]) -> Result<(), String> {
        let seq = PTY_INPUT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let kind = pty_input_kind(data);
        log::debug!(
            "INPUT_ENQ session={} seq={} kind={} n={} first={:02x}",
            self.session_id,
            seq,
            kind,
            data.len(),
            data.first().copied().unwrap_or(0)
        );
        let res = self.pump_send(ShellPumpCommand::PtyInput {
            seq,
            enqueued_at: std::time::Instant::now(),
            data: data.to_vec(),
        });
        if let Err(ref e) = res {
            log::warn!(
                "INPUT_DROP session={} seq={} kind={} err={}",
                self.session_id,
                seq,
                kind,
                e
            );
        }
        res
    }

    pub fn send_zmodem(&self, data: Vec<u8>) -> Result<(), String> {
        self.pump_send(ShellPumpCommand::ZmodemWrite(data))
    }

    /// 高优先级写入 PTY，绕过普通输入/ZMODEM 队列；用于中止 ZMODEM 等紧急控制。
    pub fn send_priority_interrupt(&self, data: Vec<u8>) -> Result<(), String> {
        self.interrupt_tx
            .send(data)
            .map_err(|e| format!("Interrupt send failed: {}", e))
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

    /// 将远程一次 `exec` 排入 **shell 泵线程**，与 PTY 读写在同一 OS 线程互斥执行。
    ///
    /// 不得在其它线程对本会话并行调用 [`SshManager::exec_remote`]，否则会与 PTY `Channel` 争用底层
    /// `Session`（常见症状：终端卡死、`set_blocking` 状态错乱）。
    pub fn enqueue_remote_exec(
        &self,
        command: &str,
    ) -> Result<mpsc::Receiver<Result<String, String>>, String> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.pump_send(ShellPumpCommand::ExecRemote {
            cmd: command.to_string(),
            reply: reply_tx,
        })?;
        Ok(reply_rx)
    }

    /// 将一段 Session 任务（如 SFTP 操作）排入 **shell 泵线程** 独占执行。
    ///
    /// shell 泵在处理本命令期间已 `drop(ch)` 释放 PTY channel 锁、并暂停 `channel.read` 轮询，
    /// 闭包独占底层 `ssh2::Session`，可放心 `set_blocking(true)`，避免 `Timeout waiting for status message`。
    /// 闭包应自行通过捕获的 `mpsc::Sender` 回传结果与错误。
    pub fn enqueue_session_job<F>(&self, job: F) -> Result<(), String>
    where
        F: FnOnce(&::ssh2::Session) + Send + 'static,
    {
        self.pump_send(ShellPumpCommand::SessionJob(Box::new(job)))
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

    /// 非阻塞读：直到 EAGAIN / WouldBlock 或达到单次上限，把数据发到 UI（libssh2 写之前必须先排空入站）。
    ///
    /// **单次调用上限**：`max_bytes_per_call`。Tab 补全 / `ls` 大目录 / `cat 大文件` 时远端会持续吐数据，
    /// 若不加上限，`write_pty_with_drain` 内部的「写失败 → pump_channel_reads 再试」路径会卡在不断读输出
    /// 上，导致 PTY 写入久久不能推进（表现：输入的 Tab/字符「没反应」过很久才发出去）。
    /// 传入 `None` 时使用默认 `DEFAULT_READ_BYTES_PER_CALL`（64KB，约 4 轮 16KB buffer）。
    pub(crate) fn pump_channel_reads(
        channel: &mut Channel,
        read_buffer: &mut [u8],
        message_tx: &Sender<SshMessage>,
        session_id: SshSessionId,
        upload_bypass: &Arc<Mutex<Option<Arc<UploadPtyBypass>>>>,
        max_bytes_per_call: Option<usize>,
    ) -> Result<(), ()> {
        const DEFAULT_READ_BYTES_PER_CALL: usize = 64 * 1024;
        let cap = max_bytes_per_call.unwrap_or(DEFAULT_READ_BYTES_PER_CALL);
        let mut pumped = 0usize;
        loop {
            if pumped >= cap {
                log::debug!("PUMP_READ session={} total={} (hit cap)", session_id, pumped);
                return Ok(());
            }
            match channel.read(read_buffer) {
                Ok(0) => {
                    let _ = message_tx.send(SshMessage::Disconnected { session_id });
                    return Err(());
                }
                Ok(n) => {
                    pumped += n;
                    // 上传旁路开启时协议字节已同步进 `upload_pty_rx`；再拷一份进 UI mpsc
                    // 只会拖慢泵线程，并让主线程做无用的 detect/utf8（ACK 路径尤其伤吞吐）。
                    let mut via_bypass = false;
                    if let Ok(guard) = upload_bypass.lock() {
                        if let Some(ref b) = *guard {
                            b.feed_from_shell_pump(&read_buffer[..n]);
                            via_bypass = true;
                        }
                    }
                    if !via_bypass {
                        let _ = message_tx.send(SshMessage::Output {
                            session_id,
                            data: read_buffer[..n].to_vec(),
                        });
                    }
                }
                Err(e) if Self::is_retryable_read_error(&e) => {
                    if pumped > 0 {
                        log::debug!("PUMP_READ session={} total={}", session_id, pumped);
                    }
                    return Ok(());
                }
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
        // 无进展轮询上限：每次 sleep 150µs × 6000 次 ≈ 900ms，≈ 1 秒仍无进展就报错返回。
        // 旧值 60_000 轮 ×150µs = 9 秒，是「Tab 卡住半天 UI 还不动」的两大元凶之一
        // （另一个是 UI 线程同步 send 到泵队列；见 SshSessionHandle::pump_send）。
        const MAX_NO_PROGRESS: usize = 6_000;
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
                    None,
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
                            None,
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
                                    None,
                                );
                            }
                        } else if Self::pump_channel_reads(
                            channel,
                            read_buffer,
                            message_tx,
                            session_id,
                            upload_bypass,
                            None,
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
                            None,
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
                        None,
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
        let (interrupt_tx, interrupt_rx) = sync_channel::<Vec<u8>>(8);
        let (resize_tx, resize_rx) = sync_channel::<(u32, u32)>(RESIZE_QUEUE_CAP);
        let upload_bypass_slot = Arc::new(Mutex::new(None::<Arc<UploadPtyBypass>>));

        let mgr_for_pump = self.clone();

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
            interrupt_rx,
            resize_rx,
            message_tx,
            session_id,
            upload_bypass_slot.clone(),
            mgr_for_pump,
        );

        Ok(SshSessionHandle {
            session_id,
            pump_tx,
            interrupt_tx,
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

    pub(crate) fn tick_session_keepalive(&self, session_id: SshSessionId) {
        let sessions = self.sessions.lock().unwrap();
        if let Some(client) = sessions.get(&session_id) {
            super::client::tick_keepalive(client.get_session());
        }
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
        let _guard = SessionBlockingGuard::new(session);
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
    }

    pub fn spawn_local_forward(
        &self,
        session_id: SshSessionId,
        fwd: super::port_forward::LocalPortForward,
    ) -> Result<super::port_forward::ForwardControl, String> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| format!("会话 {} 不可用", session_id))?;
        super::port_forward::spawn_local_forward_controllable(session, fwd)
    }

    pub fn spawn_remote_forward(
        &self,
        session_id: SshSessionId,
        fwd: super::port_forward::RemotePortForward,
    ) -> Result<super::port_forward::ForwardControl, String> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| format!("会话 {} 不可用", session_id))?;
        super::port_forward::spawn_remote_forward_controllable(session, fwd)
    }

    pub fn spawn_dynamic_forward(
        &self,
        session_id: SshSessionId,
        fwd: super::socks_proxy::DynamicPortForward,
    ) -> Result<super::port_forward::ForwardControl, String> {
        let session = self
            .get_session(session_id)
            .ok_or_else(|| format!("会话 {} 不可用", session_id))?;
        super::socks_proxy::spawn_dynamic_forward_controllable(session, fwd)
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
        interrupt_rx: Receiver<Vec<u8>>,
        resize_rx: Receiver<(u32, u32)>,
        message_tx: Sender<SshMessage>,
        session_id: SshSessionId,
        upload_bypass_slot: Arc<Mutex<Option<Arc<UploadPtyBypass>>>>,
        mgr: SshManager,
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
                interrupt_rx,
                    resize_rx,
                    message_tx,
                    session_id,
                    upload_bypass_slot,
                    mgr,
                );
                log::warn!("shell pump thread exited session_id={}", session_id);
            })
            .expect("spawn shell pump thread");
    }

    fn shell_pump_loop(
        channel: Arc<Mutex<Channel>>,
        pump_rx: Receiver<ShellPumpCommand>,
        interrupt_rx: Receiver<Vec<u8>>,
        resize_rx: Receiver<(u32, u32)>,
        message_tx: Sender<SshMessage>,
        session_id: SshSessionId,
        upload_bypass_slot: Arc<Mutex<Option<Arc<UploadPtyBypass>>>>,
        mgr: SshManager,
    ) {
        let mut read_buffer = [0u8; 16384];
        let mut input_line_buf: Vec<u8> = Vec::new();
        let mut esc_state = InputEscState::None;
        // 探针：泵线程心跳。每轮循环打点，若两轮间隔 > 1s 说明泵被某处卡住。
        let mut __last_beat = std::time::Instant::now();
        'pump_loop: loop {
            {
                let __gap = __last_beat.elapsed();
                if __gap.as_millis() as u64 >= 1_000 {
                    log::warn!(
                        "PUMP_STUCK session={} gap_ms={} (泵线程上一轮耗时异常)",
                        session_id,
                        __gap.as_millis()
                    );
                }
                __last_beat = std::time::Instant::now();
            }
            while let Ok(data) = interrupt_rx.try_recv() {
                if !process_priority_interrupt_sync(
                    &channel,
                    &message_tx,
                    session_id,
                    data,
                    &mut read_buffer,
                    &upload_bypass_slot,
                ) {
                    return;
                }
            }

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

            // ZMODEM 上传旁路开启时缩短空闲读间隔，尽快把 ZACK/ZRPOS 交给发送线程。
            let idle_ms = if upload_bypass_slot
                .lock()
                .ok()
                .and_then(|g| g.as_ref().map(|_| ()))
                .is_some()
            {
                1
            } else {
                8
            };
            match pump_rx.recv_timeout(Duration::from_millis(idle_ms)) {
                Ok(cmd) => {
                    // PtyInput 合并：把泵队列中相邻的 PtyInput 小 Vec<u8> 拼成一条大的再下发。
                    // 这是「Tab + 连续按键挤满 pump_rx（SHELL_PUMP_QUEUE_CAP=512）」的治本方案：
                    // 过去每按一次字母/Tab 就是一条 PtyInput（哪怕只有 1-4 字节），512 条很快被占满，
                    // 现在合并后，同样的按键只占 1 条队列条目，大大降低 try_send 丢帧概率。
                    //
                    // 注意：PtyInput 的 capture_and_log_user_command + UserCommand 分发必须在
                    // **合并之后的数据**上跑，避免把一条按字节拆碎的命令（中间夹着方向键/Ctrl）
                    // 错切到多轮造成 esc_state 漂移或命令截断。
                    let cmd = match cmd {
                        ShellPumpCommand::PtyInput {
                            seq: seq_lo,
                            enqueued_at,
                            mut data,
                        } => {
                            let mut seq_hi = seq_lo;
                            while data.len() < PTY_INPUT_COALESCE_CAP {
                                match pump_rx.try_recv() {
                                    Ok(ShellPumpCommand::PtyInput {
                                        seq,
                                        data: more,
                                        ..
                                    }) => {
                                        seq_hi = seq;
                                        data.extend_from_slice(&more);
                                    }
                                    Ok(other) => {
                                        // 先提交合并好的这一块，把 other 留到下一轮
                                        let commands = capture_and_log_user_command(
                                            session_id,
                                            &data,
                                            &mut input_line_buf,
                                            &mut esc_state,
                                        );
                                        for command in commands {
                                            let _ = message_tx.send(SshMessage::UserCommand {
                                                session_id,
                                                command,
                                            });
                                        }
                                        if !dispatch_pump_command_coalesced(
                                            &mgr,
                                            &channel,
                                            &message_tx,
                                            session_id,
                                            ShellPumpCommand::PtyInput {
                                                seq: seq_lo,
                                                enqueued_at,
                                                data,
                                            },
                                            &pump_rx,
                                            &mut read_buffer,
                                            &upload_bypass_slot,
                                            &interrupt_rx,
                                        ) {
                                            return;
                                        }
                                        // 显式再进 dispatch，确保 deferred 被执行
                                        let ok = dispatch_pump_command_coalesced(
                                            &mgr,
                                            &channel,
                                            &message_tx,
                                            session_id,
                                            other,
                                            &pump_rx,
                                            &mut read_buffer,
                                            &upload_bypass_slot,
                                            &interrupt_rx,
                                        );
                                        if !ok {
                                            return;
                                        }
                                        // 本帧已整体处理完，跳到外层下一轮
                                        continue 'pump_loop;
                                    }
                                    Err(_) => break,
                                }
                            }
                            // 常规出口：合并完毕（要么凑够上限要么队列空）
                            let commands = capture_and_log_user_command(
                                session_id,
                                &data,
                                &mut input_line_buf,
                                &mut esc_state,
                            );
                            for command in commands {
                                let _ = message_tx.send(SshMessage::UserCommand {
                                    session_id,
                                    command,
                                });
                            }
                            if seq_hi != seq_lo {
                                log::debug!(
                                    "INPUT_COALESCE session={} seq={}..={} n={}",
                                    session_id,
                                    seq_lo,
                                    seq_hi,
                                    data.len()
                                );
                            }
                            ShellPumpCommand::PtyInput {
                                seq: seq_lo,
                                enqueued_at,
                                data,
                            }
                        }
                        other => other,
                    };

                    if !dispatch_pump_command_coalesced(
                        &mgr,
                        &channel,
                        &message_tx,
                        session_id,
                        cmd,
                        &pump_rx,
                        &mut read_buffer,
                        &upload_bypass_slot,
                        &interrupt_rx,
                    ) {
                        return;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    let __s = std::time::Instant::now();
                    mgr.tick_session_keepalive(session_id);
                    let __ka = __s.elapsed();
                    let __s2 = std::time::Instant::now();
                    let ok = process_idle_read_sync(
                        &channel,
                        &message_tx,
                        session_id,
                        &mut read_buffer,
                        &upload_bypass_slot,
                    );
                    let __idle = __s2.elapsed();
                    if __ka.as_millis() as u64 >= 200 || __idle.as_millis() as u64 >= 200 {
                        log::warn!(
                            "PUMP_IDLE_SLOW session={} keepalive_ms={} idle_read_ms={}",
                            session_id,
                            __ka.as_millis(),
                            __idle.as_millis()
                        );
                    }
                    if !ok {
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

    /// 连续 `ZmodemWrite` 合并为一次 `write_pty_with_drain`（一次 flush），显著减少 SSH 往返。
    const ZMODEM_WRITE_COALESCE_CAP: usize = 64 * 1024;

    /// 连续 `PtyInput` 合并上限（字节）。
    ///
    /// 每次 egui 帧里用户可能按 1 到几十个键（每个字母/Tab/方向键一条独立的 PtyInput），
    /// 合并为一条后写入 `write_pty_with_drain`，避免 pump_rx 队列被大量 1-4 字节小条目占满，
    /// 从根源减少 `try_send(PtyInput)` 的丢帧概率。
    const PTY_INPUT_COALESCE_CAP: usize = 32 * 1024;

    fn dispatch_pump_command_coalesced(
        mgr: &SshManager,
        channel: &Arc<Mutex<Channel>>,
        message_tx: &Sender<SshMessage>,
        session_id: SshSessionId,
        cmd: ShellPumpCommand,
        pump_rx: &Receiver<ShellPumpCommand>,
        read_buffer: &mut [u8; 16384],
        upload_bypass: &Arc<Mutex<Option<Arc<UploadPtyBypass>>>>,
        interrupt_rx: &Receiver<Vec<u8>>,
    ) -> bool {
        let mut next = Some(cmd);
        while let Some(cmd) = next.take() {
            while let Ok(data) = interrupt_rx.try_recv() {
                if !process_priority_interrupt_sync(
                    channel,
                    message_tx,
                    session_id,
                    data,
                    read_buffer,
                    upload_bypass,
                ) {
                    return false;
                }
            }

            let (cmd, deferred) = match cmd {
                ShellPumpCommand::ZmodemWrite(mut data) => {
                    let mut deferred: Option<ShellPumpCommand> = None;
                    while data.len() < ZMODEM_WRITE_COALESCE_CAP {
                        match pump_rx.try_recv() {
                            Ok(ShellPumpCommand::ZmodemWrite(more)) => {
                                data.extend_from_slice(&more);
                            }
                            Ok(other) => {
                                deferred = Some(other);
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                    (ShellPumpCommand::ZmodemWrite(data), deferred)
                }
                other => (other, None),
            };

            if !process_one_command_sync(
                mgr,
                channel,
                message_tx,
                session_id,
                cmd,
                read_buffer,
                upload_bypass,
            ) {
                return false;
            }

            next = deferred.or_else(|| pump_rx.try_recv().ok());
        }
        true
    }

    fn process_priority_interrupt_sync(
        channel: &Arc<Mutex<Channel>>,
        message_tx: &Sender<SshMessage>,
        session_id: SshSessionId,
        data: Vec<u8>,
        read_buffer: &mut [u8; 16384],
        upload_bypass: &Arc<Mutex<Option<Arc<UploadPtyBypass>>>>,
    ) -> bool {
        let mut ch = match channel.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!("shell pump priority interrupt: channel mutex poisoned: {}", e);
                return false;
            }
        };
        if let Err(e) = SshManager::write_pty_with_drain(
            &mut *ch,
            &data,
            read_buffer,
            message_tx,
            session_id,
            upload_bypass,
        ) {
            log::error!("Priority interrupt write error: {}", e);
        }
        true
    }

    fn process_one_command_sync(
        mgr: &SshManager,
        channel: &Arc<Mutex<Channel>>,
        message_tx: &Sender<SshMessage>,
        session_id: SshSessionId,
        cmd: ShellPumpCommand,
        read_buffer: &mut [u8; 16384],
        upload_bypass: &Arc<Mutex<Option<Arc<UploadPtyBypass>>>>,
    ) -> bool {
        let __t0 = std::time::Instant::now();
        let variant = ShellPumpCommand::variant_name(&cmd);
        let (data_n, pty_seq, queue_wait_ms, input_kind) = match &cmd {
            ShellPumpCommand::PtyInput {
                seq,
                enqueued_at,
                data,
            } => (
                data.len(),
                Some(*seq),
                Some(enqueued_at.elapsed().as_millis() as u64),
                Some(super::pty_input_kind(data)),
            ),
            ShellPumpCommand::ZmodemWrite(v) => (v.len(), None, None, None),
            _ => (0, None, None, None),
        };
        let mut ch = match channel.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!("shell pump: channel mutex poisoned: {}", e);
                return false;
            }
        };
        let __t_lock = __t0.elapsed();
        let mut __stage_read = std::time::Duration::ZERO;
        let mut __stage_write = std::time::Duration::ZERO;
        if let (Some(seq), Some(wait_ms), Some(kind)) = (pty_seq, queue_wait_ms, input_kind) {
            if wait_ms >= 50 {
                log::warn!(
                    "INPUT_DEQ session={} seq={} kind={} n={} queue_wait_ms={}",
                    session_id,
                    seq,
                    kind,
                    data_n,
                    wait_ms
                );
            } else {
                log::debug!(
                    "INPUT_DEQ session={} seq={} kind={} n={} queue_wait_ms={}",
                    session_id,
                    seq,
                    kind,
                    data_n,
                    wait_ms
                );
            }
        } else if matches!(cmd, ShellPumpCommand::ZmodemWrite(_)) {
            log::debug!(
                "PUMP_CONSUME session={} variant={} data_n={}",
                session_id,
                variant,
                data_n
            );
        } else if matches!(cmd, ShellPumpCommand::ExecRemote { .. } | ShellPumpCommand::SessionJob(_))
        {
            log::warn!(
                "PUMP_BLOCK session={} variant={} (PTY input will queue behind this)",
                session_id,
                variant
            );
        }
        match cmd {
            ShellPumpCommand::PtyInput { data, seq, .. } => {
                let __s = std::time::Instant::now();
                if SshManager::pump_channel_reads(
                    &mut *ch,
                    read_buffer,
                    message_tx,
                    session_id,
                    upload_bypass,
                    None,
                )
                .is_err()
                {
                    return false;
                }
                __stage_read = __s.elapsed();
                let __s = std::time::Instant::now();
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
                __stage_write = __s.elapsed();
                let write_ms = __stage_write.as_millis() as u64;
                let read_ms = __stage_read.as_millis() as u64;
                if write_ms >= 20 || read_ms >= 20 || queue_wait_ms.unwrap_or(0) >= 50 {
                    log::warn!(
                        "INPUT_WRITE session={} seq={} n={} read_ms={} write_ms={} queue_wait_ms={}",
                        session_id,
                        seq,
                        data.len(),
                        read_ms,
                        write_ms,
                        queue_wait_ms.unwrap_or(0)
                    );
                } else {
                    log::debug!(
                        "INPUT_WRITE session={} seq={} n={} read_ms={} write_ms={}",
                        session_id,
                        seq,
                        data.len(),
                        read_ms,
                        write_ms
                    );
                }
            }
            ShellPumpCommand::ZmodemWrite(data) => {
                let n = data.len();
                let __s = std::time::Instant::now();
                let w = SshManager::write_pty_with_drain(
                    &mut *ch,
                    &data,
                    read_buffer,
                    message_tx,
                    session_id,
                    upload_bypass,
                );
                __stage_write = __s.elapsed();
                if let Err(e) = w {
                    log::error!(
                        "shell 泵 session={} ZmodemWrite 失败 n={} {}",
                        session_id,
                        n,
                        e
                    );
                }
            }
            ShellPumpCommand::ExecRemote { cmd, reply } => {
                drop(ch);
                let res = mgr.exec_remote(session_id, &cmd);
                let _ = reply.send(res);
            }
            ShellPumpCommand::SessionJob(job) => {
                drop(ch);
                if let Some(session) = mgr.get_session(session_id) {
                    job(&session);
                } else {
                    log::warn!(
                        "shell 泵 session={} SessionJob 找不到会话，已丢弃",
                        session_id
                    );
                }
            }
        }
        let __total = __t0.elapsed();
        if __total.as_millis() as u64 >= 50 {
            log::warn!(
                "PUMP_SLOW session={} variant={} data_n={} total_ms={} lock_ms={} read_ms={} write_ms={}",
                session_id,
                variant,
                data_n,
                __total.as_millis(),
                __t_lock.as_millis(),
                __stage_read.as_millis(),
                __stage_write.as_millis(),
            );
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
                            log::info!("shell input session={} cmd={}", session_id, cmd);
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
            None,
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
            tx.send(ShellPumpCommand::PtyInput {
                seq: 1,
                enqueued_at: std::time::Instant::now(),
                data: vec![0x61],
            })
            .unwrap();
            tx.send(ShellPumpCommand::ZmodemWrite(vec![0x2a, 0x2a]))
                .unwrap();
            drop(tx);
            match rx.recv().unwrap() {
                ShellPumpCommand::PtyInput { data: v, .. } => assert_eq!(v, vec![0x61]),
                other => panic!("unexpected: {:?}", other),
            }
            match rx.recv().unwrap() {
                ShellPumpCommand::ZmodemWrite(v) => assert_eq!(v, vec![0x2a, 0x2a]),
                other => panic!("unexpected: {:?}", other),
            }
            assert!(rx.recv().is_err());
        }

        #[test]
        fn bounded_sync_pump_queue_backpressure() {
            let (tx, rx) = sync_channel::<ShellPumpCommand>(2);
            tx.send(ShellPumpCommand::PtyInput {
                seq: 1,
                enqueued_at: std::time::Instant::now(),
                data: vec![1],
            })
            .unwrap();
            tx.send(ShellPumpCommand::PtyInput {
                seq: 2,
                enqueued_at: std::time::Instant::now(),
                data: vec![2],
            })
            .unwrap();
            let tx_c = tx.clone();
            let fill = thread::spawn(move || {
                tx_c
                    .send(ShellPumpCommand::PtyInput {
                        seq: 3,
                        enqueued_at: std::time::Instant::now(),
                        data: vec![3],
                    })
                    .unwrap();
            });
            thread::sleep(Duration::from_millis(20));
            match rx.recv().unwrap() {
                ShellPumpCommand::PtyInput { data: v, .. } => assert_eq!(v, vec![1]),
                other => panic!("unexpected: {:?}", other),
            }
            match rx.recv().unwrap() {
                ShellPumpCommand::PtyInput { data: v, .. } => assert_eq!(v, vec![2]),
                other => panic!("unexpected: {:?}", other),
            }
            fill.join().unwrap();
            match rx.recv().unwrap() {
                ShellPumpCommand::PtyInput { data: v, .. } => assert_eq!(v, vec![3]),
                other => panic!("unexpected: {:?}", other),
            }
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
