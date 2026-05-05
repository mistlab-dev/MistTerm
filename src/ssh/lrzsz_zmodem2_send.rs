//! 本机 → 远端 `rz` 的 ZMODEM **发送**侧，使用 [`zmodem2`] 状态机实现协议；
//! 与 SFTP 面板、`cat`/`scp` 直传在入口与链路上完全独立（仅共用 shell 泵 `ZmodemWrite`）。
//!
//! 调试：设 **`MISTTERM_ZMODEM_DUMP_TX=1`** 可累计经 **`ZmodemWrite`→PTY** 的全部字节，在首次入站停滞 WARN、会话完成、超时或取消时打印 hex（与外部 `sz` / `RZ_SMOKE_DUMP_TX` 对照）；超过 64KiB 仅打印长度与前 64KiB；每行带 **`0000` 字节偏移** 以免换行误读帧边界。
//! **`MISTTERM_ZMODEM_LOG_ZFILE_PAYLOAD=1`**（或与 **`DUMP_TX` 同时开启**）时在上传开始额外打印 **ZFILE 子包逻辑内容（ZDLE 转义前）**，与 `vendor/zmodem2::write_zfile` 内 `buf` 一致，便于和线材 hex 对照边界。
//!
//! **若对端长期不发 ZRPOS / 冒烟 exit 128**，而 DUMP 与 `sz` 首包一致：优先查服务端 **`rz`、/tmp、lrzsz 版本**。可继续做的代码向改动（未默认开启）：
//! - **Golden**：把 `sz` 写到 pipe 的前 N 字节与 DUMP 逐字节 diff。
//! - **ZFILE 扩展域**：按协议文档在文件名与长度后补充 **mtime/mode**（八进制），需扩展 `vendor/zmodem2` 的 `write_zfile` 并从本地 `std::fs::metadata` 取值。
//! - **兜底**：`MISTTERM_ZMODEM_USE_EXTERNAL_SZ=1` 走系统 **`sz`**（已实现）。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender as EventSender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use zmodem2::{Sender as ZmodemSender, SenderEvent};

use crate::ssh::lrzsz::TransferEvent;
use crate::ssh::manager::{ShellPumpCommand, ShellPumpTx};
use crate::ssh::zmodem_pty_pipeline::{UploadIngressPhase, ZmodemPtyIngress};

const TRANSFER_DEADLINE: Duration = Duration::from_secs(300);

fn hex_preview(data: &[u8], max_bytes: usize) -> String {
    let n = data.len().min(max_bytes);
    if n == 0 {
        return String::new();
    }
    let mut s = String::with_capacity(n * 3);
    for (i, b) in data.iter().take(n).enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("{:02x}", b));
    }
    if data.len() > n {
        s.push_str(" …");
    }
    s
}

/// 粗略区分「疑似终端 ANSI/OSC」与二进制协议字节，便于对照日志。
fn ingress_class_hint(data: &[u8]) -> &'static str {
    if data.is_empty() {
        return "empty";
    }
    // bracketed paste `\e[?2004h` / `\e[?2004l`
    if data.windows(8).any(|w| {
        w[0] == 0x1b && w[1] == b'[' && w[2] == b'?' && w[3] == b'2'
            && w[4] == b'0' && w[5] == b'0' && w[6] == b'4'
    }) {
        return "CSI_bracketed_paste";
    }
    if data[0] == 0x1b {
        return "leading_ESC_CSI_OSC";
    }
    let n = data.len();
    let can_bs = data.iter().filter(|&&b| b == 0x18 || b == 0x08).count();
    if n >= 8 && can_bs * 2 >= n {
        return "CAN_BS_dominated";
    }
    if data.windows(2).any(|w| w == [0x2a, 0x2a]) || data.contains(&0x18) {
        return "zmodem_like";
    }
    "other"
}

fn contains_shell_fallback_markers(data: &[u8]) -> bool {
    data.windows("command not found".len())
        .any(|w| w == b"command not found")
        || data.windows(":~$ ".len()).any(|w| w == b":~$ ")
        || data.windows("# ".len()).any(|w| w == b"# ")
}

