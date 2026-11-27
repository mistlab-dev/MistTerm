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
use super::manager::{enqueue_zmodem_write, ShellPumpTx, SshSessionHandle};

const RX_INCOMING_CAP: usize = 512 * 1024;

/// 会话结束后继续吞掉的协议残尾上限（`**B08…` / 收尾 XON 等）。超过即视为远端又开始说话。
const RX_TAIL_RESIDUE_CAP: usize = 4 * 1024;

/// 残尾吞噬窗口：`SessionComplete` 后这段时间内到达的协议字节都视为 ZMODEM 收尾。
const RX_TAIL_WINDOW: Duration = Duration::from_millis(1500);

/// 单次 `sz`→本机会话：`zmodem2::Receiver` + 未解析完的 PTY 字节
struct ZmodemRxSession {
    receiver: zmodem2::Receiver,
    incoming: Vec<u8>,
    last_save_path: Option<PathBuf>,
    /// MistTerm (#458): `SessionComplete` 之后继续吃掉的尾字节。
    ///
    /// `zmodem2::Receiver` 在 `ZFIN` 上置 `SessionEnd` 并报 `SessionComplete`，
    /// 但远端 `sz` 的 ZFIN 应答 / 收尾 `XON` 常与**后续 PTY 分片**一起到达。
    /// 若此时立刻清空 `recv_side`，这批残余就落进 VTE 与会话日志（典型症状：
    /// 提示符前多出字面 `00`、日志里留下未消费的 `**B08…`）。
    residue: Vec<u8>,
    /// 接收结束时刻（`SessionComplete`）；作为残尾窗口的起点。
    ended_at: Option<Instant>,
    /// 窗口内单独到达、证据还不够的字面 `00`。先按住：既不进 VTE，也不算已确认残尾。
    /// 后续分片若补齐 bracketed-paste / 提示符则丢弃；若是着色或普通正文则放回显示路径。
    uncertain_hold: Vec<u8>,
}

impl ZmodemRxSession {
    fn new(receiver: zmodem2::Receiver, incoming: Vec<u8>, last_save_path: Option<PathBuf>) -> Self {
        Self {
            receiver,
            incoming,
            last_save_path,
            residue: Vec::new(),
            ended_at: None,
            uncertain_hold: Vec::new(),
        }
    }
}

/// 被动接收：先攒字节直到出现 ZRQINIT，再建 `zmodem2::Receiver`（其 `new` 会排队 ZRINIT）
enum RecvSide {
    Passive { pending: Vec<u8> },
    Active(ZmodemRxSession),
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
    /// HEX 帧编码符 `B`（接收端 rz 在 PTY 上几乎总是发 HEX 头）
    pub const ZHEX: u8 = 0x42;
    /// BIN16 帧编码符 `A`
    pub const ZBIN16: u8 = 0x41;
    pub const ZBIN32: u8 = 0x43;

    pub const ZRQINIT: u8 = 0x00;
    /// 部分实现用 `ZVBIN 'a'` 代替 `ZBIN 'A'` 发 BIN16 头
    pub const ZVBIN16: u8 = b'a';
    /// 收尾 XON（`^Q`）：`sz` 结束后会发两个，常与残尾同行到达。
    pub const XON: u8 = 0x11;
}

fn is_zpad(b: u8) -> bool {
    b == zmodem::ZPAD || b == 0x80
}

// ---------------------------------------------------------------- #458 残尾吸收

