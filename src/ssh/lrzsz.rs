//! lrzsz 文件传输：远端 `sz`→本机接收由 `zmodem2::Receiver`；本机→远端 `rz` 发送默认用 `zmodem2::Sender`（`lrzsz_zmodem2_send`），
//! 可选 `MISTTERM_ZMODEM_USE_EXTERNAL_SZ=1`：临时调用本机 **`sz`** 做回归对照（产品仍以内置 `zmodem2` 为主）。

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use ssh2::Channel;
use super::manager::{ShellPumpCommand, ShellPumpTx, SshSessionHandle};

const RX_INCOMING_CAP: usize = 512 * 1024;

/// 会话结束后继续吞掉的协议残尾上限（`**B08…` / 收尾 XON 等）。超过即视为远端又开始说话。
const RX_TAIL_RESIDUE_CAP: usize = 4 * 1024;

/// 单次 `sz`→本机会话：`zmodem2::Receiver` + 未解析完的 PTY 字节
struct ZmodemRxSession {
    receiver: zmodem2::Receiver,
    incoming: Vec<u8>,
    last_save_path: Option<PathBuf>,
    /// MistTerm (#458): `SessionComplete` 后继续经 `feed_receive_data` 吃掉的尾字节。
    ///
    /// `zmodem2::Receiver` 在 `ZFIN` 上置 `SessionEnd` 并报 `SessionComplete`，
    /// 但远端 `sz` 的 ZFIN 应答 / 收尾 `XON` 常和**后续 PTY 分片**一起到达。
    /// 若此时立刻清空 `recv_side`，这批残余就落进 VTE 与日志（典型症状：提示符前多出
    /// 字面 `00`、日志里留下未消费的 `**B08…`）。
    residue: Vec<u8>,
    /// 接收结束时刻（`SessionComplete`）；用于给残尾一个有限的吞噬窗口。
    ended_at: Option<Instant>,
}

/// 被动接收：先攒字节直到出现 ZRQINIT，再建 `zmodem2::Receiver`（其 `new` 会排队 ZRINIT）
enum RecvSide {
    Passive { pending: Vec<u8> },
    Active(ZmodemRxSession),
}

impl ZmodemRxSession {
    fn new(receiver: zmodem2::Receiver, incoming: Vec<u8>) -> Self {
        Self {
            receiver,
            incoming,
            last_save_path: None,
            residue: Vec::new(),
            ended_at: None,
        }
    }
}

fn is_hex_digit(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

/// ZMODEM 控制里「无害的空转字节」：帧填充、ZDLE、收尾 XON、行尾填充。
fn rx_pad_byte(b: u8) -> bool {
    matches!(
        b,
        zmodem::ZPAD | 0x80 | zmodem::ZDLE | 0x11 | b'\r' | b'\n' | b'\t' | b' ' | b'|'
    )
}

/// 只可能出现在协议流里的 C0 控制符（不含 `\r` `\n` `\t`，也不含 XON）。
fn rx_drop_byte(b: u8) -> bool {
    matches!(b, 0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x17 | 0x19..=0x1f)
}

/// 提示符片段：与 `terminal.rs` 的 `contains_shell_prompt_fragment` 同口径。
fn prompt_fragment_pos(text: &[u8]) -> Option<usize> {
    text.windows(2)
        .position(|w| matches!(w, [b'$', b' '] | [b'#', b' '] | [b'>', b' ']))
}

/// 从 `data[..]` 里取提示符行（含）之后的部分的起点；没有提示符则返回 `None`。
fn prompt_line_start(text: &[u8]) -> Option<usize> {
    prompt_fragment_pos(text)?;
    let mut start = 0usize;
    for (i, b) in text.iter().enumerate() {
        if *b == b'\n' {
            start = i + 1;
        }
    }
    Some(start)
}

/// 消费 `data` 开头属于 ZMODEM 收尾残尾的字节数（#458）。
///
/// `SessionComplete` 之后，远端 `sz` 的收尾 `**B08…` ZHEX 头 / XON / `\r\n`
/// 常与下一段 PTY 分片一起到达。这些字节既不是文件载荷，也不是终端该显示的内容，
/// 必须在本机侧吃掉，否则会落到 VTE 与会话日志里（提示符前的字面 `00`）。
///
/// 只看**开头**：一旦遇到可见文本（如 `user@host:~$`），立刻停下，
/// 返回已吃掉的长度，剩下的原样交给显示路径。
/// 残尾吞噬窗口：`SessionComplete` 后这段时间内到达的协议字节都视为 ZMODEM 收尾。
const RX_TAIL_WINDOW: Duration = Duration::from_millis(1500);

/// `feed_receive_data` 的消费结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiveFeed {
    /// 整段由 ZMODEM 接收状态机 / 残尾吞噬消费，**不得**进 VTE 与会话日志。
    Consumed,
    /// 与本接收无关，按普通 PTY 输出处理。
    Passthrough { data: Vec<u8> },
    /// 头部协议残尾已剥离，剩余字节按普通 PTY 输出处理。
    Stripped { rest: Vec<u8> },
}

fn rx_residue_prefix_len(data: &[u8], budget: usize) -> usize {
    let mut i = 0usize;
    let mut in_protocol = false;
    while i < data.len() && i < budget {
        let b = data[i];
        if rx_drop_byte(b) {
            i += 1;
            in_protocol = true;
            continue;
        }
        if rx_pad_byte(b) {
            i += 1;
            in_protocol = true;
            continue;
        }
        if is_hex_digit(b) {
            let start = i;
            while i < data.len() && is_hex_digit(data[i]) {
                i += 1;
            }
            let next = data.get(i).copied();
            if matches!(next, Some(b'\r') | Some(b'\n') | Some(zmodem::XON)) {
                // 整行 hex 后紧跟帧结束符：只在前面已经有协议字节时才认为是残尾，
                // 否则可能只是普通输出（例如纯数字行）。
                if in_protocol {
                    continue;
                }
                return start;
            }
            let rest = &data[i..];
            // 被分片截断的收尾 hex：紧接着就是提示符（`**B08…` 的 `0800000000022d`，
            // 或提示符前粘着的 `00`），整段丢掉。
            if prompt_fragment_pos(rest).is_some() && (in_protocol || start == 0) {
                continue;
            }
            // 协议前缀后的 hex，或整段都是截断 hex（单独到达的 `**B08…` / `00`），
            // 同样视为残尾，避免落入 VTE 与会话日志。
            if in_protocol || (start == 0 && i == data.len()) {
                in_protocol = true;
                continue;
            }
            return start;
        }
        // 可见文本：交给显示路径。
        return i;
    }
    i
}

