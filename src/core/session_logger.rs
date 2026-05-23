//! 终端输出会话日志（按会话/日期落盘）

use chrono::Local;
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const DEFAULT_RETENTION_DAYS: u32 = 30;
pub const DEFAULT_MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;
pub const LOG_TAIL_READ_BYTES: usize = 512 * 1024;

/// 全局日志偏好
#[derive(Debug, Clone)]
pub struct SessionLogSettings {
    pub enabled: bool,
    pub base_dir: PathBuf,
    pub retention_days: u32,
    pub include_ansi: bool,
    pub max_file_bytes: u64,
}

impl Default for SessionLogSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            base_dir: default_log_base_dir(),
            retention_days: DEFAULT_RETENTION_DAYS,
            include_ansi: false,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
        }
    }
}

pub fn default_log_base_dir() -> PathBuf {
    let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("mistterm");
    p.push("logs");
    p
}

/// 启动时后台清理过期日志目录
pub fn spawn_cleanup_old_logs(base: PathBuf, retention_days: u32) {
    std::thread::spawn(move || {
        cleanup_old_logs(&base, retention_days);
    });
}

pub fn cleanup_old_logs(base: &Path, retention_days: u32) {
    let Ok(read) = fs::read_dir(base) else {
        return;
    };
    let cutoff =
        (Local::now() - chrono::Duration::days(retention_days as i64)).date_naive();
    for entry in read.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(sub) = fs::read_dir(&path) else {
            continue;
        };
        for f in sub.flatten() {
            let fp = f.path();
            if fp.extension().and_then(|e| e.to_str()) != Some("log") {
                continue;
            }
            if let Some(stem) = fp.file_stem().and_then(|s| s.to_str()) {
                if let Ok(date) = chrono::NaiveDate::parse_from_str(stem, "%Y-%m-%d") {
                    if date < cutoff {
                        let _ = fs::remove_file(&fp);
                    }
                }
            }
        }
    }
}

/// 单个活动标签的日志写入器
pub struct SessionLogWriter {
    session_id: String,
    session_name: String,
    host_line: String,
    settings: SessionLogSettings,
    writer: Option<BufWriter<std::fs::File>>,
    current_path: Option<PathBuf>,
    current_date: String,
    last_flush: Instant,
    disabled: bool,
    /// PTY 分片缓冲：勿对无 `\n` 的 chunk 强行换行（否则按键回显会一字一行）
    pty_buffer: Vec<u8>,
    /// vim/less 等交替屏幕（1049/47）嵌套深度；>0 时不记录 PTY 刷屏
    alt_screen_depth: u32,
    /// 连续「仅 ~ / 空行」计数（vim 退出时空行填充）
    tilde_pad_run: u32,
    /// 与上一行完全相同、尚未落盘的重复次数
    dedupe_line: Vec<u8>,
    dedupe_count: u32,
    /// 刚写入命令标记，用于抑制 PTY 回显重复
    last_marked_command: Option<String>,
}

impl SessionLogWriter {
    pub fn new(
        session_id: String,
        session_name: String,
        host_line: String,
        settings: SessionLogSettings,
    ) -> Self {
        Self {
            session_id,
            session_name,
            host_line,
            settings,
            writer: None,
            current_path: None,
            current_date: String::new(),
            last_flush: Instant::now(),
            disabled: false,
            pty_buffer: Vec::new(),
            alt_screen_depth: 0,
            tilde_pad_run: 0,
            dedupe_line: Vec::new(),
            dedupe_count: 0,
            last_marked_command: None,
        }
    }

    /// 将缓冲中的 PTY 尾行与文件写入器立即落盘（打开日志弹窗、断开连接前调用）。
    pub fn flush_pending_output(&mut self) {
        self.last_marked_command = None;
        self.flush_pty_buffer(true);
        self.flush_line_compress_state();
        self.flush_file_writer();
    }

    fn flush_file_writer(&mut self) {
        if let Some(ref mut w) = self.writer {
            let _ = w.flush();
            self.last_flush = Instant::now();
        }
    }

    pub fn is_active(&self) -> bool {
        self.settings.enabled && self.writer.is_some() && !self.disabled
    }

    pub fn current_log_path(&self) -> Option<PathBuf> {
        self.current_path.clone()
    }

