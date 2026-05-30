//! PTY 上 rz 常在 ZMODEM 帧前插入 ANSI；与 `zmodem2::Sender` 的 SeekingZpad 组合时会把整段当噪声吞掉。

use zmodem2::{ZDLE, ZPAD};

const ESC: u8 = 0x1b;

/// 握手阶段剥掉 PTY 侧插入的终端序列（CSI/OSC/DCS、charset 两/三字节），避免被 `feed_incoming` 整段当协议字节吞掉。
/// **仅在未进入 ZDATA 前使用**；文件数据阶段勿调用，以免误伤二进制载荷。
pub fn strip_handshake_pty_noise(buf: &mut Vec<u8>) -> usize {
    let mut total = 0usize;
    for _ in 0..4096 {
        if buf.is_empty() {
            break;
        }
        let bel = strip_leading_byte_run(buf, 0x07, 32);
        total += bel;
        if buf.is_empty() {
            break;
        }
        let spin = strip_leading_can_bs_spinner(buf);
        total += spin;
        if buf.is_empty() {
            break;
        }
        if buf[0] != ESC {
            break;
        }
        let n = strip_one_terminal_escape(buf);
        if n == 0 {
            break;
        }
        total += n;
    }
    total
}

fn strip_leading_byte_run(buf: &mut Vec<u8>, byte: u8, max: usize) -> usize {
    let n = buf
        .iter()
        .take(max)
        .take_while(|&&b| b == byte)
        .count();
    if n > 0 {
        buf.drain(..n);
    }
    n
}

/// 连续 CAN/退格多为终端进度条；仅当头部长段均为 0x18/0x08 时剥掉，避免误删合法 ZPAD。
fn strip_leading_can_bs_spinner(buf: &mut Vec<u8>) -> usize {
    const MIN_RUN: usize = 16;
    const MAX_RUN: usize = 512;
    if buf.len() < MIN_RUN {
        return 0;
    }
    if !buf[..MIN_RUN]
        .iter()
        .all(|&b| b == 0x18 || b == 0x08)
    {
        return 0;
    }
    let mut i = 0usize;
    while i < buf.len() && i < MAX_RUN && (buf[i] == 0x18 || buf[i] == 0x08) {
        i += 1;
    }
    if i >= MIN_RUN {
        buf.drain(..i);
        return i;
    }
    0
}

fn strip_one_terminal_escape(buf: &mut Vec<u8>) -> usize {
    if buf.len() < 2 || buf[0] != ESC {
        return 0;
    }
    match buf[1] {
        b'[' => strip_one_csi(buf),
        b']' => strip_one_osc(buf),
        b'P' => strip_dcs_to_st(buf),
        b'(' | b')' | b'%' if buf.len() >= 3 => {
            buf.drain(..3);
            3
        }
        _ => {
            buf.drain(..2);
            2
        }
    }
}

/// CSI：ESC [ … 最终字节 0x40–0x7E（ECMA-48）
fn strip_one_csi(buf: &mut Vec<u8>) -> usize {
    if let Some(n) = csi_len_at(buf) {
        buf.drain(..n);
        return n;
    }
    0
}

/// OSC：ESC ] … BEL 或 ST（ESC \）
fn strip_one_osc(buf: &mut Vec<u8>) -> usize {
    if buf.len() < 3 || buf[0] != ESC || buf[1] != b']' {
        return 0;
    }
    let mut i = 2usize;
    while i < buf.len() {
        if buf[i] == 0x07 {
            i += 1;
            buf.drain(..i);
            return i;
        }
        if buf[i] == ESC && i + 1 < buf.len() && buf[i + 1] == b'\\' {
            i += 2;
            buf.drain(..i);
            return i;
        }
        i += 1;
    }
    0
}

/// DCS：ESC P … ST（ESC \），长度封顶避免误扫整缓冲
fn strip_dcs_to_st(buf: &mut Vec<u8>) -> usize {
    if buf.len() < 3 || buf[0] != ESC || buf[1] != b'P' {
        return 0;
    }
    let limit = buf.len().min(16384);
    let mut i = 2usize;
    while i < limit {
        if buf[i] == ESC && i + 1 < buf.len() && buf[i + 1] == b'\\' {
            i += 2;
            buf.drain(..i);
            return i;
        }
        i += 1;
    }
    0
}

