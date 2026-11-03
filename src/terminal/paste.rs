//! 终端粘贴：组包（换行规范化 + bracketed paste）、分片（不切断 UTF-8）、行模式超长行检测。
//!
//! 背景：Linux/macOS 的 tty 在行模式（ICANON，`cat`、`read`、`python input()` 等）下单行
//! 最多缓存 4095 字节（N_TTY_BUF_SIZE=4096，最后一字节留给换行），超出部分被内核直接丢弃，
//! 与终端软件无关。远端开了 bracketed paste 时（交互 shell 的 readline/zle、vim）程序处于
//! 原始模式自行读取，不受此限。

/// tty 行模式下单行能保留的最大字节数（不含换行）。
pub const TTY_CANON_LINE_MAX: usize = 4095;
/// 每次写入 PTY 的分片上限。
pub const PASTE_CHUNK: usize = 4096;

const BP_START: &[u8] = b"\x1b[200~";
const BP_END: &[u8] = b"\x1b[201~";

/// 换行统一成 `\n`（粘贴历史与直接粘贴共用，避免再次粘贴和第一次组包不一致）。
pub fn normalize_paste_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// 换行统一成 `\n`（保持既有行为），开启 bracketed paste 时包上 `ESC[200~ … ESC[201~`，
/// 并剔除正文里伪造的起止标记（防止粘贴内容提前“结束粘贴”后被当命令执行）。
pub fn build_paste_payload(text: &str, bracketed: bool) -> Vec<u8> {
    let normalized = normalize_paste_newlines(text);
    if !bracketed {
        return normalized.into_bytes();
    }
    let body = normalized.replace("\x1b[200~", "").replace("\x1b[201~", "");
    let mut out = Vec::with_capacity(body.len() + BP_START.len() + BP_END.len());
    out.extend_from_slice(BP_START);
    out.extend_from_slice(body.as_bytes());
    out.extend_from_slice(BP_END);
    out
}

/// 最长一行的字节数（不含换行）。
pub fn longest_line_bytes(text: &str) -> usize {
    text.split(['\n', '\r']).map(str::len).max().unwrap_or(0)
}

/// 行模式下会被内核截断的风险：存在超过 4095 字节的单行。
pub fn exceeds_canon_line_limit(text: &str) -> bool {
    longest_line_bytes(text) > TTY_CANON_LINE_MAX
}

/// 取下一片长度：不超过 `max`，且不把 UTF-8 多字节字符切成两半（除非单字符已超过 max）。
pub fn next_chunk_len(buf: &[u8], max: usize) -> usize {
    if buf.len() <= max {
        return buf.len();
    }
    let mut n = max;
    // UTF-8 续字节形如 10xxxxxx；退到字符起点。
    while n > 0 && (buf[n] & 0xC0) == 0x80 {
        n -= 1;
    }
    if n == 0 { max } else { n }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_payload_normalizes_newlines() {
        assert_eq!(build_paste_payload("a\r\nb\rc\n", false), b"a\nb\nc\n");
    }

    #[test]
    fn bracketed_payload_wraps_and_strips_markers() {
        let p = build_paste_payload("ls\x1b[201~rm -rf x\n", true);
        assert_eq!(p, b"\x1b[200~lsrm -rf x\n\x1b[201~");
    }

    #[test]
    fn canon_limit_detection() {
        assert!(!exceeds_canon_line_limit(&"a".repeat(4095)));
        assert!(exceeds_canon_line_limit(&"a".repeat(4096)));
        let many = format!("{}\n", "x".repeat(79)).repeat(2000);
        assert!(!exceeds_canon_line_limit(&many));
    }

    #[test]
    fn chunks_never_split_utf8_and_reassemble_exactly() {
        let text = "中文粘贴测试，abc😀\n".repeat(3000);
        let mut buf = text.as_bytes();
        let mut out = Vec::new();
        while !buf.is_empty() {
            let n = next_chunk_len(buf, PASTE_CHUNK);
            assert!(n > 0 && n <= PASTE_CHUNK);
            assert!(std::str::from_utf8(&buf[..n]).is_ok());
            out.extend_from_slice(&buf[..n]);
            buf = &buf[n..];
        }
        assert_eq!(out, text.as_bytes());
    }
}