    pub fn status_label(&self) -> String {
        if !self.settings.enabled || self.disabled {
            return "日志关".to_string();
        }
        "日志".to_string()
    }

    fn write_system_line(&mut self, msg: &str) {
        if self.disabled || !self.settings.enabled {
            return;
        }
        let line = format!(
            "[MistTerm {}] {}\n",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            msg
        );
        self.write_raw(line.as_bytes());
    }

    pub fn write_connected(&mut self) {
        self.write_system_line("← 连接建立");
        if !self.host_line.is_empty() {
            let line = format!(
                "[MistTerm {}] {}\n",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                self.host_line
            );
            self.write_raw(line.as_bytes());
        }
    }

    /// 记录用户提交的命令（带时间戳，便于与后续输出按顺序对照）
    pub fn write_prompt_marker(&mut self, command: &str) {
        let cmd = command.trim();
        if cmd.is_empty() {
            return;
        }
        self.last_marked_command = Some(cmd.to_string());
        self.pty_buffer.clear();
        let line = format!(
            "[MistTerm {}] ❯ {}\n",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            cmd
        );
        self.write_raw(line.as_bytes());
    }

    pub fn append_output(&mut self, data: &[u8]) {
        if self.disabled || !self.settings.enabled || data.is_empty() {
            return;
        }
        self.apply_alt_screen_scan(data);
        if self.alt_screen_depth > 0 {
            return;
        }
        let payload = if self.settings.include_ansi {
            data.to_vec()
        } else {
            strip_ansi_bytes(data)
        };
        self.pty_buffer.extend_from_slice(&payload);
        self.flush_pty_buffer(false);
    }

    /// 识别交替屏幕（vim 全屏），避免把整屏 `~` 与状态栏碎片写入日志。
    fn apply_alt_screen_scan(&mut self, raw: &[u8]) {
        let enters = count_subsequence(raw, b"\x1b[?1049h")
            + count_subsequence(raw, b"\x1b[?47h")
            + count_subsequence(raw, b"\x1b7");
        let leaves = count_subsequence(raw, b"\x1b[?1049l")
            + count_subsequence(raw, b"\x1b[?47l")
            + count_subsequence(raw, b"\x1b8");
        for _ in 0..enters {
            if self.alt_screen_depth == 0 {
                self.write_system_line("[vim/全屏 TUI 开始 — 期间屏幕刷新不写入日志]");
            }
            self.alt_screen_depth = self.alt_screen_depth.saturating_add(1);
        }
        for _ in 0..leaves {
            if self.alt_screen_depth > 0 {
                self.alt_screen_depth -= 1;
                if self.alt_screen_depth == 0 {
                    self.write_system_line("[vim/全屏 TUI 结束]");
                }
            }
        }
    }

    fn flush_tilde_pad_run(&mut self) {
        if self.tilde_pad_run == 0 {
            return;
        }
        if self.tilde_pad_run == 1 {
            // 单条 `~` 多为 shell 行尾碎片，不落盘（避免每条命令后出现孤立 ~）
        } else {
            let line = format!(
                "[MistTerm {}] [略过 vim 空行填充 ×{}]\n",
                Local::now().format("%Y-%m-%d %H:%M:%S"),
                self.tilde_pad_run
            );
            self.write_bytes(line.as_bytes());
        }
        self.tilde_pad_run = 0;
    }

    fn flush_dedupe_run(&mut self) {
        if self.dedupe_count <= 1 {
            self.dedupe_line.clear();
            self.dedupe_count = 0;
            return;
        }
        let line = format!(
            "[MistTerm {}] [略过重复行 ×{}]\n",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            self.dedupe_count
        );
        self.write_bytes(line.as_bytes());
        self.dedupe_line.clear();
        self.dedupe_count = 0;
    }