fn disambiguate_download_path(download_dir: &Path, raw_name: &[u8]) -> Result<PathBuf, String> {
    let base = String::from_utf8_lossy(raw_name);
    let only = Path::new(base.trim())
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("zmodem.bin");
    let mut file_path = download_dir.join(only);
    let mut counter = 1u32;
    while file_path.exists() {
        let stem = file_path.file_stem().unwrap_or_default().to_string_lossy();
        let ext = file_path.extension().unwrap_or_default().to_string_lossy();
        file_path = if ext.is_empty() {
            download_dir.join(format!("{}_{}", stem, counter))
        } else {
            download_dir.join(format!("{}_{}.{}", stem, counter, ext))
        };
        counter = counter.saturating_add(1);
    }
    Ok(file_path)
}

/// ZMODEM 常量：仅保留 `rz` 检测与 ZRQINIT 扫描所需项（协议编解码在 `zmodem2`）。
mod zmodem {
    pub const ZPAD: u8 = b'*';
    pub const ZDLE: u8 = 0x18;
    /// 收尾 XON（DC1），ZHEX 帧末尾常见
    pub const XON: u8 = 0x11;
    /// HEX 帧编码符 `B`（接收端 rz 在 PTY 上几乎总是发 HEX 头）
    pub const ZHEX: u8 = 0x42;
    /// BIN16 帧编码符 `A`
    pub const ZBIN16: u8 = 0x41;
    pub const ZBIN32: u8 = 0x43;

    pub const ZRQINIT: u8 = 0x00;
    /// 部分实现用 `ZVBIN 'a'` 代替 `ZBIN 'A'` 发 BIN16 头
    pub const ZVBIN16: u8 = b'a';
}

fn is_zpad(b: u8) -> bool {
    b == zmodem::ZPAD || b == 0x80
}

#[inline]
fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

fn hex_pair(hi: u8, lo: u8) -> Option<u8> {
    Some(hex_nibble(hi)? * 16 + hex_nibble(lo)?)
}

/// BIN16 头里的 TYPE 在链路上的编码（lrzsz 在 ZRINIT 里常设 ESCCTL，要求对 `<0x20` 做 ZDLE 转义，如 ZFILE=0x04→`\x18D`）
#[inline]
fn bin16_wire_decode_type(data: &[u8], idx: usize) -> Option<u8> {
    let b = *data.get(idx)?;
    if b == zmodem::ZDLE {
        Some(*data.get(idx + 1)? ^ 0x40)
    } else {
        Some(b)
    }
}

/// 在缓冲区内扫描所有 `** ZDLE (B|b)` HEX 头，是否出现 `want` 帧类型。
fn hex_scan_for_type(data: &[u8], want: u8) -> bool {
    // 至少 6 字节：`** ZDLE 'B' HH`（HH 为类型十六进制对）
    for i in 0..data.len().saturating_sub(5) {
        if !is_zpad(data[i]) || !is_zpad(data[i + 1]) || data[i + 2] != zmodem::ZDLE {
            continue;
        }
        let hb = data[i + 3];
        if hb != zmodem::ZHEX && hb != b'B' && hb != b'b' {
            continue;
        }
        if let Some(t) = hex_pair(data[i + 4], data[i + 5]) {
            if t == want {
                return true;
            }
        }
    }
    false
}

/// 二进制 BIN16/32 头：`ZPAD{1,2} ZDLE ('A'|'a'|'C') TYPE`（规范为单 ZPAD；lrzsz 常见双 ZPAD）
fn binary_frame_type(data: &[u8], want: u8) -> bool {
    for binch in [zmodem::ZBIN16, zmodem::ZVBIN16, zmodem::ZBIN32] {
        // 双 ZPAD：`** ZDLE binch TYPE`（至少 5 字节）
        for i in 0..data.len().saturating_sub(4) {
            if !is_zpad(data[i])
                || !is_zpad(data[i + 1])
                || data[i + 2] != zmodem::ZDLE
                || data[i + 3] != binch
            {
                continue;
            }
            if bin16_wire_decode_type(data, i + 4).is_some_and(|t| t == want) {
                return true;
            }
        }
        // 单 ZPAD：`* ZDLE binch TYPE`（ZMODEM 规范；否则收不到 rz 的 BIN16 ZACK）
        for i in 0..data.len().saturating_sub(3) {
            if !is_zpad(data[i])
                || data[i + 1] != zmodem::ZDLE
                || data[i + 2] != binch
            {
                continue;
            }
            if bin16_wire_decode_type(data, i + 3).is_some_and(|t| t == want) {
                return true;
            }
        }
    }
    false
}

/// 传输事件
#[derive(Debug, Clone)]
pub enum TransferEvent {
    /// `outgoing`: 本机→远端（`rz` 上传）为 `true`；远端 `sz` 下发到本机为 `false`
    FileStart {
        filename: String,
        size: u64,
        outgoing: bool,
    },
    FileProgress { filename: String, received: u64, total: u64 },
    FileComplete { filename: String, path: PathBuf },
    FileError { filename: String, error: String },
    TransferComplete,
}

/// Shell 泵线程与 UI 共用同一 `upload_pty_rx`；注册后由泵在 `channel.read` 时**同步**旁路，
/// 避免经 `mpsc→UI→feed_send` 一帧延迟导致上传线程错过 `write_pty_with_drain` 期间的入站。
pub struct UploadPtyBypass {
    upload_pty_rx: Arc<Mutex<Vec<u8>>>,
    upload_pty_capture_on: Arc<AtomicBool>,
    upload_pty_feed_bytes: Arc<AtomicU64>,
}

impl UploadPtyBypass {
    pub(crate) fn feed_from_shell_pump(&self, data: &[u8]) {
        if !self.upload_pty_capture_on.load(Ordering::Relaxed) || data.is_empty() {
            return;
        }
        const CAP: usize = 2 * 1024 * 1024;
        let mut q = self.upload_pty_rx.lock().unwrap();
        if q.len() >= CAP {
            log::warn!(
                "upload_pty_rx 已满 {} bytes（shell 泵旁路），丢弃本段 {} bytes",
                CAP,
                data.len()
            );
            return;
        }
        let take = (CAP - q.len()).min(data.len());
        q.extend_from_slice(&data[..take]);
        self.upload_pty_feed_bytes
            .fetch_add(take as u64, Ordering::Relaxed);
    }
}