/// 远端若回退到 X/YMODEM，常周期性发送单字节 `C`（可夹杂 CR/LF）。
fn contains_xymodem_c_fallback(data: &[u8]) -> bool {
    let mut saw = false;
    for &b in data {
        match b {
            b'\r' | b'\n' => {}
            b'C' => saw = true,
            _ => return false,
        }
    }
    saw
}

/// 统计远端 ZHEX `ZRINIT` 前缀 `** ZDLE 'B' '0' '1'` 出现次数。
fn count_peer_zrinit_hex_prefix(data: &[u8]) -> usize {
    data.windows(6)
        .filter(|w| **w == [0x2a, 0x2a, 0x18, b'B', b'0', b'1'])
        .count()
}

/// 将 PTY 字节流粗略清洗为可读文本片段（保留可打印 ASCII），用于错误提示。
fn readable_excerpt(data: &[u8], max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut prev_space = false;
    for &b in data {
        if out.chars().count() >= max_chars {
            break;
        }
        let c = match b {
            b'\r' | b'\n' | b'\t' => ' ',
            0x20..=0x7e => b as char,
            _ => continue,
        };
        if c.is_whitespace() {
            if prev_space {
                continue;
            }
            out.push(' ');
            prev_space = true;
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

fn wire_hex_dump_enabled() -> bool {
    std::env::var("MISTTERM_ZMODEM_WIRE_HEX")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn dump_tx_enabled() -> bool {
    std::env::var("MISTTERM_ZMODEM_DUMP_TX")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn zfile_payload_log_enabled() -> bool {
    std::env::var("MISTTERM_ZMODEM_LOG_ZFILE_PAYLOAD")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn zfile_bin32_enabled() -> bool {
    std::env::var("MISTTERM_ZMODEM_ZFILE_BIN32")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn sender_reply_zrinit_enabled() -> bool {
    std::env::var("MISTTERM_ZMODEM_SENDER_REPLY_ZRINIT")
        .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false") || v.eq_ignore_ascii_case("no")))
        .unwrap_or(false)
}

/// 与 `vendor/zmodem2` `write_zfile` 中写入子包前 `buf` 相同：
/// `name\0` + `<size> <mtime(oct)> <mode(oct)> <serial(oct)> <filesleft> <bytesleft>\0`。
fn zfile_subpacket_payload_pre_escape(name: &[u8], size: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(name);
    buf.push(b'\0');
    buf.extend_from_slice(format!("{size} 0 100644 0 0 {size}\0").as_bytes());
    buf
}

fn log_zfile_logical_payload_if_enabled(file_name: &str, size: u32) {
    if !dump_tx_enabled() && !zfile_payload_log_enabled() {
        return;
    }
    let p = zfile_subpacket_payload_pre_escape(file_name.as_bytes(), size);
    const ROW: usize = 32;
    log::info!(
        "MISTTERM_ZMODEM ZFILE 子包逻辑 payload（转义前）{} B：`{}` size={}：",
        p.len(),
        file_name,
        size
    );
    let mut off = 0usize;
    for row in p.chunks(ROW) {
        log::info!(
            "    zfile-payload {:04x}  {}",
            off,
            row.iter()
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<_>>()
                .join(" ")
        );
        off += row.len();
    }
}

/// 累计发往 PTY 的字节 hex；超长仅打前 64KiB；每行前缀 **4 位十六进制偏移**。
fn log_tx_dump_accum(context: &str, buf: &[u8]) {
    const CAP: usize = 65536;
    const ROW: usize = 32;
    if buf.is_empty() {
        return;
    }
    let dump_slice = if buf.len() > CAP { &buf[..CAP] } else { buf };
    if buf.len() > CAP {
        log::info!(
            "MISTTERM_ZMODEM_DUMP_TX {}: 累计 {} B，仅 hex 前 {} B（带偏移）：",
            context,
            buf.len(),
            CAP
        );
    } else {
        log::info!(
            "MISTTERM_ZMODEM_DUMP_TX {}: 累计 {} B（带偏移）：",
            context,
            buf.len()
        );
    }
    let mut off = 0usize;
    for row in dump_slice.chunks(ROW) {
        log::info!(
            "    {:04x}  {}",
            off,
            row.iter()
                .map(|b| format!("{:02x}", b))
                .collect::<Vec<_>>()
                .join(" ")
        );
        off += row.len();
    }
}

/// ZHEX 形式 ZRINIT 行常以 CR LF + XON(0x11) 结尾，其后紧跟 BIN ZFILE；lrzsz 在 PTY 上有时需分两次写出。
fn split_zrinit_zfile_after_zhex_line(out: &[u8]) -> Option<usize> {
    for i in 0..out.len().saturating_sub(2) {
        if out[i] == 0x0d && out[i + 1] == 0x0a && out.get(i + 2).copied() == Some(0x11) {
            let split = i + 3;
            if split < out.len() {
                return Some(split);
            }
        }
    }
    None
}

/// 默认关闭：实测拆两段后 PTY 侧可出现 ZDLE(0x18) 被收成 BEL(0x07) 等错位；需要时再 `MISTTERM_ZMODEM_SPLIT_HANDSHAKE=1`
fn split_handshake_writes_enabled() -> bool {
    std::env::var("MISTTERM_ZMODEM_SPLIT_HANDSHAKE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn flush_sender_out(
    sender: &mut ZmodemSender,
    pump_tx: &ShellPumpTx,
    first_wire_logged: &mut bool,
    dump_tx: bool,
    tx_accum: &mut Vec<u8>,
) -> Result<(), String> {
    let mut total = 0usize;
    let mut chunks = 0usize;
    loop {
        let out = sender.drain_outgoing();
        if out.is_empty() {
            break;
        }

        if split_handshake_writes_enabled() {
            if let Some(split) = split_zrinit_zfile_after_zhex_line(out) {
                if !*first_wire_logged {
                    *first_wire_logged = true;
                    if wire_hex_dump_enabled() {
                        log::info!(
                            "ZMODEM 首包拆两段写 PTY（ZRINIT ZHEX 行 | ZFILE BIN）总长={} A={} B={}",
                            out.len(),
                            split,
                            out.len() - split
                        );
                        log::info!(
                            "  hex A=[{}]",
                            hex_preview(&out[..split], out[..split].len().min(512))
                        );
                        log::info!(
                            "  hex B=[{}]",
                            hex_preview(&out[split..], out[split..].len().min(512))
                        );
                    }
                }
                if dump_tx {
                    tx_accum.extend_from_slice(&out[..split]);
                }
                pump_tx
                    .send(ShellPumpCommand::ZmodemWrite(out[..split].to_vec()))
                    .map_err(|e| {
                        log::error!("flush_sender_out: pump_tx.send 失败: {}", e);
                        format!("SSH shell 泵已关闭或队列断开: {}", e)
                    })?;
                sender.advance_outgoing(split);
                total += split;
                chunks += 1;

                let rest = sender.drain_outgoing();
                let rn = rest.len();
                if dump_tx {
                    tx_accum.extend_from_slice(rest);
                }
                pump_tx
                    .send(ShellPumpCommand::ZmodemWrite(rest.to_vec()))
                    .map_err(|e| {
                        log::error!("flush_sender_out: pump_tx.send 失败: {}", e);
                        format!("SSH shell 泵已关闭或队列断开: {}", e)
                    })?;
                sender.advance_outgoing(rn);
                total += rn;
                chunks += 1;

                log::debug!(
                    "flush_sender_out: 握手拆两段 → ZmodemWrite {} + {} bytes",
                    split,
                    rn
                );
                continue;
            }
        }

        if !*first_wire_logged {
            *first_wire_logged = true;
            if wire_hex_dump_enabled() {
                let cap = out.len().min(512);
                log::info!(
                    "ZMODEM 首包写出 PTY（可与真 sz / rz -bye 对照）n={} hex=[{}]",
                    out.len(),
                    hex_preview(&out, cap)
                );
            }
        }
        let n = out.len();
        total += n;
        chunks += 1;
        if dump_tx {
            tx_accum.extend_from_slice(out);
        }
        pump_tx
            .send(ShellPumpCommand::ZmodemWrite(out.to_vec()))
            .map_err(|e| {
                log::error!("flush_sender_out: pump_tx.send 失败: {}", e);
                format!("SSH shell 泵已关闭或队列断开: {}", e)
            })?;
        sender.advance_outgoing(n);
    }
    if chunks > 1 {
        log::debug!(
            "flush_sender_out: {} 块共 {} bytes → ZmodemWrite",
            chunks,
            total
        );
    }
    Ok(())
}

/// 使用 zmodem2 完成一次「sz → 远端 rz」上传；不持有 `Channel`，只通过 `pump_tx` 写出。
pub(super) fn run_upload_zmodem2(
    file_data: &[u8],
    file_name: &str,
    file_path: PathBuf,
    file_size: u64,
    pump_tx: &ShellPumpTx,
    upload_pty_rx: &Arc<Mutex<Vec<u8>>>,
    is_active: &AtomicBool,
    received_bytes: &AtomicU64,
    tx: &EventSender<TransferEvent>,
    upload_pty_feed_bytes: &Arc<AtomicU64>,
    upload_pty_pull_bytes: &Arc<AtomicU64>,
) -> Result<(), String> {
    // 握手期攒在队列里的字节视为已 feed，避免 run_upload 起始 feed=0、pull 后「差」失真
    let backlog = upload_pty_rx.lock().map(|g| g.len()).unwrap_or(0);
    upload_pty_feed_bytes.store(backlog as u64, Ordering::Relaxed);
    upload_pty_pull_bytes.store(0, Ordering::Relaxed);

    let sz_u32 = u32::try_from(file_size).map_err(|_| {
        "文件大小超过 ZMODEM 32 位上限（4GiB-1），请使用 SFTP 或「直传」上传".to_string()
    })?;

    let mut ingress = ZmodemPtyIngress::new();
    ingress.pull_from_rx(upload_pty_rx, upload_pty_pull_bytes.as_ref());
    let prep0 = ingress.preprocess_for_phase(UploadIngressPhase::Handshake);
    const INIT_BACKLOG_KEEP: usize = 96;
    if ingress.buf.len() > INIT_BACKLOG_KEEP {
        let drop_n = ingress.buf.len() - INIT_BACKLOG_KEEP;
        ingress.buf.drain(..drop_n);
        log::info!(
            "ZMODEM 预缓冲裁剪：丢弃旧旁路 {} B，仅保留尾部 {} B 参与首轮握手",
            drop_n,
            INIT_BACKLOG_KEEP
        );
    }

    let mut sender = ZmodemSender::new().map_err(|e| format!("ZMODEM Sender::new: {}", e))?;
    // `Sender::new()` 会排队 ZRQINIT（接收端邀请语）；远端 `rz` 已在发 ZRQINIT，本机作为发送端
    // 应回复 ZRINIT（由 vendor/zmodem2 对 ZRQINIT 的处理完成），勿再向 PTY 写入第二条 ZRQINIT。
    let drop_pre = {
        let pre = sender.drain_outgoing();
        let n = pre.len();
        if n > 0 {
            sender.advance_outgoing(n);
        }
        n
    };
    sender
        .start_file(file_name.as_bytes(), sz_u32)
        .map_err(|e| format!("ZMODEM start_file: {}", e))?;
    log_zfile_logical_payload_if_enabled(file_name, sz_u32);

    let no_handshake_strip = std::env::var("MISTTERM_ZMODEM_NO_HANDSHAKE_STRIP")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let skip_post_hex_xon = std::env::var("MISTTERM_ZMODEM_SKIP_POST_HEX_XON")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if no_handshake_strip {
        log::warn!(
            "MISTTERM_ZMODEM_NO_HANDSHAKE_STRIP=1：握手期不剥 CSI/终端序列，feed_incoming 会逐字节吞掉大量 PTY 噪声（如 bracketed-paste/提示符），易长时间无对端 ZRPOS；**日常上传请 unset 或设 0**"
        );
    }
    log::info!(
        "ZMODEM→rz {} size={} B 握手续剥={} B 预缓冲={} B 丢弃Sender预排={} B | WIRE_HEX={} SPLIT={} ZFILE_BIN32={} SENDER_REPLY_ZRINIT={} | NO_HANDSHAKE_STRIP={} SKIP_POST_HEX_XON={}",
        file_name,
        file_size,
        prep0,
        ingress.buf.len(),
        drop_pre,
        wire_hex_dump_enabled(),
        split_handshake_writes_enabled(),
        zfile_bin32_enabled(),
        sender_reply_zrinit_enabled(),
        no_handshake_strip,
        skip_post_hex_xon,
    );

    let dump_tx = dump_tx_enabled();
    let mut tx_dump_buf: Vec<u8> = Vec::new();
    let mut dump_tx_logged_at_stall = false;

    let mut first_wire_logged = false;
    flush_sender_out(
        &mut sender,
        pump_tx,
        &mut first_wire_logged,
        dump_tx,
        &mut tx_dump_buf,
    )?;

    let deadline = Instant::now() + TRANSFER_DEADLINE;
    let loop_started = Instant::now();
    let mut session_done = false;
    let mut file_complete_sent = false;
    // 已为 `poll_file` 供过数据：此后 PTY 上多为 ZDATA 二进制，不再剥前导以免误伤载荷。
    let mut file_data_started = false;
    // 握手期重复看到对端 ZRINIT（ZHEX B01）但始终无 ZRPOS，可判定为严格模式不兼容。
    let mut peer_zrinit_reinvite_count = 0usize;
    // 是否已主动触发过 WaitFilePos 兼容恢复（含 ZSINIT 路径）。
    let mut wait_file_pos_recover_attempted = false;
    // 兼容恢复后的等待窗口：避免同一批 backlog ZRINIT 触发连续补发，给对端留出 ZACK/ZRPOS 响应时间。
    let mut recover_cooldown_until: Option<Instant> = None;
    // 上次「拉 PTY / feed 消费到字节」时间，用于停滞告警
    let mut last_ingress_activity = Instant::now();
    let mut last_stall_warn: Option<Instant> = None;

    while Instant::now() < deadline {
        if !is_active.load(Ordering::Relaxed) {
            if dump_tx && !tx_dump_buf.is_empty() {
                log_tx_dump_accum("用户取消", &tx_dump_buf);
            }
            return Err("传输已由用户取消（Ctrl+C）".to_string());
        }

        flush_sender_out(
            &mut sender,
            pump_tx,
            &mut first_wire_logged,
            dump_tx,
            &mut tx_dump_buf,
        )?;
        let len_before_pull = ingress.buf.len();
        ingress.pull_from_rx(upload_pty_rx, upload_pty_pull_bytes.as_ref());
        if ingress.buf.len() > len_before_pull {
            last_ingress_activity = Instant::now();
        }
        let ingress_phase = if file_data_started {
            UploadIngressPhase::Binary
        } else {
            UploadIngressPhase::Handshake
        };
        let n = ingress.preprocess_for_phase(ingress_phase);
        if n > 0 && !file_data_started {
            log::info!("ZMODEM 握手续剥 {} B（含内嵌 CSI/纯提示符丢弃）", n);
        }

        while !ingress.buf.is_empty() {
            let before = ingress.buf.len();
            let consumed = sender
                .feed_incoming(&ingress.buf)
                .map_err(|e| format!("ZMODEM feed_incoming: {}", e))?;
            if consumed == 0 {
                break;
            }
            last_ingress_activity = Instant::now();
            ingress.on_fed(consumed);
            let pending_out = sender.drain_outgoing().len();
            let slice = &ingress.buf[..consumed];
            if !file_data_started {
                let zhex_zrinit_cnt = count_peer_zrinit_hex_prefix(slice);
                if zhex_zrinit_cnt > 0 {
                    peer_zrinit_reinvite_count = peer_zrinit_reinvite_count
                        .saturating_add(zhex_zrinit_cnt);
                } else {
                    recover_cooldown_until = None;
                }
            }
            if !file_data_started && contains_xymodem_c_fallback(slice) {
                return Err(
                    "远端 rz 已切换到 X/YMODEM（收到连续 'C'），本次 ZMODEM 会话已失配；请在远端重新执行 rz 后重试（建议优先 rz -y）".to_string(),
                );
            }
            if pending_out == 0 {
                if consumed >= 8 {
                    let hint = ingress_class_hint(slice);
                    let excerpt = readable_excerpt(slice, 120);
                    log::info!(
                        "ZMODEM feed 后无待发帧 consumed={} buf_was={} class={} text='{}' hex48=[{}]",
                        consumed,
                        before,
                        hint,
                        excerpt,
                        hex_preview(slice, 48)
                    );
                    if contains_shell_fallback_markers(slice) {
                        let detail = if excerpt.is_empty() {
                            "<无可读文本>".to_string()
                        } else {
                            excerpt
                        };
                        return Err(
                            format!(
                                "远端已回到 shell（检测到 command not found / prompt），rz 可能已退出；请在远端重新执行 rz 再试。首段回显: {}",
                                detail
                            ),
                        );
                    }
                }
            } else {
                log::debug!(
                    "feed_incoming ok consumed={} pending_out={} 入站hex16=[{}]",
                    consumed,
                    pending_out,
                    hex_preview(slice, 16)
                );
            }
            ingress.buf.drain(..consumed);
            flush_sender_out(
                &mut sender,
                pump_tx,
                &mut first_wire_logged,
                dump_tx,
                &mut tx_dump_buf,
            )?;
        }

        while let Some(req) = sender.poll_file() {
            file_data_started = true;
            let off = req.offset as usize;
            let end = off.checked_add(req.len).ok_or_else(|| {
                "ZMODEM FileRequest 偏移溢出".to_string()
            })?;
            let slice = file_data
                .get(off..end)
                .ok_or_else(|| format!("ZMODEM 请求区间 [{}..{}) 越界", off, end))?;
            sender
                .feed_file(slice)
                .map_err(|e| format!("ZMODEM feed_file: {}", e))?;
            received_bytes.store(end as u64, Ordering::Relaxed);
            let _ = tx.send(TransferEvent::FileProgress {
                filename: file_name.to_string(),
                received: end as u64,
                total: file_size,
            });
            if end & 0x7fff == 0 {
                std::thread::yield_now();
            }
            flush_sender_out(
                &mut sender,
                pump_tx,
                &mut first_wire_logged,
                dump_tx,
                &mut tx_dump_buf,
            )?;
        }

        loop {
            let ev = match sender.poll_event() {
                Some(e) => e,
                None => break,
            };
            match ev {
                SenderEvent::FileComplete => {
                    if !file_complete_sent {
                        file_complete_sent = true;
                        let _ = tx.send(TransferEvent::FileComplete {
                            filename: file_name.to_string(),
                            path: file_path.clone(),
                        });
                        log::debug!("ZMODEM 文件协议层完成 {}", file_name);
                    }
                    sender
                        .finish_session()
                        .map_err(|e| format!("ZMODEM finish_session: {}", e))?;
                    flush_sender_out(
                        &mut sender,
                        pump_tx,
                        &mut first_wire_logged,
                        dump_tx,
                        &mut tx_dump_buf,
                    )?;
                }
                SenderEvent::SessionComplete => {
                    session_done = true;
                    log::debug!("ZMODEM 会话结束");
                }
            }
        }

        if session_done {
            flush_sender_out(
                &mut sender,
                pump_tx,
                &mut first_wire_logged,
                dump_tx,
                &mut tx_dump_buf,
            )?;
            if dump_tx && !tx_dump_buf.is_empty() {
                log_tx_dump_accum("会话完成", &tx_dump_buf);
            }
            log::info!(
                "ZMODEM 会话完成 旁路 feed={} B pull={} B | 管道 pulled={} stripped={} fed={}",
                upload_pty_feed_bytes.load(Ordering::Relaxed),
                upload_pty_pull_bytes.load(Ordering::Relaxed),
                ingress.total_pulled,
                ingress.total_stripped,
                ingress.total_fed
            );
            break;
        }

        // 入站长时间无数据且无会话结束：打疏告警（默认 INFO，约每 12s 最多一条）
        if !session_done
            && ingress.buf.is_empty()
            && last_ingress_activity.elapsed() >= Duration::from_secs(4)
        {
            let now = Instant::now();
            let due = last_stall_warn
                .map(|t| now.duration_since(t) >= Duration::from_secs(12))
                .unwrap_or(true);
            if due {
                last_stall_warn = Some(now);
                if !file_data_started && peer_zrinit_reinvite_count >= 2 {
                    let cooldown_active = recover_cooldown_until
                        .map(|t| Instant::now() < t)
                        .unwrap_or(false);
                    if cooldown_active {
                        continue;
                    }
                    let retried = sender
                        .retry_wait_file_pos_handshake()
                        .map_err(|e| format!("ZMODEM retry_wait_file_pos_handshake: {}", e))?;
                    if retried {
                        wait_file_pos_recover_attempted = true;
                        peer_zrinit_reinvite_count = 0;
                        recover_cooldown_until = Some(Instant::now() + Duration::from_millis(900));
                        log::warn!(
                            "停滞期检测到重复 ZRINIT，已再次触发兼容重试（等待对端 ZACK/ZRPOS）"
                        );
                        flush_sender_out(
                            &mut sender,
                            pump_tx,
                            &mut first_wire_logged,
                            dump_tx,
                            &mut tx_dump_buf,
                        )?;
                        continue;
                    }
                    if wait_file_pos_recover_attempted {
                        return Err(
                            "远端重复发送 ZRINIT 且客户端已尝试兼容重试（含 ZSINIT）仍未进入 ZRPOS；当前会话与 rz -e/-bye 严格模式不兼容。建议改用 rz -y，或升级/更换远端 lrzsz 版本".to_string(),
                        );
                    }
                    return Err(
                        "远端重复发送 ZRINIT 但未进入 ZRPOS，当前会话与 rz -e/-bye 严格模式不兼容；请改用 rz -y".to_string(),
                    );
                }
                let rx_tail = upload_pty_rx.lock().map(|q| q.len()).unwrap_or(0);
                let feed = upload_pty_feed_bytes.load(Ordering::Relaxed);
                let pull = upload_pty_pull_bytes.load(Ordering::Relaxed);
                log::warn!(
                    "ZMODEM 入站空闲 {:.1}s（累计 {:.1}s）仍无对端帧；旁路 feed={} B pull={} B 差={} 队尾={} B（差≈0且队尾0→泵/UI 未再旁路到队列）",
                    last_ingress_activity.elapsed().as_secs_f32(),
                    loop_started.elapsed().as_secs_f32(),
                    feed,
                    pull,
                    feed as i64 - pull as i64,
                    rx_tail
                );
                if dump_tx && !tx_dump_buf.is_empty() && !dump_tx_logged_at_stall {
                    dump_tx_logged_at_stall = true;
                    log_tx_dump_accum("入站停滞（首条 WARN）", &tx_dump_buf);
                }
            }
        }

        std::thread::sleep(Duration::from_millis(1));
    }

    if !session_done {
        if dump_tx && !tx_dump_buf.is_empty() {
            log_tx_dump_accum("超时未完成", &tx_dump_buf);
        }
        return Err(format!(
            "ZMODEM 超时（{} 秒内未完成会话）",
            TRANSFER_DEADLINE.as_secs()
        ));
    }

    Ok(())
}
