//! `rz` 上传时 **PTY 入站 → zmodem2** 的显式管道（与「终端里打命令、画 UI」解耦）。
//!
//! # 阶段（严格串行，只处理字节流）
//!
//! 1. **旁路入队**（`manager` / `UploadPtyBypass`）：`channel.read` 与 `ZmodemWrite` 同轴，读到的副本进 `upload_pty_rx`。
//! 2. **拉取** [`ZmodemPtyIngress::pull_from_rx`]：本线程从 `upload_pty_rx` 顺序追加到 `buf`（可计量 `total_pulled`）。
//! 3. **握手期过滤**（仅 [`UploadIngressPhase::Handshake`]）：[`ZmodemPtyIngress::preprocess_for_phase`]
//!    调用 `strip_handshake_incoming`；**进入 ZDATA 后**必须切到 [`UploadIngressPhase::Binary`]，**永不再剥**，避免把协议载荷当 ANSI 清掉。
//! 4. **解析** `zmodem2::Sender::feed_incoming`：只消费 `buf` 前缀，由状态机决定 `consumed`（可计量 `total_fed`）。
//!
//! 调试：
//! - `MISTTERM_ZMODEM_PIPELINE_TRACE=1`：在 2→3、3→4 边界打 `trace!`。
//! - `MISTTERM_ZMODEM_NO_HANDSHAKE_STRIP=1`：**握手期也不剥** CSI/提示符，字节流与外部 `sz` stdin 一致；用于排查内置 `zmodem2` 是否被剥壳误伤。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::ssh::zmodem_pty_prefix::strip_handshake_incoming;

const INCOMING_CAP: usize = 512 * 1024;

/// 入站预处理阶段：与「shell 里执行 `rz` 命令」不是同一件事；仅描述 **进入解析器之前** 的字节处理策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UploadIngressPhase {
    /// ZDATA 开始前：允许剥 CSI/纯提示符等，便于对齐 ZPAD。
    Handshake,
    /// 已开始 file data：旁路与缓冲必须为原始字节流，禁止握手剥除。
    Binary,
}

fn pipeline_trace_enabled() -> bool {
    std::env::var("MISTTERM_ZMODEM_PIPELINE_TRACE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// 为 true 时握手阶段跳过 `strip_handshake_incoming`（对齐外部 `sz`：原始 PTY → 解析器）。
fn no_handshake_strip_by_env() -> bool {
    std::env::var("MISTTERM_ZMODEM_NO_HANDSHAKE_STRIP")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn hex16(data: &[u8]) -> String {
    data.iter()
        .take(16)
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

/// PTY 入站到 zmodem2 之间的缓冲与管道级计量。
pub(crate) struct ZmodemPtyIngress {
    pub buf: Vec<u8>,
    /// 从 `upload_pty_rx` 拉入的字节累计（与 `pull` 原子一致）
    pub total_pulled: u64,
    /// 仅在握手期 `strip_handshake_incoming` 删除的字节累计
    pub total_stripped: u64,
    /// 已由 `feed_incoming` 消耗的字节累计
    pub total_fed: u64,
}

impl ZmodemPtyIngress {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(8192),
            total_pulled: 0,
            total_stripped: 0,
            total_fed: 0,
        }
    }

    /// 阶段 2：从旁路队列顺序拉取；与原先 `pull_pty_into` 行为一致。
    pub fn pull_from_rx(&mut self, upload_pty_rx: &Arc<Mutex<Vec<u8>>>, pty_pull: &AtomicU64) {
        let len_before = self.buf.len();
        {
            let mut g = upload_pty_rx.lock().unwrap();
            if g.is_empty() {
                return;
            }
            if self.buf.len() + g.len() > INCOMING_CAP {
                let drop_n = self.buf.len() + g.len() - INCOMING_CAP;
                if drop_n < self.buf.len() {
                    self.buf.drain(..drop_n);
                } else {
                    self.buf.clear();
                    let skip = drop_n.saturating_sub(self.buf.len());
                    if skip < g.len() {
                        g.drain(..skip);
                    } else {
                        g.clear();
                    }
                }
                log::warn!(
                    "ZMODEM 入站缓冲超过 {} KiB，已丢弃最旧数据（请检查对端是否异常喷流）",
                    INCOMING_CAP / 1024
                );
            }
            let n = g.len();
            self.buf.extend_from_slice(&g);
            g.clear();
            pty_pull.fetch_add(n as u64, Ordering::Relaxed);
            self.total_pulled += n as u64;
        }
        if pipeline_trace_enabled() {
            log::trace!(
                "ZMODEM pipe: pull len_after={} (+{} B)",
                self.buf.len(),
                self.buf.len().saturating_sub(len_before)
            );
        }
    }

    /// 阶段 3：仅握手期剥噪声；Binary 阶段必须为 no-op。
    pub fn preprocess_for_phase(&mut self, phase: UploadIngressPhase) -> usize {
        match phase {
            UploadIngressPhase::Binary => 0,
            UploadIngressPhase::Handshake => {
                if no_handshake_strip_by_env() {
                    return 0;
                }
                let n = strip_handshake_incoming(&mut self.buf);
                self.total_stripped += n as u64;
                if pipeline_trace_enabled() && n > 0 {
                    log::trace!("ZMODEM pipe: strip removed={} B buf_len={}", n, self.buf.len());
                }
                n
            }
        }
    }

    pub fn on_fed(&mut self, consumed: usize) {
        self.total_fed += consumed as u64;
        if pipeline_trace_enabled() && consumed > 0 {
            let take = consumed.min(self.buf.len()).min(16);
            let head = self.buf.get(..take).unwrap_or(&[]);
            log::trace!(
                "ZMODEM pipe: fed consumed={} total_fed={} head16=[{}]",
                consumed,
                self.total_fed,
                hex16(head)
            );
        }
    }
}
