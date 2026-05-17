//! 终端输出会话日志（按标签/会话落盘）

use chrono::Local;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// 全局日志偏好
#[derive(Debug, Clone)]
pub struct SessionLogSettings {
    pub enabled: bool,
    pub base_dir: PathBuf,
}

impl Default for SessionLogSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            base_dir: default_log_base_dir(),
        }
    }
}

pub fn default_log_base_dir() -> PathBuf {
    let mut p = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("mistterm");
    p.push("session_logs");
    p
}

/// 单个活动标签的日志写入器
pub struct SessionLogWriter {
    session_id: String,
    session_name: String,
    settings: SessionLogSettings,
    file: Option<std::fs::File>,
    current_date: String,
}

impl SessionLogWriter {
    pub fn new(
        session_id: String,
        session_name: String,
        settings: SessionLogSettings,
    ) -> Self {
        Self {
            session_id,
            session_name,
            settings,
            file: None,
            current_date: String::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.settings.enabled && self.file.is_some()
    }

    pub fn current_log_path(&self) -> Option<PathBuf> {
        if !self.settings.enabled {
            return None;
        }
        let date = Local::now().format("%Y-%m-%d").to_string();
        Some(
            self.settings
                .base_dir
                .join(sanitize_filename(&self.session_id))
                .join(format!("{}.log", date)),
        )
    }

    pub fn status_label(&self) -> String {
        if !self.settings.enabled {
            return "日志关".to_string();
        }
        "📝 日志".to_string()
    }

    pub fn append_output(&mut self, data: &[u8]) {
        if !self.settings.enabled || data.is_empty() {
            return;
        }
        let date = Local::now().format("%Y-%m-%d").to_string();
        if self.file.is_none() || self.current_date != date {
            self.open_for_date(&date);
        }
        if let Some(ref mut f) = self.file {
            let _ = f.write_all(b"[");
            let _ = write!(f, "{}", Local::now().format("%H:%M:%S"));
            let _ = f.write_all(b"] ");
            let _ = f.write_all(data);
            if !data.ends_with(b"\n") {
                let _ = f.write_all(b"\n");
            }
        }
    }

    fn open_for_date(&mut self, date: &str) {
        self.current_date = date.to_string();
        self.file = None;
        let dir = self
            .settings
            .base_dir
            .join(sanitize_filename(&self.session_id));
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = dir.join(format!("{}.log", date));
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(
                &mut f,
                "# MistTerm session log — {} ({}) — {}",
                self.session_name,
                self.session_id,
                Local::now().format("%Y-%m-%d %H:%M:%S")
            );
            self.file = Some(f);
        }
    }

    pub fn close(&mut self) {
        self.file = None;
    }
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

/// 列出某会话目录下的日志文件（新→旧）
pub fn list_session_log_files(base: &Path, session_id: &str) -> Vec<PathBuf> {
    let dir = base.join(sanitize_filename(session_id));
    let Ok(read) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = read
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("log"))
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

/// 日志弹窗展示用：剥掉 ANSI/OSC 等控制序列，避免 `[39;49m` 一类「乱码」
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