/// lrzsz 传输器
pub struct LrzszTransfer {
    rx: Arc<Mutex<Receiver<TransferEvent>>>,
    tx: Sender<TransferEvent>,
    is_active: Arc<AtomicBool>,
    received_bytes: Arc<AtomicU64>,
    total_bytes: Arc<AtomicU64>,
    current_filename: Arc<Mutex<String>>,
    download_dir: PathBuf,
    /// 远端 `sz`→本机：被动攒包 / 主动 `zmodem2::Receiver` 会话
    recv_side: Arc<Mutex<Option<RecvSide>>>,
    // 新增：SSH 通道引用
    channel: Arc<Mutex<Option<Arc<Mutex<Channel>>>>>,
    receive_pump_tx: Arc<Mutex<Option<ShellPumpTx>>>,
    // 新增：当前接收的文件句柄
    receive_file: Arc<Mutex<Option<File>>>,
    /// rz→本机（上传）时，PTY 上来自远端的 ZMODEM 帧只能经 SSH 泵线程读出；
    /// 发送线程不得再对同一 `Channel` 并发 `read`，否则永远读不到握手包。
    upload_pty_rx: Arc<Mutex<Vec<u8>>>,
    upload_pty_capture_on: Arc<AtomicBool>,
    /// UI `feed_send_pty_output` 累计入队字节（单次上传内由 `run_upload_zmodem2` 起始归零）
    upload_pty_feed_bytes: Arc<AtomicU64>,
    /// 上传线程 `pull_pty_into` 累计拉取字节（同上）
    upload_pty_pull_bytes: Arc<AtomicU64>,
    /// `true` 时 PTY 入队已由 shell 泵同步写入 `upload_pty_rx`，UI 侧 `feed_send_pty_output` 须跳过以免重复。
    upload_feed_via_shell_pump: Arc<AtomicBool>,
}

impl LrzszTransfer {
    /// 更换 `sz` 接收保存目录（偏好「默认本地目录」改动后，下次接收生效）。
    pub fn set_download_dir(&mut self, dir: PathBuf) {
        self.download_dir = dir;
    }

    /// 创建新的传输器
    pub fn new(download_dir: &str) -> Self {
        let (tx, rx) = channel();
        let download_path = PathBuf::from(download_dir);
        
        // 创建下载目录
        let _ = fs::create_dir_all(&download_path);
        
        Self {
            rx: Arc::new(Mutex::new(rx)),
            tx,
            is_active: Arc::new(AtomicBool::new(false)),
            received_bytes: Arc::new(AtomicU64::new(0)),
            total_bytes: Arc::new(AtomicU64::new(0)),
            current_filename: Arc::new(Mutex::new(String::new())),
            download_dir: download_path,
            recv_side: Arc::new(Mutex::new(None)),
            channel: Arc::new(Mutex::new(None)),
            receive_pump_tx: Arc::new(Mutex::new(None)),
            receive_file: Arc::new(Mutex::new(None)),
            upload_pty_rx: Arc::new(Mutex::new(Vec::new())),
            upload_pty_capture_on: Arc::new(AtomicBool::new(false)),
            upload_pty_feed_bytes: Arc::new(AtomicU64::new(0)),
            upload_pty_pull_bytes: Arc::new(AtomicU64::new(0)),
            upload_feed_via_shell_pump: Arc::new(AtomicBool::new(false)),
        }
    }

    fn upload_pty_bypass_arc(&self) -> Arc<UploadPtyBypass> {
        Arc::new(UploadPtyBypass {
            upload_pty_rx: self.upload_pty_rx.clone(),
            upload_pty_capture_on: self.upload_pty_capture_on.clone(),
            upload_pty_feed_bytes: self.upload_pty_feed_bytes.clone(),
        })
    }

    /// 与 [`Self::begin_rz_handshake_capture`] 配套：在首次 `feed_send_pty_output` **之后**调用，使泵线程旁路与上传线程无帧延迟。
    pub fn register_shell_pump_upload_feed(&self, handle: &SshSessionHandle) {
        handle.set_upload_pty_bypass(Some(self.upload_pty_bypass_arc()));
        self.upload_feed_via_shell_pump
            .store(true, Ordering::Relaxed);
    }

    /// 结束握手 / 传输完成 / 断开前注销，恢复仅 UI 旁路路径。
    pub fn unregister_shell_pump_upload_feed(&self, handle: &SshSessionHandle) {
        handle.set_upload_pty_bypass(None);
        self.upload_feed_via_shell_pump
            .store(false, Ordering::Relaxed);
    }

    /// 将 SSH 泵线程读到的 PTY 输出旁路一份给「本机 sz→远端 rz」上传握手/确认解析（与 `start_send` 配对）。
    /// 本机→远端 `rz` 上传进行中：PTY 读出需旁路给 ZMODEM 解析，**不得**再送入终端模拟器。
    #[inline]
    pub fn is_upload_pty_capture(&self) -> bool {
        self.upload_pty_capture_on.load(Ordering::Relaxed)
    }

    /// 检测到远端 `rz` 提示后尽早打开 PTY 旁路，把 ZRQINIT 等握手字节攒进 `upload_pty_rx`，避免选文件前几秒的字节只进 VTE 而被 `start_send` 错过。
    pub fn begin_rz_handshake_capture(&self) {
        self.upload_pty_capture_on.store(true, Ordering::Relaxed);
    }

    /// 用户取消上传文件选择时关闭旁路并丢弃攒包（须与 `send_ctrl_c` 等配合）。
    pub fn end_rz_handshake_capture(&self) {
        self.upload_pty_capture_on.store(false, Ordering::Relaxed);
        self.upload_pty_rx.lock().unwrap().clear();
    }

    pub fn feed_send_pty_output(&self, data: &[u8]) {
        if !self.upload_pty_capture_on.load(Ordering::Relaxed) || data.is_empty() {
            return;
        }
        if self.upload_feed_via_shell_pump.load(Ordering::Relaxed) {
            return;
        }
        const CAP: usize = 2 * 1024 * 1024;
        let mut q = self.upload_pty_rx.lock().unwrap();
        if q.len() >= CAP {
            log::warn!(
                "upload_pty_rx 已满 {} bytes，丢弃本段 {} bytes（请检查泵线程与上传锁竞争）",
                CAP,
                data.len()
            );
            return;
        }
        let take = (CAP - q.len()).min(data.len());
        q.extend_from_slice(&data[..take]);
        self.upload_pty_feed_bytes
            .fetch_add(take as u64, Ordering::Relaxed);
        if std::env::var("MISTTERM_ZMODEM_PTY_LOG")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        {
            log::info!(
                "旁路 feed_send_pty 本段={} B 队尾={} B 累计入队={} B",
                take,
                q.len(),
                self.upload_pty_feed_bytes.load(Ordering::Relaxed)
            );
        }
    }