#[inline]
fn is_hex_digit(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

/// 行填充：只有在本段已经出现充分协议证据时，才随残尾行一起吞掉。
/// 单独的 CR/LF 必须留给普通输出（#458 安全边界）。
fn rx_line_fill_byte(b: u8) -> bool {
    matches!(b, b'\r' | b'\n' | b'\t' | b' ')
}

/// 只出现在 ZMODEM 帧里的填充 / 控制，本身即可作为协议证据。
fn rx_protocol_pad_byte(b: u8) -> bool {
    matches!(b, zmodem::ZPAD | 0x80 | zmodem::ZDLE | zmodem::XON)
}

/// 只可能出现在协议流里的 C0 控制符（不含 `\r` `\n` `\t`，也不含 XON，也不含 ESC）。
///
/// 0x1B（ESC）必须保留：`SessionComplete` 之后，迟到的 `00` 常与 bash 重新打开
/// bracketed-paste 的 CSI（`ESC[?2004h`）以及提示符同包或紧挨着到达。
/// 若把 ESC 当协议字节丢掉，VTE 会显示出字面 `[?2004h`。
fn rx_drop_byte(b: u8) -> bool {
    matches!(b, 0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x10 | 0x12..=0x17 | 0x19 | 0x1a | 0x1c..=0x1f)
}

/// 提示符片段：与 `terminal.rs` 的 `contains_shell_prompt_fragment` 同口径。
fn prompt_fragment_pos(text: &[u8]) -> Option<usize> {
    text.windows(2)
        .position(|w| matches!(w, [b'$', b' '] | [b'#', b' '] | [b'>', b' ']))
}

/// ZFIN（类型 `0x08`）的 ZHEX 头正文：`B08` + 标志/CRC 十六进制。
/// 点查失败形态是没有 `**` 的 `00B0800000000022d`（前导 `00` + `B08…`）。
fn looks_like_zfin_zhex_hex(hex: &[u8]) -> bool {
    hex.windows(3).any(|w| w.eq_ignore_ascii_case(b"B08"))
}

/// 点查与议题里的字面残尾就是两个 ASCII `0`，不是任意 hex 正文。
fn is_exact_leftover_zeros(hex: &[u8]) -> bool {
    hex == b"00"
}

/// 完整的 bracketed-paste CSI（bash 在命令结束后常重开）。不含着色 CSI / OSC。
fn starts_with_bracketed_paste_csi(data: &[u8]) -> bool {
    data.starts_with(b"\x1b[?2004h") || data.starts_with(b"\x1b[?2004l")
}

/// 同包里足以把前面的 `00` 判成残尾、而不是「00 + 着色/正文」的证据。
/// 不把不完整的 `ESC[` 当证据：那也可能是 `\x1b[32m` 的开头。
fn rest_has_shell_restore_evidence(rest: &[u8]) -> bool {
    starts_with_bracketed_paste_csi(rest) || prompt_fragment_pos(rest).is_some()
}

/// 已确认吞下的残尾里是否已有充分协议证据（`**` / ZDLE / ZFIN ZHEX）。
/// 仅按住的模糊 `00` 不算。
fn residue_has_protocol_evidence(residue: &[u8]) -> bool {
    residue
        .iter()
        .any(|&b| matches!(b, zmodem::ZPAD | 0x80 | zmodem::ZDLE))
        || looks_like_zfin_zhex_hex(residue)
}

enum ResiduePrefix {
    /// 有充分协议或同包 shell 恢复证据，这些前缀不得进 VTE。
    Commit(usize),
    /// 窗口内单独到达的 `00`：先按住，等下一分片再决定。
    HoldZeros(usize),
    /// 不是残尾；整段按普通输出处理（含 CR/LF、着色 CSI、OSC、hex 正文）。
    None,
}

#[cfg(test)]
fn rx_residue_prefix_len(data: &[u8], budget: usize) -> usize {
    rx_residue_prefix_len_at(data, budget, false, true)
}

#[cfg(test)]
fn rx_residue_prefix_len_at(
    data: &[u8],
    budget: usize,
    prior_evidence: bool,
    within_window: bool,
) -> usize {
    match classify_rx_residue_prefix(data, budget, prior_evidence, within_window) {
        ResiduePrefix::Commit(n) | ResiduePrefix::HoldZeros(n) => n,
        ResiduePrefix::None => 0,
    }
}

/// 判定 `data` 开头属于 ZMODEM 收尾残尾的字节数（#458）。
///
/// `SessionComplete` 之后，远端 `sz` 的收尾 `**B08…` ZHEX 头 / XON / `\r\n`
/// 常与下一段 PTY 分片一起到达。这些字节既不是文件载荷，也不是终端该显示的内容。
/// 只看**开头**：一旦遇到可见文本（如 `user@host:~$`），立刻停下。
fn classify_rx_residue_prefix(
    data: &[u8],
    budget: usize,
    prior_evidence: bool,
    within_window: bool,
) -> ResiduePrefix {
    if data.is_empty() || budget == 0 {
        return ResiduePrefix::None;
    }
    let cap = data.len().min(budget);
    let mut i = 0usize;
    while i < cap && rx_line_fill_byte(data[i]) {
        i += 1;
    }
    let fill = i;
    if i >= cap {
        // 只有 CR/LF/空白：保留给普通输出。
        return ResiduePrefix::None;
    }

    let mut in_protocol = prior_evidence;
    let mut saw_protocol = prior_evidence;

    while i < cap {
        let b = data[i];
        if rx_drop_byte(b) {
            if !(within_window || in_protocol) {
                break;
            }
            i += 1;
            in_protocol = true;
            saw_protocol = true;
            continue;
        }
        if rx_protocol_pad_byte(b) {
            i += 1;
            in_protocol = true;
            saw_protocol = true;
            continue;
        }
        if rx_line_fill_byte(b) {
            if in_protocol {
                i += 1;
                continue;
            }
            break;
        }
        if is_hex_digit(b) {
            let start = i;
            while i < cap && is_hex_digit(data[i]) {
                i += 1;
            }
            let hex = &data[start..i];
            let zfin_hex = looks_like_zfin_zhex_hex(hex);
            let rest = &data[i..];
            if zfin_hex {
                while i < cap && (rx_line_fill_byte(data[i]) || data[i] == zmodem::XON) {
                    i += 1;
                }
                in_protocol = true;
                saw_protocol = true;
                continue;
            }
            if in_protocol {
                // 已有 `**` / ZDLE 等证据后的截断 hex 续段（例如先到 `**B` 再到 `0800…`）。
                while i < cap && (rx_line_fill_byte(data[i]) || data[i] == zmodem::XON) {
                    i += 1;
                }
                saw_protocol = true;
                continue;
            }
            if is_exact_leftover_zeros(hex) && rest_has_shell_restore_evidence(rest) {
                // 只吞 `00`，CSI/OSC/提示符留给 VTE。前导 CR/LF 若紧贴残尾则随行一起剥掉。
                return ResiduePrefix::Commit(i);
            }
            if is_exact_leftover_zeros(hex)
                && rest.is_empty()
                && within_window
                && fill == 0
                && start == 0
            {
                return ResiduePrefix::HoldZeros(2);
            }
            // `00\r\n`、`00` + 着色 CSI / OSC / 其它 hex 正文：一律不吞。
            return ResiduePrefix::None;
        }
        // ESC / 可见文本：交给显示路径。
        break;
    }

    if saw_protocol && i > fill {
        return ResiduePrefix::Commit(i);
    }
    ResiduePrefix::None
}

/// `feed_receive` 的消费结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiveFeed {
    /// 整段由 ZMODEM 接收状态机 / 残尾吞噬消费，**不得**进 VTE 与会话日志。
    Consumed,
    /// 与本接收无关，按普通 PTY 输出处理。
    Passthrough { data: Vec<u8> },
    /// 头部协议残尾已剥离，剩余字节按普通 PTY 输出处理。
    Stripped { rest: Vec<u8> },
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
    receive_pump: Arc<Mutex<Option<(ShellPumpTx, Arc<polling::Poller>)>>>,
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
            receive_pump: Arc::new(Mutex::new(None)),
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
        *self.receive_pump.lock().unwrap() = None;
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
        *self.receive_pump.lock().unwrap() = None;
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
        *self.receive_pump.lock().unwrap() = None;

        let receiver = zmodem2::Receiver::new().map_err(|e| format!("ZMODEM Receiver::new: {}", e))?;
        let mut session = ZmodemRxSession::new(receiver, Vec::new(), None);
        self.flush_receiver_outgoing(&mut session.receiver)?;
        *self.recv_side.lock().unwrap() = Some(RecvSide::Active(session));
        log::info!("ZMODEM receive started (active, ZRINIT sent)");
        Ok(())
    }

    /// 通过 shell pump 写回接收端 ACK（交互式 shell 使用同一 PTY FIFO）。
    ///
    /// `pump_tx` 与 `pump_poller` 必须一起登记，避免写出应答时有队列却唤不醒泵。
    pub fn start_receive_pump(
        &self,
        pump_tx: ShellPumpTx,
        pump_poller: Arc<polling::Poller>,
    ) -> Result<(), String> {
        if self.is_active.load(Ordering::Relaxed) {
            return Err("Transfer already in progress".to_string());
        }

        self.is_active.store(true, Ordering::Relaxed);
        self.received_bytes.store(0, Ordering::Relaxed);
        self.total_bytes.store(0, Ordering::Relaxed);
        *self.channel.lock().unwrap() = None;
        *self.receive_pump.lock().unwrap() = Some((pump_tx, pump_poller));
        *self.recv_side.lock().unwrap() = Some(RecvSide::Passive {
            pending: Vec::new(),
        });
        log::info!("ZMODEM receive started via shell pump (passive, waiting for ZRQINIT)");
        Ok(())
    }

    /// 向 SSH 通道写 ZMODEM 出站字节（`zmodem2::Receiver::drain_outgoing`）
    ///
    /// 交互式 shell 路径必须走 `enqueue_zmodem_write`（阻塞 send + notify）：
    /// 泵把远端 ZRQINIT 交给界面后会回到 `poll.wait`，界面再排队 ZRINIT/ZACK
    /// 时若不唤醒，对端就一直等应答（#442，与 #494 上传同源）。
    fn write_to_channel(&self, data: &[u8]) {
        if let Ok(pump_guard) = self.receive_pump.lock() {
            if let Some((ref pump_tx, ref pump_poller)) = *pump_guard {
                match enqueue_zmodem_write(pump_tx, pump_poller, data.to_vec()) {
                    Ok(()) => {
                        log::debug!("ZMODEM queued {} bytes via shell pump", data.len());
                    }
                    Err(e) => {
                        log::warn!("ZMODEM enqueue via shell pump failed: {e}");
                    }
                }
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
    fn poll_receiver_events(&self, session: &mut ZmodemRxSession) -> Result<bool, String> {
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
                    // 会话保留到残尾窗口结束，由 `feed_receive` 的残尾分支吃掉收尾字节。
                    *self.receive_file.lock().unwrap() = None;
                    *self.receive_pump.lock().unwrap() = None;
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
        *self.receive_pump.lock().unwrap() = None;
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

        // 残尾分支：先把结论算出来（只借用会话），再决定是否结束 `recv_side`。
        let mut tail_result: Option<(ReceiveFeed, bool)> = None;
        if let RecvSide::Active(session) = side {
            if session.ended_at.is_some() {
                // `drive_rx_session` 在 ZFIN 处停下时，同包里尚未解析的尾部还留在
                // `session.incoming`（可能正是 `**B08…` / `00` 与随后的提示符）。
                // 首次进入残尾分支时把它并进来一起判定，避免提示符被吞掉。
                let mut combined = std::mem::take(&mut session.incoming);
                combined.extend_from_slice(data);
                let within_window = session
                    .ended_at
                    .is_some_and(|t| t.elapsed() <= RX_TAIL_WINDOW);
                let budget = RX_TAIL_RESIDUE_CAP.saturating_sub(session.residue.len());
                let prior = residue_has_protocol_evidence(&session.residue);
                tail_result = Some(match classify_rx_residue_prefix(
                    &combined,
                    budget,
                    prior,
                    within_window,
                ) {
                    ResiduePrefix::Commit(eaten) => {
                        session.residue.extend_from_slice(&combined[..eaten]);
                        session.uncertain_hold.clear();
                        log::debug!(
                            "ZMODEM receive: 吞掉收尾残尾 {} B（累计 {} B）",
                            eaten,
                            session.residue.len()
                        );
                        let rest = combined[eaten..].to_vec();
                        if rest.is_empty() {
                            // 纯残尾分片：继续保持窗口，等下一个可能迟到的分片。
                            (ReceiveFeed::Consumed, false)
                        } else if prompt_fragment_pos(&rest).is_some() {
                            // 提示符已恢复：残尾窗口到此刻结束，成功接收保持完成状态。
                            (ReceiveFeed::Stripped { rest }, true)
                        } else {
                            (ReceiveFeed::Stripped { rest }, false)
                        }
                    }
                    ResiduePrefix::HoldZeros(n) => {
                        session.uncertain_hold.extend_from_slice(&combined[..n]);
                        // 残尾窗口内单独到达的 `00` 先按住：既不进 VTE，也不算已确认残尾。
                        (ReceiveFeed::Consumed, false)
                    }
                    ResiduePrefix::None => {
                        let restore = rest_has_shell_restore_evidence(&combined);
                        let had_held = !session.uncertain_hold.is_empty();
                        let mut out = std::mem::take(&mut session.uncertain_hold);
                        if restore {
                            // 下一分片是 2004 / 提示符：先前按住的 `00` 是残尾，丢弃。
                            out.clear();
                        }
                        // 下一分片是着色或普通正文：把按住的 `00` 放回显示，不得误吞。
                        out.extend_from_slice(&combined);
                        // 普通输出已开始（或提示符已恢复）：停止再扫残尾。
                        let clear = !restore || prompt_fragment_pos(&combined).is_some();
                        if had_held && restore {
                            (ReceiveFeed::Stripped { rest: out }, clear)
                        } else {
                            (ReceiveFeed::Passthrough { data: out }, clear)
                        }
                    }
                });
            }
        }
        if let Some((feed, clear)) = tail_result {
            if clear {
                *g = None;
            }
            return feed;
        }

        if !self.is_active.load(Ordering::Relaxed) {
            return ReceiveFeed::Passthrough { data: data.to_vec() };
        }

        log::debug!(
            "feed_receive len={} active={}",
            data.len(),
            self.is_active.load(Ordering::Relaxed)
        );

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
                *side = RecvSide::Active(ZmodemRxSession::new(receiver, incoming, None));
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
                // 会话已 `SessionComplete`：保留 `recv_side` 吸收残尾（见 `feed_receive`）。
            }
            Ok(false) => {}
            Err(e) => {
                self.abort_recv_locked(&mut *g, &e);
            }
        }
        ReceiveFeed::Consumed
    }

    /// 发送文件（sz）。**不得**再持有 `ssh2::Channel`：所有 ZMODEM 字节经 `pump_tx` 进入 shell 泵队列，与键盘输入 FIFO 串行写出。
    pub fn start_send(&self, file_path: &str, pump_tx: ShellPumpTx, pump_poller: Arc<polling::Poller>) -> Result<(), String> {
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
                    &pump_poller,
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
                    &pump_poller,
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

    // -------------------------------------------- #458 SessionComplete 之后残尾吸收

    /// 议题里日志残留的 ZFIN 风格 ZHEX 尾巴（无 CRC 后的 CR/LF 也可能单独到达）。
    const ZFIN_ZHEX_TAIL: &[u8] = b"**B0800000000022d";
    /// 点查实际漏到 VTE 的残尾行（无 `**`，前导 `00` + `B08…`）。
    const GUI_LEFTOVER_00B08: &[u8] = b"00B0800000000022d";
    const CSI_BRACKETED_PASTE_ON: &[u8] = b"\x1b[?2004h";
    const OSC_TITLE: &[u8] = b"\x1b]0;title\x07";
    const PROMPT: &[u8] = b"user@host:~$ ";
    const COLOR_GREEN: &[u8] = b"\x1b[32m";
    const COLOR_RESET: &[u8] = b"\x1b[0m";

    fn color_line() -> Vec<u8> {
        let mut v = b"00".to_vec();
        v.extend_from_slice(COLOR_GREEN);
        v.extend_from_slice(b"red");
        v.extend_from_slice(COLOR_RESET);
        v.extend_from_slice(b"\r\n");
        v
    }

    fn prompt_with_csi() -> Vec<u8> {
        let mut v = CSI_BRACKETED_PASTE_ON.to_vec();
        v.extend_from_slice(PROMPT);
        v
    }

    fn transfer_after_session_complete() -> LrzszTransfer {
        transfer_after_session_complete_ended_ago(Duration::from_millis(0))
    }

    fn transfer_after_session_complete_ended_ago(ago: Duration) -> LrzszTransfer {
        let lrzsz = LrzszTransfer::new("/tmp");
        let receiver = zmodem2::Receiver::new().expect("zmodem2 receiver");
        let mut session = ZmodemRxSession::new(receiver, Vec::new(), None);
        session.ended_at = Some(Instant::now().checked_sub(ago).expect("ended_at in the past"));
        *lrzsz.recv_side.lock().unwrap() = Some(RecvSide::Active(session));
        lrzsz.is_active.store(false, Ordering::Relaxed);
        lrzsz
    }

    fn assert_clean_prompt(rest: &[u8]) {
        assert_eq!(rest, PROMPT, "display path must be the restored prompt only");
        assert!(!rest.starts_with(b"00"), "literal 00 must not prefix the prompt");
        assert!(
            !rest.windows(3).any(|w| w == b"**B" || w.eq_ignore_ascii_case(b"B08")),
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

        let glued = b"00user@host:~$ ";
        let eaten = rx_residue_prefix_len(glued, RX_TAIL_RESIDUE_CAP);
        assert_clean_prompt(&glued[eaten..]);

        let with_zhex = b"**B0800000000022d\r\n\x1100user@host:~$ ";
        let eaten = rx_residue_prefix_len(with_zhex, RX_TAIL_RESIDUE_CAP);
        assert_clean_prompt(&with_zhex[eaten..]);
    }

    #[test]
    fn test_rx_residue_prefix_absorbs_gui_leftover_00b08_without_stars() {
        assert_eq!(
            rx_residue_prefix_len(GUI_LEFTOVER_00B08, RX_TAIL_RESIDUE_CAP),
            GUI_LEFTOVER_00B08.len()
        );
        for tail in [
            b"00B0800000000022d\r\n".as_slice(),
            b"00B0800000000022d\n".as_slice(),
            b"00B0800000000022d\r\n\x11".as_slice(),
            b"\r\n00B0800000000022d\r\n".as_slice(),
            b"B0800000000022d\r\n".as_slice(),
        ] {
            assert_eq!(
                rx_residue_prefix_len(tail, RX_TAIL_RESIDUE_CAP),
                tail.len(),
                "must swallow {tail:?} and not leave it for VTE"
            );
        }
        // 不要误吞普通数字行。
        assert_eq!(rx_residue_prefix_len(b"1234\r\n", RX_TAIL_RESIDUE_CAP), 0);
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

        match lrzsz.feed_receive(b"00user@host:~$ ") {
            ReceiveFeed::Stripped { rest } => assert_clean_prompt(&rest),
            other => panic!("expected Stripped prompt, got {other:?}"),
        }
        // 提示符已恢复：残尾窗口结束，成功接收保持完成状态。
        assert!(!lrzsz.has_rx_tail_pending());
        assert!(!lrzsz.is_active());
        assert!(
            lrzsz
                .try_recv_event()
                .map(|ev| !matches!(ev, TransferEvent::FileError { .. }))
                .unwrap_or(true),
            "absorbing leftover ZMODEM bytes must not fail the completed receive"
        );
    }

    #[test]
    fn test_feed_receive_after_session_complete_strips_combined_tail_and_prompt() {
        let lrzsz = transfer_after_session_complete();
        let data = b"**B0800000000022d\r\n\x1100user@host:~$ ";
        match lrzsz.feed_receive(data) {
            ReceiveFeed::Stripped { rest } => assert_clean_prompt(&rest),
            other => panic!("expected Stripped prompt, got {other:?}"),
        }
        assert!(!lrzsz.has_rx_tail_pending());
        assert!(!lrzsz.is_active());
    }

    #[test]
    fn test_feed_receive_after_session_complete_does_not_emit_gui_leftover_00b08() {
        let lrzsz = transfer_after_session_complete();
        assert_eq!(lrzsz.feed_receive(GUI_LEFTOVER_00B08), ReceiveFeed::Consumed);
        assert_eq!(
            lrzsz.feed_receive(b"00B0800000000022d\r\n"),
            ReceiveFeed::Consumed
        );
        assert!(lrzsz.has_rx_tail_pending());
        assert!(!lrzsz.is_active());

        match lrzsz.feed_receive(PROMPT) {
            ReceiveFeed::Stripped { rest } | ReceiveFeed::Passthrough { data: rest } => {
                assert_clean_prompt(&rest);
            }
            other => panic!("expected clean prompt, got {other:?}"),
        }
        assert!(!lrzsz.has_rx_tail_pending());
        assert!(!lrzsz.is_active());
    }

    #[test]
    fn test_feed_receive_absorbs_delayed_post_complete_00b08_chunk() {
        // 点查里成功后先空等，残尾在软窗口（1.5s）之后才作为独立分片到达。
        let lrzsz = transfer_after_session_complete_ended_ago(
            RX_TAIL_WINDOW + Duration::from_millis(500),
        );
        assert!(lrzsz.has_rx_tail_pending());
        assert_eq!(
            lrzsz.feed_receive(b"00B0800000000022d\r\n"),
            ReceiveFeed::Consumed,
            "delayed 00B08… chunk after SessionComplete must not passthrough"
        );
        assert!(lrzsz.has_rx_tail_pending());

        match lrzsz.feed_receive(PROMPT) {
            ReceiveFeed::Stripped { rest } | ReceiveFeed::Passthrough { data: rest } => {
                assert_clean_prompt(&rest);
            }
            other => panic!("expected clean prompt, got {other:?}"),
        }
        assert!(!lrzsz.has_rx_tail_pending());
        assert!(!lrzsz.is_active());
    }

    /// #497 的 shell 泵唤醒必须保持：`pump_tx` 与 `pump_poller` 一起登记，
    /// 接收端出站字节仍经 `enqueue_zmodem_write`（阻塞 send + notify），
    /// 不得退回只发 `ShellPumpCommand::ZmodemWrite` 的旧写法。
    #[test]
    fn test_start_receive_pump_registers_poller_together() {
        let lrzsz = LrzszTransfer::new("/tmp");
        let poller = Arc::new(polling::Poller::new().expect("poller"));
        let (tx, rx) = std::sync::mpsc::sync_channel(8);
        lrzsz
            .start_receive_pump(tx, poller.clone())
            .expect("start_receive_pump");
        assert!(lrzsz.is_active());
        assert!(
            lrzsz.receive_pump.lock().unwrap().is_some(),
            "pump_tx 与 pump_poller 必须一起登记"
        );

        // 接收状态机的出站字节仍走 shell 泵队列（阻塞 send），不会退回直写 Channel。
        lrzsz.write_to_channel(b"ZRINIT");
        match rx.try_recv() {
            Ok(crate::ssh::ShellPumpCommand::ZmodemWrite(data)) => {
                assert_eq!(data, b"ZRINIT");
            }
            other => panic!("expected ZmodemWrite through the shell pump queue, got {other:?}"),
        }

        lrzsz.cancel_active_transfer();
        assert!(lrzsz.receive_pump.lock().unwrap().is_none());
        assert!(!lrzsz.is_active());
        assert!(lrzsz.recv_side.lock().unwrap().is_none());
    }

    #[test]
    fn test_feed_receive_absorbs_split_00_then_b08_without_stars() {
        let lrzsz = transfer_after_session_complete();
        assert_eq!(lrzsz.feed_receive(b"00"), ReceiveFeed::Consumed);
        assert_eq!(
            lrzsz.feed_receive(b"B0800000000022d\r\n"),
            ReceiveFeed::Consumed
        );
        match lrzsz.feed_receive(PROMPT) {
            ReceiveFeed::Stripped { rest } | ReceiveFeed::Passthrough { data: rest } => {
                assert_clean_prompt(&rest);
            }
            other => panic!("expected clean prompt, got {other:?}"),
        }
        assert!(!lrzsz.has_rx_tail_pending());
    }

    /// 产品点查失败形态：软窗口之后迟到的 `00` 与 `ESC[?2004h` + 提示符同包到达。
    #[test]
    fn test_rx_residue_prefix_eats_00_stuck_to_csi_but_keeps_esc() {
        let mut data = b"00".to_vec();
        data.extend_from_slice(&prompt_with_csi());
        assert_eq!(rx_residue_prefix_len(&data, RX_TAIL_RESIDUE_CAP), 2);
        assert_eq!(
            rx_residue_prefix_len(&prompt_with_csi(), RX_TAIL_RESIDUE_CAP),
            0
        );
        assert!(
            !rx_drop_byte(0x1b),
            "ESC must not be classified as a protocol drop byte"
        );
    }

    #[test]
    fn test_feed_receive_late_00_glued_to_csi_and_prompt() {
        let lrzsz = transfer_after_session_complete_ended_ago(
            RX_TAIL_WINDOW + Duration::from_millis(500),
        );
        let mut data = b"00".to_vec();
        data.extend_from_slice(&prompt_with_csi());
        match lrzsz.feed_receive(&data) {
            ReceiveFeed::Stripped { rest } => {
                assert_eq!(rest, prompt_with_csi());
                assert_eq!(rest.first().copied(), Some(0x1b));
                assert!(!rest.starts_with(b"00"));
                assert!(!rest.contains(&b'\r'), "must not inject CR");
            }
            other => panic!("expected CSI+prompt, got {other:?}"),
        }
        assert!(!lrzsz.has_rx_tail_pending());
    }

    #[test]
    fn test_feed_receive_split_00_then_csi_prompt() {
        let lrzsz = transfer_after_session_complete();
        assert_eq!(lrzsz.feed_receive(b"00"), ReceiveFeed::Consumed);
        match lrzsz.feed_receive(CSI_BRACKETED_PASTE_ON) {
            ReceiveFeed::Stripped { rest } => {
                assert_eq!(rest, CSI_BRACKETED_PASTE_ON);
                assert_eq!(rest.first().copied(), Some(0x1b));
                assert!(!rest.starts_with(b"00"));
            }
            other => panic!("expected CSI only, got {other:?}"),
        }
        assert!(lrzsz.has_rx_tail_pending());
        match lrzsz.feed_receive(PROMPT) {
            ReceiveFeed::Stripped { rest } | ReceiveFeed::Passthrough { data: rest } => {
                assert_clean_prompt(&rest);
                assert!(!rest.contains(&b'\r'));
            }
            other => panic!("expected clean prompt, got {other:?}"),
        }
        assert!(!lrzsz.has_rx_tail_pending());
    }

    #[test]
    fn test_feed_receive_00_csi_then_prompt_without_injecting_enter() {
        let lrzsz = transfer_after_session_complete();
        let mut data = b"00".to_vec();
        data.extend_from_slice(&prompt_with_csi());
        match lrzsz.feed_receive(&data) {
            ReceiveFeed::Stripped { rest } => {
                assert_eq!(rest, prompt_with_csi());
                assert!(rest.starts_with(b"\x1b[?2004h"));
                assert!(!rest.contains(&b'\r'), "must not inject CR/LF");
                assert!(!rest.starts_with(b"00"));
            }
            other => panic!("expected CSI+prompt, got {other:?}"),
        }
    }

    #[test]
    fn test_rx_residue_prefix_does_not_swallow_color_osc_or_hex_body() {
        let color = color_line();
        assert_eq!(rx_residue_prefix_len(&color, RX_TAIL_RESIDUE_CAP), 0);
        assert_eq!(
            rx_residue_prefix_len_at(&color, RX_TAIL_RESIDUE_CAP, false, false),
            0,
            "after the window, color output still must not be swallowed"
        );

        let mut osc = b"00".to_vec();
        osc.extend_from_slice(OSC_TITLE);
        assert_eq!(rx_residue_prefix_len(&osc, RX_TAIL_RESIDUE_CAP), 0);

        assert_eq!(
            rx_residue_prefix_len(b"ff\x1b[32mred\x1b[0m", RX_TAIL_RESIDUE_CAP),
            0
        );
        assert_eq!(
            rx_residue_prefix_len(b"00000000  61 62 63 64\n", RX_TAIL_RESIDUE_CAP),
            0
        );
        assert_eq!(rx_residue_prefix_len(b"00\r\n", RX_TAIL_RESIDUE_CAP), 0);
        assert_eq!(rx_residue_prefix_len(b"cafe\r\n", RX_TAIL_RESIDUE_CAP), 0);
        // 任意 hex 粘在提示符前也不够当协议证据。
        assert_eq!(
            rx_residue_prefix_len(b"cafe\x1b[?2004huser@host:~$ ", RX_TAIL_RESIDUE_CAP),
            0
        );
        // 单独 CR/LF、以及 CR/LF + 提示符，必须保留行尾。
        assert_eq!(rx_residue_prefix_len(b"\r\n", RX_TAIL_RESIDUE_CAP), 0);
        let with_nl = b"\r\nuser@host:~$ ";
        assert_eq!(rx_residue_prefix_len(with_nl, RX_TAIL_RESIDUE_CAP), 0);
        // 不完整的 ESC[?200 不能当 2004 证据（也可能是着色 CSI 的开头）。
        assert_eq!(rx_residue_prefix_len(b"00\x1b[?200", RX_TAIL_RESIDUE_CAP), 0);
    }

    #[test]
    fn test_feed_receive_passes_00_plus_ansi_color_and_crlf() {
        let lrzsz = transfer_after_session_complete();
        let color = color_line();
        match lrzsz.feed_receive(&color) {
            ReceiveFeed::Stripped { rest } | ReceiveFeed::Passthrough { data: rest } => {
                assert_eq!(rest, color, "00 + color CSI + CR/LF must stay intact");
            }
            other => panic!("expected color line passthrough, got {other:?}"),
        }
        assert!(!lrzsz.has_rx_tail_pending());
    }

    #[test]
    fn test_feed_receive_releases_held_00_when_next_chunk_is_color() {
        // 窗口内先到单独 `00`（按住），下一分片是着色正文：必须放回 `00`，不得丢掉。
        let lrzsz = transfer_after_session_complete();
        assert_eq!(lrzsz.feed_receive(b"00"), ReceiveFeed::Consumed);
        assert!(lrzsz.has_rx_tail_pending());
        let mut second = COLOR_GREEN.to_vec();
        second.extend_from_slice(b"OK");
        second.extend_from_slice(COLOR_RESET);
        second.extend_from_slice(b"\r\n");
        match lrzsz.feed_receive(&second) {
            ReceiveFeed::Stripped { rest } | ReceiveFeed::Passthrough { data: rest } => {
                let mut expected = b"00".to_vec();
                expected.extend_from_slice(&second);
                assert_eq!(rest, expected);
                assert!(rest.ends_with(b"\r\n"));
            }
            other => panic!("expected released 00 + color, got {other:?}"),
        }
        assert!(!lrzsz.has_rx_tail_pending());
    }

    #[test]
    fn test_feed_receive_after_window_lone_00_is_not_swallowed() {
        let lrzsz = transfer_after_session_complete_ended_ago(
            RX_TAIL_WINDOW + Duration::from_millis(500),
        );
        match lrzsz.feed_receive(b"00") {
            ReceiveFeed::Stripped { rest } | ReceiveFeed::Passthrough { data: rest } => {
                assert_eq!(rest, b"00");
            }
            other => panic!("lone 00 after the window must be shown, got {other:?}"),
        }
        assert!(!lrzsz.has_rx_tail_pending());
    }

    #[test]
    fn test_feed_receive_00_plus_osc_without_prompt_is_kept() {
        let lrzsz = transfer_after_session_complete();
        let mut data = b"00".to_vec();
        data.extend_from_slice(OSC_TITLE);
        match lrzsz.feed_receive(&data) {
            ReceiveFeed::Stripped { rest } | ReceiveFeed::Passthrough { data: rest } => {
                assert_eq!(
                    rest, data,
                    "00 + OSC title must not be stripped without prompt evidence"
                );
            }
            other => panic!("expected OSC kept, got {other:?}"),
        }
    }

    #[test]
    fn test_feed_receive_00_plus_osc_and_prompt_strips_only_zeros() {
        let lrzsz = transfer_after_session_complete();
        let mut data = b"00".to_vec();
        data.extend_from_slice(OSC_TITLE);
        data.extend_from_slice(PROMPT);
        match lrzsz.feed_receive(&data) {
            ReceiveFeed::Stripped { rest } => {
                let mut expected = OSC_TITLE.to_vec();
                expected.extend_from_slice(PROMPT);
                assert_eq!(rest, expected);
                assert_eq!(rest.first().copied(), Some(0x1b));
                assert!(!rest.contains(&b'\r'));
            }
            other => panic!("expected OSC+prompt, got {other:?}"),
        }
        assert!(!lrzsz.has_rx_tail_pending());
    }

    fn sha256_bytes(data: &[u8]) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(data);
        h.finalize().into()
    }

    #[test]
    fn test_feed_receive_residue_does_not_change_saved_file_hash() {
        // 残尾吸收发生在 SessionComplete 之后，不得改写已落盘文件。
        let payload = b"qa tiny payload for #458 hash integrity\n";
        let path = std::env::temp_dir().join(format!(
            "mistterm-458-hash-integrity-{}.txt",
            std::process::id()
        ));
        std::fs::write(&path, payload).expect("write fixture");
        let before = sha256_bytes(payload);
        assert_eq!(sha256_bytes(&std::fs::read(&path).unwrap()), before);

        let lrzsz = transfer_after_session_complete();
        if let Some(RecvSide::Active(session)) = lrzsz.recv_side.lock().unwrap().as_mut() {
            session.last_save_path = Some(path.clone());
        }

        assert_eq!(lrzsz.feed_receive(GUI_LEFTOVER_00B08), ReceiveFeed::Consumed);
        let mut glued = b"00".to_vec();
        glued.extend_from_slice(&prompt_with_csi());
        let _ = lrzsz.feed_receive(&glued);
        let _ = lrzsz.feed_receive(&color_line());

        let on_disk = std::fs::read(&path).expect("read fixture");
        assert_eq!(on_disk, payload, "saved file bytes must be unchanged");
        assert_eq!(
            sha256_bytes(&on_disk),
            before,
            "saved file sha256 must be unchanged"
        );
        if let Some(RecvSide::Active(session)) = lrzsz.recv_side.lock().unwrap().as_ref() {
            assert_eq!(session.last_save_path.as_deref(), Some(path.as_path()));
        }
        let _ = std::fs::remove_file(&path);
    }
}
