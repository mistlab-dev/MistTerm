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
        "📝 日志".to_string()
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

    pub fn write_prompt_marker(&mut self, command: &str) {
        if command.trim().is_empty() {
            return;
        }
        let line = format!("❯ {}\n", command.trim());
        self.write_raw(line.as_bytes());
    }

    pub fn append_output(&mut self, data: &[u8]) {
        if self.disabled || !self.settings.enabled || data.is_empty() {
            return;
        }
        let payload = if self.settings.include_ansi {
            data.to_vec()
        } else {
            strip_ansi_bytes(data)
        };
        self.write_raw(&payload);
    }

    fn write_raw(&mut self, data: &[u8]) {
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
            if !data.ends_with(b"\n") {
                let _ = w.write_all(b"\n");
            }
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
        self.write_system_line("← 连接断开");
        if let Some(ref mut w) = self.writer {
            let _ = w.flush();
        }
        self.writer = None;
        self.current_path = None;
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
}