    /// 检查是否正在传输
    pub fn is_active(&self) -> bool {
        self.is_active.load(Ordering::Relaxed)
    }

    /// 用户 Ctrl+C 等：将本机侧传输协作标记为停止（须与发往 PTY 的 `0x03` 一起使用，远端 `rz`/`sz` 才会退出）。
    pub fn cancel_active_transfer(&self) {
        self.is_active.store(false, Ordering::Relaxed);
        *self.recv_side.lock().unwrap() = None;
        *self.receive_file.lock().unwrap() = None;
        *self.receive_pump_tx.lock().unwrap() = None;
    }

    /// 获取接收进度
    pub fn get_progress(&self) -> (u64, u64) {
        (
            self.received_bytes.load(Ordering::Relaxed),
            self.total_bytes.load(Ordering::Relaxed)
        )
    }

    /// 获取当前文件名
    pub fn get_filename(&self) -> String {
        self.current_filename.lock().unwrap().clone()
    }

    /// 获取接收事件
    pub fn try_recv_event(&self) -> Option<TransferEvent> {
        self.rx.lock().ok()?.try_recv().ok()
    }

    /// 检测终端输出中是否包含 rz 命令触发序列
    pub fn detect_rz_command(&self, data: &[u8]) -> bool {
        // 检测常见的 rz 触发模式
        let text = String::from_utf8_lossy(data);
        
        // 文本模式：需要明确的 rz 等待提示
        if text.contains("rz rz rz") || 
           text.contains("Awaiting rz") ||
           text.contains("rz waiting to receive") {
            return true;
        }
        
        false
    }

    /// 远端 `sz` 会先发 ZRQINIT；这是下载接收，不应当触发 `rz` 上传选文件。
    pub fn detect_zmodem_download(&self, data: &[u8]) -> bool {
        parse_zrqinit_packet(data)
    }

    /// 远端 `rz` 在 PTY 上的握手邀请（含 Windows ASCII `**B01…`）。
    pub fn detect_rz_pty_invitation(&self, data: &[u8]) -> bool {
        detect_rz_pty_invitation(data)
    }

    /// Windows `rz -bye` 在 PTY 上发 ASCII `**B01…`（早于本机 Enter 回显）；与远端 `sz` 的 ZRQINIT `**B00` 区分。
    pub fn detect_windows_rz_wait_invitation(&self, data: &[u8]) -> bool {
        detect_windows_rz_wait_invitation(data)
    }

    /// 开始接收（被动）：等 PTY 上出现 ZRQINIT 后再建 `zmodem2::Receiver` 并回 ZRINIT。
    pub fn start_receive(&self, channel: Arc<Mutex<Channel>>) -> Result<(), String> {
        if self.is_active.load(Ordering::Relaxed) {
            return Err("Transfer already in progress".to_string());
        }

        self.is_active.store(true, Ordering::Relaxed);
        self.received_bytes.store(0, Ordering::Relaxed);
        self.total_bytes.store(0, Ordering::Relaxed);
        *self.channel.lock().unwrap() = Some(channel);
        *self.receive_pump_tx.lock().unwrap() = None;
        *self.recv_side.lock().unwrap() = Some(RecvSide::Passive {
            pending: Vec::new(),
        });
        log::info!("ZMODEM receive started (passive, waiting for ZRQINIT)");
        Ok(())
    }

    /// 主动开始接收：立即建 `zmodem2::Receiver`（排队 ZRINIT）并刷到 SSH 通道。
    pub fn start_receive_active(&self, channel: Arc<Mutex<Channel>>) -> Result<(), String> {
        if self.is_active.load(Ordering::Relaxed) {
            return Err("Transfer already in progress".to_string());
        }

        self.is_active.store(true, Ordering::Relaxed);
        self.received_bytes.store(0, Ordering::Relaxed);
        self.total_bytes.store(0, Ordering::Relaxed);
        *self.channel.lock().unwrap() = Some(channel);
        *self.receive_pump_tx.lock().unwrap() = None;

        let receiver = zmodem2::Receiver::new().map_err(|e| format!("ZMODEM Receiver::new: {}", e))?;
        let mut session = ZmodemRxSession::new(receiver, Vec::new());
        self.flush_receiver_outgoing(&mut session.receiver)?;
        *self.recv_side.lock().unwrap() = Some(RecvSide::Active(session));
        log::info!("ZMODEM receive started (active, ZRINIT sent)");
        Ok(())
    }

    /// 通过 shell pump 写回接收端 ACK（交互式 shell 使用同一 PTY FIFO）。
    pub fn start_receive_pump(&self, pump_tx: ShellPumpTx) -> Result<(), String> {
        if self.is_active.load(Ordering::Relaxed) {
            return Err("Transfer already in progress".to_string());
        }

        self.is_active.store(true, Ordering::Relaxed);
        self.received_bytes.store(0, Ordering::Relaxed);
        self.total_bytes.store(0, Ordering::Relaxed);
        *self.channel.lock().unwrap() = None;
        *self.receive_pump_tx.lock().unwrap() = Some(pump_tx);
        *self.recv_side.lock().unwrap() = Some(RecvSide::Passive {
            pending: Vec::new(),
        });
        log::info!("ZMODEM receive started via shell pump (passive, waiting for ZRQINIT)");
        Ok(())
    }

    /// 向 SSH 通道写 ZMODEM 出站字节（`zmodem2::Receiver::drain_outgoing`）
    fn write_to_channel(&self, data: &[u8]) {
        if let Ok(pump_guard) = self.receive_pump_tx.lock() {
            if let Some(ref pump_tx) = *pump_guard {
                let _ = pump_tx.send(ShellPumpCommand::ZmodemWrite(data.to_vec()));
                log::debug!("ZMODEM queued {} bytes via shell pump", data.len());
                return;
            }
        }
        if let Ok(chan_lock) = self.channel.lock() {
            if let Some(ref chan) = *chan_lock {
                if let Ok(mut c) = chan.lock() {
                    let _ = c.write_all(data);
                    log::debug!("ZMODEM wrote {} bytes to channel", data.len());
                }
            }
        }
    }

    fn flush_receiver_outgoing(&self, receiver: &mut zmodem2::Receiver) -> Result<(), String> {
        loop {
            let out = receiver.drain_outgoing();
            if out.is_empty() {
                break;
            }
            let n = out.len();
            self.write_to_channel(out);
            receiver.advance_outgoing(n);
        }
        Ok(())
    }