    /// 落盘一行 PTY 文本（合并连续 `~`、去重完全相同行）。
    fn write_log_line(&mut self, line: Vec<u8>) {
        if line.is_empty() {
            return;
        }
        if self.is_pending_command_echo(&line) {
            return;
        }
        if self.last_marked_command.is_some() {
            let text = String::from_utf8_lossy(&line);
            let t = text.trim();
            if !t.is_empty() && t != "~" && !looks_like_shell_prompt_line(t) {
                self.last_marked_command = None;
            }
        }
        if self.alt_screen_depth == 0 && is_vim_tilde_pad_line(&line) {
            return;
        }
        if is_vim_tilde_pad_line(&line) {
            self.tilde_pad_run += 1;
            return;
        }
        self.flush_tilde_pad_run();
        if !self.dedupe_line.is_empty() && self.dedupe_line == line {
            self.dedupe_count += 1;
            return;
        }
        self.flush_dedupe_run();
        self.dedupe_line = line.clone();
        self.dedupe_count = 1;
        self.write_bytes(&line);
    }

    fn flush_line_compress_state(&mut self) {
        self.flush_tilde_pad_run();
        self.flush_dedupe_run();
    }

    /// 将 [`pty_buffer`] 中完整行写入磁盘；`force` 时刷出末尾无换行残留（断开连接等）。
    fn flush_pty_buffer(&mut self, force: bool) {
        loop {
            // 常见 PTY：`\r\n` 行尾 — 须保留 `\r` 前内容，勿当「覆写行」删掉
            if let Some(i) = self
                .pty_buffer
                .windows(2)
                .position(|w| w == [b'\r', b'\n'])
            {
                let mut line: Vec<u8> = self.pty_buffer.drain(..=i + 1).collect();
                let n = line.len();
                if n >= 2 && line[n - 2] == b'\r' && line[n - 1] == b'\n' {
                    line[n - 2] = b'\n';
                    line.pop();
                }
                self.write_log_line(line);
                continue;
            }
            // 单独 `\r`：进度条同行覆写则丢弃；否则先落盘 `\r` 前内容（shell 重绘提示符时常吃掉无换行尾行）
            if let Some(cr) = self.pty_buffer.iter().position(|&b| b == b'\r') {
                let line_start = self
                    .pty_buffer
                    .iter()
                    .take(cr)
                    .rposition(|&b| b == b'\n')
                    .map(|p| p + 1)
                    .unwrap_or(0);
                let segment: Vec<u8> = self.pty_buffer[line_start..cr].to_vec();
                if !segment.is_empty() && Self::should_preserve_segment_before_cr(&segment) {
                    let mut line = segment;
                    if !line.ends_with(&[b'\n']) {
                        line.push(b'\n');
                    }
                    self.write_log_line(line);
                }
                self.pty_buffer.drain(line_start..=cr);
                continue;
            }
            let Some(nl) = self.pty_buffer.iter().position(|&b| b == b'\n') else {
                break;
            };
            let line: Vec<u8> = self.pty_buffer.drain(..=nl).collect();
            self.write_log_line(line);
        }
        if force && !self.pty_buffer.is_empty() {
            let mut rest = std::mem::take(&mut self.pty_buffer);
            if !rest.ends_with(&[b'\n']) {
                rest.push(b'\n');
            }
            self.write_log_line(rest);
        }
    }

    /// PTY 在 `\r` 前的一段是否值得落盘（排除按键回显、提示符重绘碎片）。
    fn should_preserve_segment_before_cr(seg: &[u8]) -> bool {
        if Self::is_spinner_overwrite_segment(seg) {
            return false;
        }
        let seg_text = String::from_utf8_lossy(seg);
        let t = seg_text.trim();
        if t.is_empty() {
            return false;
        }
        if t.len() < 5 && !t.contains(' ') && !t.contains('/') && !t.contains('\\') {
            return false;
        }
        true
    }

    fn is_pending_command_echo(&self, line: &[u8]) -> bool {
        let Some(ref cmd) = self.last_marked_command else {
            return false;
        };
        let line_text = String::from_utf8_lossy(line);
        let t = line_text.trim();
        if t == "~" {
            return true;
        }
        if t.is_empty() {
            return false;
        }
        if t == cmd.as_str() {
            return true;
        }
        if cmd.starts_with(t) && t.len() < cmd.len() {
            return true;
        }
        t.starts_with(cmd.as_str())
    }