/// 首个 ZMODEM 同步位置（`ZPAD ZPAD ZDLE` 或独立 `ZPAD ZDLE`），供对齐或判断是否含协议字节。
pub fn find_zmodem_sync_offset(buf: &[u8]) -> Option<usize> {
    if buf.len() < 3 {
        return None;
    }
    const MAX_SCAN: usize = 8192;
    let n = buf.len().min(MAX_SCAN);
    let cut_double = (0..n.saturating_sub(2)).find(|&i| {
        buf[i] == ZPAD && buf[i + 1] == ZPAD && buf[i + 2] == ZDLE
    });
    let cut_single = (0..n.saturating_sub(1)).find(|&i| {
        buf[i] == ZPAD
            && buf[i + 1] == ZDLE
            && (i == 0 || buf[i - 1] != ZPAD)
    });
    match (cut_double, cut_single) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

/// 将缓冲对齐到第一个 `ZPAD ZPAD ZDLE` 或「非双星后的」`ZPAD ZDLE`，返回剥掉的字节数。
pub fn strip_leading_until_zmodem_frame_start(buf: &mut Vec<u8>) -> usize {
    if let Some(i) = find_zmodem_sync_offset(buf) {
        if i > 0 {
            buf.drain(..i);
            return i;
        }
    }
    0
}

fn csi_len_at(s: &[u8]) -> Option<usize> {
    if s.len() < 3 || s[0] != ESC || s[1] != b'[' {
        return None;
    }
    let mut i = 2usize;
    while i < s.len() {
        let b = s[i];
        if (0x40..=0x7e).contains(&b) {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn osc_len_at(s: &[u8]) -> Option<usize> {
    if s.len() < 3 || s[0] != ESC || s[1] != b']' {
        return None;
    }
    let mut i = 2usize;
    while i < s.len() {
        if s[i] == 0x07 {
            return Some(i + 1);
        }
        if s[i] == ESC && i + 1 < s.len() && s[i + 1] == b'\\' {
            return Some(i + 2);
        }
        i += 1;
    }
    None
}

fn dcs_len_at(s: &[u8]) -> Option<usize> {
    if s.len() < 3 || s[0] != ESC || s[1] != b'P' {
        return None;
    }
    let limit = s.len().min(16384);
    let mut i = 2usize;
    while i < limit {
        if s[i] == ESC && i + 1 < s.len() && s[i + 1] == b'\\' {
            return Some(i + 2);
        }
        i += 1;
    }
    None
}

/// 去掉缓冲内**任意位置**的 CSI/OSC/DCS 与常见两/三字节 ESC 序列（握手专用）。
/// 用于「ubuntu@…」与着色 CSI 交织时，仅靠剥前导 ESC 不够的情况。
pub fn strip_embedded_terminal_sequences(buf: &mut Vec<u8>) -> usize {
    let old_len = buf.len();
    let mut out = Vec::with_capacity(old_len);
    let mut i = 0usize;
    while i < buf.len() {
        if buf[i] == ESC && i + 1 < buf.len() {
            match buf[i + 1] {
                b'[' => {
                    if let Some(len) = csi_len_at(&buf[i..]) {
                        i += len;
                        continue;
                    }
                }
                b']' => {
                    if let Some(len) = osc_len_at(&buf[i..]) {
                        i += len;
                        continue;
                    }
                }
                b'P' => {
                    if let Some(len) = dcs_len_at(&buf[i..]) {
                        i += len;
                        continue;
                    }
                }
                b'(' | b')' | b'%' if i + 2 < buf.len() => {
                    i += 3;
                    continue;
                }
                _ => {
                    i += 2;
                    continue;
                }
            }
        }
        out.push(buf[i]);
        i += 1;
    }
    let removed = old_len.saturating_sub(out.len());
    *buf = out;
    removed
}

/// 缓冲内既无 ZMODEM 同步、也无任何 ZPAD 时，视为整段 shell 提示符/噪音并清空（握手专用；封顶防误删大包）。
fn strip_prompt_only_chunk_without_zpad(buf: &mut Vec<u8>) -> usize {
    if buf.is_empty() {
        return 0;
    }
    if find_zmodem_sync_offset(buf).is_some() {
        return 0;
    }
    if buf.contains(&ZPAD) {
        return 0;
    }
    // 已出现 ZDLE 但本片段尚无 `*`（跨 TCP/PTY 切包）时勿当「纯提示符」清空，否则可能丢掉半帧。
    if buf.contains(&ZDLE) {
        return 0;
    }
    // 过短或可能为 ZHEX 头 `**` 的半包
    if buf.len() <= 2 {
        return 0;
    }
    if buf[0] == b'*' {
        return 0;
    }
    const MAX_CHUNK: usize = 16384;
    if buf.len() > MAX_CHUNK {
        return 0;
    }
    let n = buf.len();
    buf.clear();
    n
}

/// 部分 PTY/SSH 链路上 ZHEX 行尾为 `\\r` + `0x8a` 而非 `\\r\\n`（与换行 0x0a 仅差高位）。
/// 握手阶段在解析后续字节前规范为 `\\r\\n`，避免残留行尾干扰下一帧同步（不改变缓冲长度）。
pub fn normalize_pty_zhex_crlf_quirk(buf: &mut Vec<u8>) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i + 1 < buf.len() {
        if buf[i] == b'\r' && buf[i + 1] == 0x8a {
            buf[i + 1] = b'\n';
            n += 1;
        }
        i += 1;
    }
    n
}

/// 握手阶段完整预处理：前导终端序列 → 内嵌 CSI/OSC → 对齐 ZPAD → 丢弃无 ZPAD 的纯提示符块。
pub fn strip_handshake_incoming(buf: &mut Vec<u8>) -> usize {
    let mut total = normalize_pty_zhex_crlf_quirk(buf);
    total += strip_handshake_pty_noise(buf);
    total += strip_embedded_terminal_sequences(buf);
    total += strip_leading_until_zmodem_frame_start(buf);
    total += strip_prompt_only_chunk_without_zpad(buf);
    total
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_pty_zhex_crlf_quirk, strip_handshake_incoming, strip_handshake_pty_noise,
        strip_leading_until_zmodem_frame_start,
    };
    use zmodem2::{Encoding, Frame, Header, ZDLE, ZPAD};

    #[test]
    fn normalize_cr_8a_to_lf() {
        let mut buf = vec![b'\r', 0x8a, 0x11];
        assert_eq!(normalize_pty_zhex_crlf_quirk(&mut buf), 1);
        assert_eq!(buf, vec![b'\r', b'\n', 0x11]);
    }

    /// 日志中「纯提示符 + 内嵌 CSI、无 ZPAD」类混流
    #[test]
    fn handshake_strips_ubuntu_prompt_embedded_sgr() {
        let mut buf: Vec<u8> = vec![
            0x75, 0x62, 0x75, 0x6e, 0x74, 0x75, 0x40, 0x56, 0x4d, 0x2d, 0x30, 0x2d, 0x31, 0x37, 0x2d, 0x75,
            0x62, 0x75, 0x6e, 0x74, 0x75, 0x1b, 0x5b, 0x30, 0x30, 0x6d, 0x3a, 0x1b, 0x5b, 0x30, 0x31, 0x3b,
            0x33, 0x34, 0x6d, 0x7e, 0x1b, 0x5b, 0x30, 0x30, 0x6d, 0x24, 0x20,
        ];
        let n = strip_handshake_incoming(&mut buf);
        assert!(n >= 43, "n={} left={:02x?}", n, buf);
        assert!(buf.is_empty());
    }

    #[test]
    fn prompt_strip_skips_star_partial() {
        let mut buf = vec![0x2a_u8];
        assert_eq!(strip_handshake_incoming(&mut buf), 0);
        assert_eq!(buf, vec![0x2a]);
    }

    /// 无 `*` 但已含 ZDLE 的半包：勿当纯提示符清空
    #[test]
    fn prompt_strip_skips_zdle_without_zpad_yet() {
        let mut buf: Vec<u8> = vec![0x0d, 0x0a, ZDLE, 0x43, 0x04];
        let before = buf.len();
        let n = strip_handshake_incoming(&mut buf);
        assert_eq!(n, 0, "should not clear ZDLE-prefixed fragment");
        assert_eq!(buf.len(), before);
    }

    #[test]
    fn handshake_strip_removes_csi_bracketed_paste_and_osc_title() {
        let mut buf: Vec<u8> = Vec::new();
        // \e[?2004h
        buf.extend_from_slice(&[0x1b, b'[', b'?', b'2', b'0', b'0', b'4', b'h']);
        // \e]0;ubuntu@host:~ \x07
        buf.extend_from_slice(&[
            0x1b, b']', b'0', b';', b'u', b'b', 0x07,
        ]);
        let n = strip_handshake_pty_noise(&mut buf);
        assert_eq!(n, 15, "strip={} buf={:02x?}", n, buf);
        assert!(buf.is_empty());
    }

    #[test]
    fn handshake_strip_spinner_then_esc() {
        let mut buf: Vec<u8> = vec![0x18; 20];
        buf.extend_from_slice(&[0x1b, b'[', b'0', b'm']);
        let n = strip_handshake_pty_noise(&mut buf);
        assert_eq!(n, 24);
        assert!(buf.is_empty());
    }

    #[test]
    fn strip_drops_crlf_and_ansi_before_zrinit() {
        let mut buf: Vec<u8> = vec![0x1b, b'[', b'2', b'J', b'\r', b'\n'];
        let h = Header::new(Encoding::ZHEX, Frame::ZRINIT, &[0u8; 4]);
        let mut tail = Vec::new();
        h.write(&mut tail).unwrap();
        buf.extend_from_slice(&tail);
        let n = strip_leading_until_zmodem_frame_start(&mut buf);
        assert!(n > 0);
        assert!(buf.starts_with(&[ZPAD, ZPAD, ZDLE]) || buf.starts_with(&[ZPAD, ZDLE]));
        assert!(buf.len() <= tail.len());
    }
}