    fn drain_receiver_file(&self, receiver: &mut zmodem2::Receiver) -> Result<bool, String> {
        let mut wrote = false;
        loop {
            let chunk = receiver.drain_file();
            if chunk.is_empty() {
                break;
            }
            let mut f_guard = self
                .receive_file
                .lock()
                .map_err(|_| "receive_file mutex poison".to_string())?;
            if let Some(ref mut file) = *f_guard {
                file.write_all(chunk).map_err(|e| e.to_string())?;
            }
            let n = chunk.len();
            receiver.advance_file(n).map_err(|e| e.to_string())?;
            wrote = true;
            let r = self.received_bytes.fetch_add(n as u64, Ordering::Relaxed) + n as u64;
            let name = self.current_filename.lock().unwrap().clone();
            let total = self.total_bytes.load(Ordering::Relaxed);
            let _ = self.tx.send(TransferEvent::FileProgress {
                filename: name,
                received: r,
                total,
            });
        }
        Ok(wrote)
    }

    /// 处理 `poll_event`；返回 `true` 表示本会话已 `SessionComplete`。
    fn poll_receiver_events(
        &self,
        session: &mut ZmodemRxSession,
    ) -> Result<bool, String> {
        loop {
            let ev = match session.receiver.poll_event() {
                Some(ev) => ev,
                None => return Ok(false),
            };
            match ev {
                zmodem2::ReceiverEvent::FileStart => {
                    *self.receive_file.lock().unwrap() = None;
                    let raw_name = session.receiver.file_name().to_vec();
                    let size = session.receiver.file_size() as u64;
                    let name = String::from_utf8_lossy(&raw_name).into_owned();
                    *self.current_filename.lock().unwrap() = name.clone();
                    self.total_bytes.store(size, Ordering::Relaxed);
                    self.received_bytes.store(0, Ordering::Relaxed);
                    let path = disambiguate_download_path(&self.download_dir, &raw_name)?;
                    let file = File::create(&path).map_err(|e| e.to_string())?;
                    session.last_save_path = Some(path.clone());
                    *self.receive_file.lock().unwrap() = Some(file);
                    let _ = self.tx.send(TransferEvent::FileStart {
                        filename: name,
                        size,
                        outgoing: false,
                    });
                }
                zmodem2::ReceiverEvent::FileComplete => {
                    *self.receive_file.lock().unwrap() = None;
                    if let Some(p) = session.last_save_path.take() {
                        let fname = self.current_filename.lock().unwrap().clone();
                        let _ = self.tx.send(TransferEvent::FileComplete {
                            filename: fname,
                            path: p,
                        });
                    }
                }
                zmodem2::ReceiverEvent::SessionComplete => {
                    // MistTerm (#458)：**不**在这里清掉 `recv_side`。
                    // `ZFIN` 已让 receiver 进入 `SessionEnd`，但远端 `sz` 的收尾
                    // `**B08…` ZHEX 头 / XON 往往与下一段 PTY 分片一起到达；
                    // 若立刻清空接收侧，这些字节就会落进 VTE 与会话日志，
                    // 表现为提示符前的字面 `00` 与日志里未消费的 `**B…` 尾巴。
                    // 会话保留到残尾窗口结束，由 `absorb_rx_tail` 吃掉收尾字节。
                    *self.receive_file.lock().unwrap() = None;
                    *self.receive_pump_tx.lock().unwrap() = None;
                    self.is_active.store(false, Ordering::Relaxed);
                    if session.ended_at.is_none() {
                        session.ended_at = Some(Instant::now());
                    }
                    let _ = self.tx.send(TransferEvent::TransferComplete);
                    return Ok(true);
                }
            }
        }
    }

    /// `SessionComplete` 之后继续从入站里剥掉 ZMODEM 收尾残尾（#458）。
    ///
    /// 返回 `Some(rest)`：`rest` 应作为普通 PTY 输出继续显示；残尾全部吃掉时 `rest` 为空。
    fn absorb_rx_tail(&self, session: &mut ZmodemRxSession, data: &[u8]) -> Option<Vec<u8>> {
        let ended_at = session.ended_at?;
        if ended_at.elapsed() > RX_TAIL_WINDOW {
            // 窗口结束：不再吞字节，交给正常显示路径。
            return Some(data.to_vec());
        }
        let budget = RX_TAIL_RESIDUE_CAP.saturating_sub(session.residue.len());
        let eaten = rx_residue_prefix_len(data, budget);
        if eaten > 0 {
            session.residue.extend_from_slice(&data[..eaten]);
            log::debug!(
                "ZMODEM receive: 吞掉收尾残尾 {} B（累计 {} B）",
                eaten,
                session.residue.len()
            );
        }
        let rest = &data[eaten..];
        if !session.residue.is_empty() && prompt_fragment_pos(rest).is_some() {
            // 提示符已到：残尾到此为止，结束会话，让提示符正常显示。
            session.ended_at = None;
            return Some(rest.to_vec());
        }
        if rest.is_empty() {
            return Some(Vec::new());
        }
        // 中途到达的可见输出：残尾窗口继续，但这段字节照常显示。
        Some(rest.to_vec())
    }

    fn drive_rx_session(&self, session: &mut ZmodemRxSession) -> Result<bool, String> {
        for _ in 0..256 {
            self.flush_receiver_outgoing(&mut session.receiver)?;
            let mut progress = false;
            let mut resynced = false;
            if !session.incoming.is_empty() {
                match session.receiver.feed_incoming(&session.incoming) {
                    Ok(c) => {
                        if c > 0 {
                            session.incoming.drain(..c);
                            progress = true;
                        }
                    }
                    Err(e) => {
                        // MistTerm (#450): a corrupted frame / malformed byte must
                        // not abort the whole transfer. `feed_incoming` has queued
                        // a ZNAK; flush it so the peer retransmits, drop the
                        // consumed prefix (up to the resync point) and continue.
                        log::warn!(
                            "ZMODEM receive recovered from protocol error: {} (queue={} B)",
                            e,
                            session.incoming.len()
                        );
                        session.incoming.clear();
                        resynced = true;
                    }
                }
            }
            self.flush_receiver_outgoing(&mut session.receiver)?;
            if resynced {
                // Give the peer a moment to react to the NAK; avoid a hot loop
                // re-feeding the same corrupted backlog.
                break;
            }
            // 先处理 FileStart / SessionComplete，再 drain_file，避免首包已到时尚未建文件
            if self.poll_receiver_events(session)? {
                return Ok(true);
            }
            if self.drain_receiver_file(&mut session.receiver)? {
                progress = true;
            }
            if self.poll_receiver_events(session)? {
                return Ok(true);
            }
            if !progress {
                break;
            }
        }
        Ok(false)
    }