    /// 是否为进度条/旋转器类 `\r` 覆写片段（此类不落盘，避免刷屏）。
    fn is_spinner_overwrite_segment(seg: &[u8]) -> bool {
        let bytes: Vec<u8> = seg.iter().copied().filter(|&b| b != 0x08).collect();
        let s = String::from_utf8_lossy(&bytes);
        let t = s.trim();
        if t.is_empty() {
            return true;
        }
        if t.len() > 80 {
            return false;
        }
        t.chars().all(|c| {
            matches!(
                c,
                '.' | '-' | '\\' | '|' | '/' | ' ' | '█' | '░' | '▏' | '▎' | '▍' | '▌' | '▋' | '▊' | '▉'
            )
        })
    }

    /// 写入已带换行的系统行 / 命令标记（不再追加换行）。
    fn write_raw(&mut self, data: &[u8]) {
        self.write_bytes(data);
    }

    fn write_bytes(&mut self, data: &[u8]) {
        if self.disabled || !self.settings.enabled || data.is_empty() {
            return;
        }
        if !disk_has_space(&self.settings.base_dir) {
            self.disabled = true;
            return;
        }
        let date = Local::now().format("%Y-%m-%d").to_string();
        if self.writer.is_none() || self.current_date != date {
            self.open_for_date(&date);
        }
        if let Some(ref mut w) = self.writer {
            let _ = w.write_all(data);
            if self.last_flush.elapsed() >= Duration::from_millis(500) {
                let _ = w.flush();
                self.last_flush = Instant::now();
            }
        }
    }

    fn open_for_date(&mut self, date: &str) {
        self.current_date = date.to_string();
        self.writer = None;
        self.current_path = None;
        let dir = self
            .settings
            .base_dir
            .join(sanitize_filename(&self.session_id));
        if fs::create_dir_all(&dir).is_err() {
            self.disabled = true;
            return;
        }
        let path = next_log_path(&dir, date, self.settings.max_file_bytes);
        if let Ok(f) = OpenOptions::new().create(true).append(true).open(&path) {
            let mut w = BufWriter::new(f);
            if path.metadata().map(|m| m.len()).unwrap_or(0) == 0 {
                let _ = writeln!(
                    &mut w,
                    "# MistTerm session log — {} ({})",
                    self.session_name, self.session_id
                );
            }
            self.current_path = Some(path);
            self.writer = Some(w);
        } else {
            self.disabled = true;
        }
    }

    pub fn stop_log(&mut self) {
        self.flush_pty_buffer(true);
        self.flush_line_compress_state();
        self.alt_screen_depth = 0;
        self.write_system_line("← 连接断开");
        if let Some(ref mut w) = self.writer {
            let _ = w.flush();
        }
        self.writer = None;
        self.current_path = None;
        self.pty_buffer.clear();
    }

    pub fn close(&mut self) {
        self.stop_log();
    }
}

fn next_log_path(dir: &Path, date: &str, max_bytes: u64) -> PathBuf {
    let base = dir.join(format!("{}.log", date));
    if base.exists() {
        if let Ok(meta) = base.metadata() {
            if meta.len() < max_bytes {
                return base;
            }
        }
    } else {
        return base;
    }
    let mut n = 1u32;
    loop {
        let p = dir.join(format!("{}.{}.log", date, n));
        if !p.exists() {
            return p;
        }
        if let Ok(meta) = p.metadata() {
            if meta.len() < max_bytes {
                return p;
            }
        }
        n += 1;
        if n > 99 {
            return p;
        }
    }
}

fn disk_has_space(base: &Path) -> bool {
    let _ = fs::create_dir_all(base);
    // 无跨平台可靠 API：尽力写入；空间不足时 open/write 会失败并 disabled
    true
}

fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn strip_ansi_bytes(data: &[u8]) -> Vec<u8> {
    let s = String::from_utf8_lossy(data);
    log_text_for_display(&s).into_bytes()
}

fn count_subsequence(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

fn looks_like_shell_prompt_line(text: &str) -> bool {
    let t = text.trim();
    t.contains("$ ") || t.contains("# ") || t.contains("> ")
}

/// vim 空缓冲区行，常为单独的 `~` 或空白。
fn is_vim_tilde_pad_line(line: &[u8]) -> bool {
    let body = line.strip_suffix(b"\n").unwrap_or(line);
    let Ok(s) = std::str::from_utf8(body) else {
        return false;
    };
    let t = s.trim();
    t.is_empty() || t == "~"
}

/// 列出某会话目录下的日志文件（新→旧）
pub fn list_session_log_files(base: &Path, session_id: &str) -> Vec<PathBuf> {
    let dir = base.join(sanitize_filename(session_id));
    let Ok(read) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = read
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().contains(".log"))
        .collect();
    paths.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    paths
}