    fn abort_recv_locked(&self, slot: &mut Option<RecvSide>, msg: &str) {
        // MistTerm (#450): on failure, actively end the session on the wire —
        // CAN CAN CAN CAN CAN + ZABORT (CAN*5 per ZMODEM spec) — so the remote
        // `sz` stops instead of continuing to spew binary into the terminal,
        // and GUI Ctrl+C regains the prompt without needing a second session.
        const CAN: u8 = 0x18;
        let mut abort_bytes = Vec::with_capacity(16);
        abort_bytes.extend_from_slice(&[CAN, CAN, CAN, CAN, CAN]);
        // ZABORT header (hex-encoded with correct CRC) built via zmodem2's encoder.
        let header = zmodem2::Header::new(
            zmodem2::Encoding::ZHEX,
            zmodem2::Frame::ZABORT,
            &[0; 4],
        );
        let mut tmp: Vec<u8> = Vec::new();
        if header.write(&mut tmp).is_ok() {
            abort_bytes.extend_from_slice(&tmp);
        }
        self.write_to_channel(&abort_bytes);
        *slot = None;
        *self.receive_file.lock().unwrap() = None;
        *self.receive_pump_tx.lock().unwrap() = None;
        self.is_active.store(false, Ordering::Relaxed);
        let fname = self.current_filename.lock().unwrap().clone();
        let _ = self.tx.send(TransferEvent::FileError {
            filename: fname,
            error: msg.to_string(),
        });
        let _ = self.tx.send(TransferEvent::TransferComplete);
    }

    /// 处理接收到的 ZMODEM 数据（由 SSH 泵等调用）；返回 `true` 表示本段数据由接收状态机消费，勿再进 VTE。
    pub fn feed_receive_data(&self, data: &[u8]) -> bool {
        !matches!(self.feed_receive(data), ReceiveFeed::Passthrough { .. })
    }

    /// `SessionComplete` 之后是否仍保留会话以吞掉 ZMODEM 收尾残尾（#458）。
    pub fn has_rx_tail_pending(&self) -> bool {
        match self.recv_side.lock() {
            Ok(g) => matches!(
                g.as_ref(),
                Some(RecvSide::Active(s)) if s.ended_at.is_some()
            ),
            Err(_) => false,
        }
    }

    /// 处理接收到的 ZMODEM 数据，并给出「本段该不该显示」的结论。
    ///
    /// MistTerm (#458)：`SessionComplete` 之后仍保留会话一小段时间，
    /// 用来吞掉收尾的 `**B08…` ZHEX 头 / `00` / XON，避免它们落进 VTE 与会话日志。
    pub fn feed_receive(&self, data: &[u8]) -> ReceiveFeed {
        if data.is_empty() {
            return ReceiveFeed::Passthrough { data: Vec::new() };
        }

        let mut g = match self.recv_side.lock() {
            Ok(x) => x,
            Err(_) => return ReceiveFeed::Passthrough { data: data.to_vec() },
        };
        let Some(side) = g.as_mut() else {
            return ReceiveFeed::Passthrough { data: data.to_vec() };
        };

        if let RecvSide::Active(session) = side {
            if let Some(ended_at) = session.ended_at {
                if ended_at.elapsed() > RX_TAIL_WINDOW {
                    // 残尾窗口已过：不再吞字节。
                    *g = None;
                    return ReceiveFeed::Passthrough { data: data.to_vec() };
                }
                let rest = self.absorb_rx_tail(session, data);
                if session.ended_at.is_none() {
                    // 提示符已恢复，会话到此结束；残尾字节已丢弃。
                    *g = None;
                }
                return match rest {
                    Some(rest) if rest.is_empty() => ReceiveFeed::Consumed,
                    Some(rest) => ReceiveFeed::Stripped { rest },
                    None => ReceiveFeed::Consumed,
                };
            }
        }

        if !self.is_active.load(Ordering::Relaxed) {
            return ReceiveFeed::Passthrough { data: data.to_vec() };
        }

        match side {
            RecvSide::Passive { pending } => {
                pending.extend_from_slice(data);
                if pending.len() > RX_INCOMING_CAP {
                    self.abort_recv_locked(&mut *g, "等待 ZRQINIT 时入站缓冲超限");
                    return ReceiveFeed::Consumed;
                }
                if !parse_zrqinit_packet(pending) {
                    return ReceiveFeed::Consumed;
                }
                let receiver = match zmodem2::Receiver::new() {
                    Ok(r) => r,
                    Err(e) => {
                        self.abort_recv_locked(&mut *g, &format!("Receiver::new: {}", e));
                        return ReceiveFeed::Consumed;
                    }
                };
                let incoming = std::mem::take(pending);
                *side = RecvSide::Active(ZmodemRxSession::new(receiver, incoming));
            }
            RecvSide::Active(session) => {
                session.incoming.extend_from_slice(data);
                if session.incoming.len() > RX_INCOMING_CAP {
                    self.abort_recv_locked(&mut *g, "ZMODEM 入站缓冲超限");
                    return ReceiveFeed::Consumed;
                }
            }
        }

        let RecvSide::Active(session) = g.as_mut().unwrap() else {
            return ReceiveFeed::Consumed;
        };

        match self.drive_rx_session(session) {
            Ok(true) => {
                // 会话已 `SessionComplete`：保留 `recv_side` 吸收残尾（见 `absorb_rx_tail`）。
            }
            Ok(false) => {}
            Err(e) => {
                self.abort_recv_locked(&mut *g, &e);
            }
        }
        ReceiveFeed::Consumed
    }

    /// 发送文件（sz）。**不得**再持有 `ssh2::Channel`：所有 ZMODEM 字节经 `pump_tx` 进入 shell 泵队列，与键盘输入 FIFO 串行写出。
    pub fn start_send(&self, file_path: &str, pump_tx: ShellPumpTx) -> Result<(), String> {
        let path = PathBuf::from(file_path);
        if !path.exists() {
            return Err(format!("File does not exist: {}", file_path));
        }

        if self.is_active.load(Ordering::Relaxed) {
            return Err("Transfer already in progress".to_string());
        }

        let file_path_clone = path.clone();
        let file_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let file_size = fs::metadata(&path)
            .map(|m| m.len())
            .unwrap_or(0);

        self.is_active.store(true, Ordering::Relaxed);
        self.received_bytes.store(0, Ordering::Relaxed);
        // 须在 spawn 之前打开；**不清空** upload_pty_rx：检测 rz 后已旁路攒入的 ZRQINIT 须留给上传线程。
        self.upload_pty_capture_on.store(true, Ordering::Relaxed);

        let tx = self.tx.clone();
        let is_active = self.is_active.clone();
        let received_bytes = self.received_bytes.clone();
        let total_bytes = self.total_bytes.clone();
        let current_filename = self.current_filename.clone();
        let upload_pty_rx = self.upload_pty_rx.clone();
        let upload_pty_capture_on = self.upload_pty_capture_on.clone();
        let upload_pty_feed_bytes = self.upload_pty_feed_bytes.clone();
        let upload_pty_pull_bytes = self.upload_pty_pull_bytes.clone();

        thread::spawn(move || {
            let finish = |is_active: &AtomicBool,
                          cap_on: &AtomicBool,
                          pty_rx: &Arc<Mutex<Vec<u8>>>,
                          tx: &Sender<TransferEvent>| {
                cap_on.store(false, Ordering::Relaxed);
                pty_rx.lock().unwrap().clear();
                is_active.store(false, Ordering::Relaxed);
                let _ = tx.send(TransferEvent::TransferComplete);
            };
            current_filename.lock().unwrap().clone_from(&file_name);
            
            let _ = tx.send(TransferEvent::FileStart {
                filename: file_name.clone(),
                size: file_size,
                outgoing: true,
            });
            
            total_bytes.store(file_size, Ordering::Relaxed);

            let use_external_sz = std::env::var("MISTTERM_ZMODEM_USE_EXTERNAL_SZ")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

            let upload_result = if use_external_sz {
                crate::ssh::lrzsz_external_sz::run_upload_external_sz(
                    file_path_clone.as_path(),
                    &file_name,
                    file_size,
                    &pump_tx,
                    &upload_pty_rx,
                    &is_active,
                    &received_bytes,
                    &tx,
                    &upload_pty_pull_bytes,
                )
            } else {
                let file_data = match fs::read(&file_path_clone) {
                    Ok(data) => data,
                    Err(e) => {
                        let _ = tx.send(TransferEvent::FileError {
                            filename: file_name.clone(),
                            error: format!("Failed to read file: {}", e),
                        });
                        finish(
                            &is_active,
                            &upload_pty_capture_on,
                            &upload_pty_rx,
                            &tx,
                        );
                        return;
                    }
                };
                crate::ssh::lrzsz_zmodem2_send::run_upload_zmodem2(
                    &file_data,
                    &file_name,
                    file_path_clone.clone(),
                    file_size,
                    &pump_tx,
                    &upload_pty_rx,
                    &is_active,
                    &received_bytes,
                    &tx,
                    &upload_pty_feed_bytes,
                    &upload_pty_pull_bytes,
                )
            };
            if let Err(e) = upload_result {
                let _ = tx.send(TransferEvent::FileError {
                    filename: file_name.clone(),
                    error: e,
                });
                finish(
                    &is_active,
                    &upload_pty_capture_on,
                    &upload_pty_rx,
                    &tx,
                );
                return;
            }

            log::info!("ZMODEM upload finished {}", file_name);

            finish(
                &is_active,
                &upload_pty_capture_on,
                &upload_pty_rx,
                &tx,
            );
        });
        
        Ok(())
    }
}

/// lrzsz / conhost 常把 ZHEX 头落成 ASCII `**B00` / `**B01`，中间无 ZDLE。
fn ascii_hex_scan_for_type(data: &[u8], want: u8) -> bool {
    for i in 0..data.len().saturating_sub(5) {
        if data[i] != b'*' || data.get(i + 1) != Some(&b'*') {
            continue;
        }
        let hb = data.get(i + 2).copied();
        if hb != Some(b'B') && hb != Some(b'b') {
            continue;
        }
        if let Some(t) = hex_pair(data[i + 3], data[i + 4]) {
            if t == want {
                return true;
            }
        }
    }
    false
}

/// Windows lrzsz `rz` 等待发送方：PTY 上出现 ASCII `**B01…`（非 ZDLE 编码）。
fn detect_windows_rz_wait_invitation(data: &[u8]) -> bool {
    ascii_hex_scan_for_type(data, 0x01)
}

/// 远端 `rz` 在 PTY 上的 ZHEX 邀请语（含 Windows ASCII `**B01…` 无 ZDLE 形式）。
pub fn detect_rz_pty_invitation(data: &[u8]) -> bool {
    parse_zrqinit_packet(data)
        || ascii_hex_scan_for_type(data, zmodem::ZRQINIT)
        || detect_windows_rz_wait_invitation(data)
}