pub fn read_log_tail(path: &Path, max_bytes: usize) -> std::io::Result<String> {
    let data = fs::read(path)?;
    if data.len() <= max_bytes {
        return Ok(String::from_utf8_lossy(&data).into_owned());
    }
    Ok(String::from_utf8_lossy(&data[data.len() - max_bytes..]).into_owned())
}

/// 日志弹窗展示用：剥掉 ANSI/OSC 等控制序列
pub fn log_text_for_display(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek().copied() {
                Some('[') => {
                    chars.next();
                    while let Some(ch) = chars.next() {
                        if ch.is_ascii_alphabetic() || ch == '~' {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    while let Some(ch) = chars.next() {
                        if ch == '\x07' {
                            break;
                        }
                        if ch == '\x1b' {
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                                break;
                            }
                        }
                    }
                }
                Some('P') | Some('^') | Some('_') => {
                    chars.next();
                    while let Some(ch) = chars.next() {
                        if ch == '\x1b' {
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                                break;
                            }
                        }
                    }
                }
                Some('(') | Some(')') => {
                    chars.next();
                    if chars.peek().is_some() {
                        chars.next();
                    }
                }
                _ => {}
            }
            continue;
        }
        if c.is_control() && c != '\n' && c != '\r' && c != '\t' {
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_text_for_display_strips_csi_color() {
        let raw = "ok\x1b[39;49m\x1b[0m\n";
        assert_eq!(log_text_for_display(raw), "ok\n");
    }

    #[test]
    fn alt_screen_suppresses_vim_flood() {
        let dir = std::env::temp_dir().join(format!(
            "mistterm_log_alt_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let settings = SessionLogSettings {
            enabled: true,
            base_dir: dir.clone(),
            ..SessionLogSettings::default()
        };
        let mut w = SessionLogWriter::new(
            "sess".into(),
            "test".into(),
            String::new(),
            settings,
        );
        w.append_output(b"\x1b[?1049h");
        w.append_output(b"~\n~\n~\n");
        w.append_output(b"\x1b[?1049l");
        w.append_output(b"\"a.txt\" written\n");
        let path = w.current_log_path().expect("log path");
        w.stop_log();
        let body = fs::read_to_string(&path).unwrap();
        assert!(
            body.contains("全屏 TUI 开始") && body.contains("全屏 TUI 结束"),
            "{body}"
        );
        assert!(
            !body.contains("~\n~\n") || body.contains("略过 vim 空行"),
            "tilde flood should be collapsed: {body}"
        );
        assert!(body.contains("a.txt"), "{body}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_output_preserves_crlf_lines() {
        let dir = std::env::temp_dir().join(format!(
            "mistterm_log_crlf_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let settings = SessionLogSettings {
            enabled: true,
            base_dir: dir.clone(),
            ..SessionLogSettings::default()
        };
        let mut w = SessionLogWriter::new(
            "sess".into(),
            "test".into(),
            String::new(),
            settings,
        );
        w.append_output(b"hosts\r\nMist.exe\r\n");
        let path = w.current_log_path().expect("log path");
        w.stop_log();
        let body = fs::read_to_string(&path).unwrap();
        assert!(
            body.contains("hosts\n") && body.contains("Mist.exe\n"),
            "CRLF lines must be logged:\n{body}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_output_buffers_keystrokes_until_newline() {
        let dir = std::env::temp_dir().join(format!(
            "mistterm_log_buf_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let settings = SessionLogSettings {
            enabled: true,
            base_dir: dir.clone(),
            ..SessionLogSettings::default()
        };
        let mut w = SessionLogWriter::new(
            "sess".into(),
            "test".into(),
            String::new(),
            settings,
        );
        w.append_output(b"l");
        w.append_output(b"s");
        w.append_output(b"\n");
        let path = w.current_log_path().expect("log path");
        w.stop_log();
        let body = fs::read_to_string(&path).unwrap();
        assert!(
            body.contains("ls\n"),
            "expected buffered keystrokes on one line, got:\n{body}"
        );
        assert!(
            !body.contains("l\ns\n"),
            "must not split single chars into separate lines:\n{body}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn lone_tilde_after_command_not_logged() {
        let dir = std::env::temp_dir().join(format!(
            "mistterm_log_tilde_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let settings = SessionLogSettings {
            enabled: true,
            base_dir: dir.clone(),
            ..SessionLogSettings::default()
        };
        let mut w = SessionLogWriter::new(
            "sess".into(),
            "test".into(),
            String::new(),
            settings,
        );
        w.write_prompt_marker("pwd");
        w.append_output(b"[root@host ~]# \r");
        w.append_output(b"~\n");
        w.append_output(b"/root\n");
        w.flush_pending_output();
        let path = w.current_log_path().expect("log path");
        let body = fs::read_to_string(&path).unwrap();
        assert!(
            body.contains("/root"),
            "expected output:\n{body}"
        );
        assert!(
            !body.lines().any(|l| l.trim() == "~"),
            "must not log stray tilde line:\n{body}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn command_echo_not_split_per_character() {
        let dir = std::env::temp_dir().join(format!(
            "mistterm_log_echo_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let settings = SessionLogSettings {
            enabled: true,
            base_dir: dir.clone(),
            ..SessionLogSettings::default()
        };
        let mut w = SessionLogWriter::new(
            "sess".into(),
            "test".into(),
            String::new(),
            settings,
        );
        w.append_output(b"p");
        w.append_output(b"w\r");
        w.append_output(b"d\r");
        w.write_prompt_marker("pwd");
        w.append_output(b"/root\n");
        w.flush_pending_output();
        let path = w.current_log_path().expect("log path");
        let body = fs::read_to_string(&path).unwrap();
        assert!(
            body.contains("❯ pwd"),
            "command marker expected:\n{body}"
        );
        assert!(
            body.contains("/root"),
            "command output expected:\n{body}"
        );
        assert!(
            !body.contains("\np\n") && !body.contains("\npw\n"),
            "must not log keystroke echo line-by-line:\n{body}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn lone_cr_preserves_output_before_prompt_redraw() {
        let dir = std::env::temp_dir().join(format!(
            "mistterm_log_cr_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let settings = SessionLogSettings {
            enabled: true,
            base_dir: dir.clone(),
            ..SessionLogSettings::default()
        };
        let mut w = SessionLogWriter::new(
            "sess".into(),
            "test".into(),
            String::new(),
            settings,
        );
        w.append_output(b"hello world");
        w.append_output(b"\r");
        let path = w.current_log_path().expect("log path");
        w.flush_pending_output();
        let body = fs::read_to_string(&path).unwrap();
        assert!(
            body.contains("hello world"),
            "output before lone \\r must be logged:\n{body}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn flush_pending_writes_trailing_line_without_newline() {
        let dir = std::env::temp_dir().join(format!(
            "mistterm_log_tail_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let settings = SessionLogSettings {
            enabled: true,
            base_dir: dir.clone(),
            ..SessionLogSettings::default()
        };
        let mut w = SessionLogWriter::new(
            "sess".into(),
            "test".into(),
            String::new(),
            settings,
        );
        w.append_output(b"last-command-output");
        w.flush_pending_output();
        let path = w.current_log_path().expect("log path");
        let body = fs::read_to_string(&path).unwrap();
        assert!(
            body.contains("last-command-output"),
            "trailing line without \\n must flush:\n{body}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_prompt_marker_includes_timestamp_and_glyph() {
        let dir = std::env::temp_dir().join(format!(
            "mistterm_log_test_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let settings = SessionLogSettings {
            enabled: true,
            base_dir: dir.clone(),
            ..SessionLogSettings::default()
        };
        let mut w = SessionLogWriter::new(
            "sess".into(),
            "test".into(),
            String::new(),
            settings,
        );
        w.write_prompt_marker("ls -la");
        w.stop_log();
        let files = list_session_log_files(&dir, "sess");
        let body = fs::read_to_string(files.first().expect("log file")).unwrap();
        assert!(
            body.contains("[MistTerm ") && body.contains("] ❯ ls -la"),
            "expected timestamped command line, got:\n{body}"
        );
    }
}