/// 检测 ZRQINIT 包
fn parse_zrqinit_packet(data: &[u8]) -> bool {
    if hex_scan_for_type(data, zmodem::ZRQINIT) {
        log::info!("Detected ZRQINIT (HEX header)");
        return true;
    }
    if ascii_hex_scan_for_type(data, zmodem::ZRQINIT) {
        log::info!("Detected ZRQINIT (ASCII ZHEX header)");
        return true;
    }
    if binary_frame_type(data, zmodem::ZRQINIT) {
        log::info!("Detected ZRQINIT (BIN16 header)");
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_rz_command_text() {
        let lrzsz = LrzszTransfer::new("/tmp");
        
        // 应该检测到的情况
        assert!(lrzsz.detect_rz_command(b"rz rz rz"));
        assert!(lrzsz.detect_rz_command(b"Awaiting rz"));
        assert!(lrzsz.detect_rz_command(b"rz waiting to receive"));
        
        // 不应该检测到的情况
        assert!(!lrzsz.detect_rz_command(b"ls -la"));
        assert!(!lrzsz.detect_rz_command(b"cd /tmp"));
        assert!(!lrzsz.detect_rz_command(b"rz is not a command"));
    }

    #[test]
    fn test_detect_rz_command_binary() {
        let lrzsz = LrzszTransfer::new("/tmp");
        // BIN16：`** ZDLE 'A' TYPE`
        assert!(!lrzsz.detect_rz_command(&[
            zmodem::ZPAD,
            zmodem::ZPAD,
            zmodem::ZDLE,
            zmodem::ZBIN16,
            zmodem::ZRQINIT,
        ]));
        assert!(!lrzsz.detect_rz_command(&[
            zmodem::ZPAD,
            zmodem::ZPAD,
            zmodem::ZDLE,
            zmodem::ZBIN16,
            0x01,
        ]));
        // HEX：`** ZDLE 'B' 00` = ZRQINIT
        assert!(!lrzsz.detect_rz_command(&[
            zmodem::ZPAD,
            zmodem::ZPAD,
            zmodem::ZDLE,
            zmodem::ZHEX,
            b'0',
            b'0',
        ]));
        assert!(lrzsz.detect_zmodem_download(&[
            zmodem::ZPAD,
            zmodem::ZPAD,
            zmodem::ZDLE,
            zmodem::ZHEX,
            b'0',
            b'0',
        ]));
        assert!(!lrzsz.detect_rz_command(&[zmodem::ZPAD, zmodem::ZPAD, 0x00, 0x00]));
    }

    #[test]
    fn test_parse_zrqinit_packet_accepts_star_and_0x80_zpad() {
        assert!(parse_zrqinit_packet(&[
            b'*',
            b'*',
            zmodem::ZDLE,
            zmodem::ZBIN16,
            zmodem::ZRQINIT,
        ]));
        assert!(parse_zrqinit_packet(b"**\x18B00000000000000\r\n\x11"));
        assert!(parse_zrqinit_packet(&[
            0x80,
            0x80,
            zmodem::ZDLE,
            zmodem::ZBIN16,
            zmodem::ZRQINIT,
        ]));
    }

    #[test]
    fn test_detect_windows_rz_wait_invitation() {
        assert!(detect_windows_rz_wait_invitation(b"**B0100000063f694"));
        assert!(!detect_windows_rz_wait_invitation(b"**\x18B00000000000000\r\n\x11"));
        assert!(!detect_windows_rz_wait_invitation(b"echo hello"));
    }

    #[test]
    fn test_detect_rz_pty_invitation_windows_ascii() {
        assert!(detect_rz_pty_invitation(b"**\x18B00000000000000\r\n\x11"));
        assert!(detect_rz_pty_invitation(b"**B0100000063f694"));
        assert!(!detect_rz_pty_invitation(b"echo hello"));
    }

    /// 议题里日志残留的 ZFIN 风格 ZHEX 尾巴（无 CRC 后的 CR/LF 也可能单独到达）。
    const ZFIN_ZHEX_TAIL: &'static [u8] = b"**B0800000000022d";

    fn transfer_after_session_complete() -> LrzszTransfer {
        let lrzsz = LrzszTransfer::new("/tmp");
        let receiver = zmodem2::Receiver::new().expect("zmodem2 receiver");
        let mut session = ZmodemRxSession::new(receiver, Vec::new());
        session.ended_at = Some(Instant::now());
        *lrzsz.recv_side.lock().unwrap() = Some(RecvSide::Active(session));
        lrzsz.is_active.store(false, Ordering::Relaxed);
        lrzsz
    }

    fn assert_display_rest_is_clean_prompt(rest: &[u8], prompt: &[u8]) {
        assert_eq!(rest, prompt, "display/log path must be the restored prompt only");
        assert!(!rest.starts_with(b"00"), "literal 00 must not prefix the prompt");
        assert!(
            !rest.windows(3).any(|w| w == b"**B"),
            "ZHEX tail must not remain for VTE/session log"
        );
    }

    #[test]
    fn test_rx_residue_prefix_absorbs_leftover_zhex_zfin_tail() {
        assert_eq!(
            rx_residue_prefix_len(ZFIN_ZHEX_TAIL, RX_TAIL_RESIDUE_CAP),
            ZFIN_ZHEX_TAIL.len()
        );
        let with_term = b"**B0800000000022d\r\n\x11";
        assert_eq!(
            rx_residue_prefix_len(with_term, RX_TAIL_RESIDUE_CAP),
            with_term.len()
        );
    }

    #[test]
    fn test_rx_residue_prefix_absorbs_trailing_00_and_keeps_prompt() {
        assert_eq!(rx_residue_prefix_len(b"00", RX_TAIL_RESIDUE_CAP), 2);

        let glued = b"00mist410@host:~$ ";
        let eaten = rx_residue_prefix_len(glued, RX_TAIL_RESIDUE_CAP);
        assert_display_rest_is_clean_prompt(&glued[eaten..], b"mist410@host:~$ ");

        let with_zhex = b"**B0800000000022d\r\n\x1100mist410@host:~$ ";
        let eaten = rx_residue_prefix_len(with_zhex, RX_TAIL_RESIDUE_CAP);
        assert_display_rest_is_clean_prompt(&with_zhex[eaten..], b"mist410@host:~$ ");
    }

    #[test]
    fn test_feed_receive_after_session_complete_does_not_emit_zhex_or_00() {
        let lrzsz = transfer_after_session_complete();
        assert!(lrzsz.has_rx_tail_pending());
        assert!(!lrzsz.is_active());

        // 单独到达的 ZHEX 尾巴必须整段吞噬，不得进显示/日志路径。
        assert_eq!(lrzsz.feed_receive(ZFIN_ZHEX_TAIL), ReceiveFeed::Consumed);
        assert!(lrzsz.feed_receive_data(ZFIN_ZHEX_TAIL));
        assert!(lrzsz.has_rx_tail_pending());
        assert!(!lrzsz.is_active());

        match lrzsz.feed_receive(b"00mist410@host:~$ ") {
            ReceiveFeed::Stripped { rest } => {
                assert_display_rest_is_clean_prompt(&rest, b"mist410@host:~$ ");
            }
            other => panic!("expected Stripped prompt, got {other:?}"),
        }
        // 提示符已恢复：残尾窗口结束，成功接收保持完成状态。
        assert!(!lrzsz.has_rx_tail_pending());
        assert!(!lrzsz.is_active());
        assert!(
            lrzsz
                .try_recv_event()
                .is_none_or(|ev| !matches!(ev, TransferEvent::FileError { .. })),
            "absorbing leftover ZMODEM bytes must not fail the completed receive"
        );
    }

    #[test]
    fn test_feed_receive_after_session_complete_strips_combined_tail_and_prompt() {
        let lrzsz = transfer_after_session_complete();
        let data = b"**B0800000000022d\r\n\x1100mist410@host:~$ ";
        match lrzsz.feed_receive(data) {
            ReceiveFeed::Stripped { rest } => {
                assert_display_rest_is_clean_prompt(&rest, b"mist410@host:~$ ");
            }
            other => panic!("expected Stripped prompt, got {other:?}"),
        }
        assert!(!lrzsz.has_rx_tail_pending());
        assert!(!lrzsz.is_active());
    }
}
