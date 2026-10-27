//! `mist ssh --log-session`：交互会话的命令级 JSONL（#164）。
//!
//! 复用 [`crate::core::interactive_exec`] 的回车草稿和 [`crate::core::exec_history::ExecLogRecord`]。
//! 不另做审计策略。没传 `--log-session` 时调用方不要构造本类型，因此不会建文件、也不会扫输出。
//!
//! 一条记录的边界是回车提交：下一条回车、提示符 / OSC 133 `D`，或会话结束，都会把上一条封口。
//! `target` 与 #156 GUI 相同，是主机；端口不是 22 时写成 `host:port`。

use std::path::{Path, PathBuf};

use crate::core::exec_history::{append_record_at, ExecLogRecord};
use crate::core::interactive_exec::{interactive_exec_record, InteractiveExecDraft};

/// JSONL `tags` 里区分 CLI 交互会话。GUI 记录没有这一项。
pub const CLI_SSH_TAG: &str = "cli_ssh";

/// `mist ssh --help` 与 CLI 文档里的默认路径形态。
pub const DEFAULT_PATH_PATTERN: &str = "~/.mist/logs/ssh-<会话>-<UTC时间>.jsonl";

/// 一次 `mist ssh --log-session` 的写入器。
pub struct CliSshSessionLog {
    path: PathBuf,
    session_id: String,
    target: String,
    open: Option<InteractiveExecDraft>,
}

impl CliSshSessionLog {
    pub fn new(path: PathBuf, session_id: impl Into<String>, target: impl Into<String>) -> Self {
        Self {
            path,
            session_id: session_id.into(),
            target: target.into(),
            open: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 回车提交一条命令。上一条若还开着，先按已收集的输出落盘。
    ///
    /// `blocked` / `cancelled` 没有后续输出，立刻写一行。
    pub fn submit(&mut self, command: &str, outcome: &str) {
        let command = command.trim();
        if command.is_empty() {
            return;
        }
        self.finish_open();
        if is_terminal_outcome(outcome) {
            let rec = interactive_exec_record(
                &self.session_id,
                &self.target,
                command,
                outcome,
                None,
                "",
                0,
            );
            self.write(rec);
            return;
        }
        self.open = Some(InteractiveExecDraft::start(
            &self.session_id,
            &self.target,
            command,
            outcome,
        ));
    }

    /// 喂远端输出。草稿在提示符或 OSC 133 `D` 处结束并落盘。
    pub fn feed_output(&mut self, data: &[u8]) {
        if data.is_empty() || self.open.is_none() {
            return;
        }
        let done = self
            .open
            .as_mut()
            .map(|draft| draft.feed(data))
            .unwrap_or(false);
        if done {
            self.finish_open();
        }
    }

    /// 会话结束：把还没封口的命令写出去。
    pub fn finish(&mut self) {
        self.finish_open();
    }

    fn finish_open(&mut self) {
        if let Some(draft) = self.open.take() {
            self.write(draft.into_record());
        }
    }

    fn write(&self, mut rec: ExecLogRecord) {
        if !rec.tags.iter().any(|tag| tag == CLI_SSH_TAG) {
            rec.tags.push(CLI_SSH_TAG.to_string());
        }
        append_record_at(&self.path, &rec);
    }
}

impl Drop for CliSshSessionLog {
    fn drop(&mut self) {
        self.finish();
    }
}

fn is_terminal_outcome(outcome: &str) -> bool {
    matches!(outcome, "blocked" | "cancelled")
}

/// 与 GUI `session_exec_endpoint` 相同：22 端口只写主机，否则 `host:port`。
pub fn exec_target_label(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("{host}:{port}")
    }
}

/// `flag` 为 `None`：没加 `--log-session`，不要写文件。
/// 空字符串：加了选项但没给路径，用 [`default_ssh_session_log_path`]。
pub fn resolve_ssh_session_log_path(session_name: &str, flag: Option<&str>) -> Option<PathBuf> {
    match flag.map(str::trim) {
        None => None,
        Some("") => Some(default_ssh_session_log_path(session_name)),
        Some(path) => Some(expand_user_path(path)),
    }
}

pub fn default_ssh_session_log_path(session_name: &str) -> PathBuf {
    let dir = dirs::home_dir()
        .map(|home| home.join(".mist").join("logs"))
        .unwrap_or_else(|| PathBuf::from(".mist").join("logs"));
    let stem = sanitize_session_stem(session_name);
    let now = chrono::Utc::now();
    let stamp = now.format("%Y%m%dT%H%M%SZ");
    let path = dir.join(format!("ssh-{stem}-{stamp}.jsonl"));
    if path.exists() {
        dir.join(format!(
            "ssh-{stem}-{stamp}-{}.jsonl",
            now.timestamp_millis()
        ))
    } else {
        path
    }
}

fn expand_user_path(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    let rest = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\"));
    if let Some(rest) = rest {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

fn sanitize_session_stem(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if out.chars().count() >= 64 {
            break;
        }
        if c.is_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "session".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::exec_history::{read_records_at, OUTPUT_SUMMARY_MAX_CHARS};

    fn temp_path(name: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "mist-ssh-log-{name}-{}-{n}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn absent_flag_does_not_choose_or_create_a_file() {
        assert!(resolve_ssh_session_log_path("web-01", None).is_none());
        let custom = temp_path("absent");
        assert!(!custom.exists());
    }

    #[test]
    fn custom_path_is_used_verbatim() {
        let custom = temp_path("custom-resolve");
        let got = resolve_ssh_session_log_path("web-01", Some(custom.to_str().unwrap())).unwrap();
        assert_eq!(got, custom);
        assert!(!custom.exists(), "解析路径不应创建文件");
    }

    #[test]
    fn tilde_and_blank_flag() {
        let home = dirs::home_dir().expect("home");
        let got = resolve_ssh_session_log_path("web", Some("~/logs/a.jsonl")).unwrap();
        assert_eq!(got, home.join("logs").join("a.jsonl"));

        let blank = resolve_ssh_session_log_path("web-01", Some("  ")).unwrap();
        let text = blank.to_string_lossy().replace('\\', "/");
        assert!(text.contains("/.mist/logs/ssh-web-01-"), "{text}");
        assert!(text.ends_with(".jsonl"), "{text}");
        assert!(!blank.exists());
    }

    #[test]
    fn default_path_matches_help_pattern() {
        let path = default_ssh_session_log_path("root@10.0.0.1:2202");
        let text = path.to_string_lossy().replace('\\', "/");
        assert!(
            text.contains("/.mist/logs/ssh-root-10-0-0-1-2202-"),
            "{text}"
        );
        assert!(text.ends_with(".jsonl"), "{text}");
        assert_eq!(
            DEFAULT_PATH_PATTERN,
            "~/.mist/logs/ssh-<会话>-<UTC时间>.jsonl"
        );
        assert_eq!(exec_target_label("10.0.0.1", 22), "10.0.0.1");
        assert_eq!(exec_target_label("10.0.0.1", 2202), "10.0.0.1:2202");
    }

    #[test]
    fn two_commands_write_two_rows_with_exec_history_fields() {
        let path = temp_path("two");
        let mut log = CliSshSessionLog::new(path.clone(), "sess-9", "10.0.0.8");
        log.submit("echo one", "allowed");
        log.feed_output(b"echo one\r\none\r\nuser@host:~$ ");
        log.submit("echo two", "allowed");
        log.feed_output(b"echo two\r\ntwo\r\nuser@host:~$ ");
        log.finish();

        let rows = read_records_at(&path).expect("read");
        assert!(
            rows.len() >= 2,
            "两条回车至少两行 JSONL，实际 {}",
            rows.len()
        );
        assert_eq!(rows[0].target, "10.0.0.8");
        assert_eq!(rows[0].command, "echo one");
        assert!(rows[0].timestamp.contains('T'), "{}", rows[0].timestamp);
        assert!(rows[0].output_summary.contains("one"));
        assert!(!rows[0].output_summary.contains("user@host"));
        assert_eq!(rows[0].session_id, "sess-9");
        assert!(rows[0].tags.iter().any(|tag| tag == "interactive_pty"));
        assert!(rows[0].tags.iter().any(|tag| tag == "allowed"));
        assert!(rows[0].tags.iter().any(|tag| tag == CLI_SSH_TAG));
        assert_eq!(rows[1].command, "echo two");
        assert_eq!(rows[1].target, "10.0.0.8");
        assert!(rows[1].timestamp.contains('T'));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn exit_flushes_commands_without_a_prompt() {
        let path = temp_path("flush");
        let mut log = CliSshSessionLog::new(path.clone(), "s", "web.example");
        log.submit("echo one", "allowed");
        log.feed_output(b"one\r\n");
        log.submit("echo two", "allowed");
        log.feed_output(b"two\r\n");
        log.finish();

        let rows = read_records_at(&path).expect("read");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].command, "echo one");
        assert!(rows[0].output_summary.contains("one"));
        assert!(rows[0].exit_code.is_none());
        assert_eq!(rows[1].command, "echo two");
        assert!(rows[1].output_summary.contains("two"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn osc_133_sets_exit_code_and_output_is_bounded() {
        let path = temp_path("osc");
        let mut log = CliSshSessionLog::new(path.clone(), "s", "h");
        log.submit("false", "allowed");
        log.feed_output(b"\x1b]133;D;3\x07\r\nuser@h:~$ ");
        log.finish();
        let rows = read_records_at(&path).expect("read");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].exit_code, Some(3));
        assert!(!rows[0].ok);
        let _ = std::fs::remove_file(&path);

        let path = temp_path("bound");
        let mut log = CliSshSessionLog::new(path.clone(), "s", "h");
        log.submit("cat big", "allowed");
        let mut chunk = b"cat big\r\n".to_vec();
        let line = b"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\r\n";
        for _ in 0..200 {
            chunk.extend_from_slice(line);
        }
        chunk.extend_from_slice(b"user@h:~$ ");
        log.feed_output(&chunk);
        log.finish();
        let rows = read_records_at(&path).expect("read");
        assert_eq!(rows.len(), 1);
        assert!(
            rows[0].output_summary.contains("[truncated]"),
            "{}",
            rows[0].output_summary.len()
        );
        assert!(rows[0].output_summary.chars().count() < OUTPUT_SUMMARY_MAX_CHARS + 80);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn blocked_and_cancelled_are_immediate_rows() {
        let path = temp_path("deny");
        let mut log = CliSshSessionLog::new(path.clone(), "s", "h");
        log.submit("rm -rf /", "blocked");
        log.submit("rm -rf /tmp", "cancelled");
        log.finish();
        let rows = read_records_at(&path).expect("read");
        assert_eq!(rows.len(), 2);
        assert!(rows[0].tags.iter().any(|tag| tag == "blocked"));
        assert!(rows[0].exit_code.is_none());
        assert!(!rows[0].ok);
        assert!(rows[1].tags.iter().any(|tag| tag == "cancelled"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn drop_flushes_the_open_command() {
        let path = temp_path("drop");
        {
            let mut log = CliSshSessionLog::new(path.clone(), "s", "h");
            log.submit("uptime", "allowed");
            log.feed_output(b"uptime\r\n 12:00 up\r\n");
        }
        let rows = read_records_at(&path).expect("read");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].command, "uptime");
        assert!(rows[0].output_summary.contains("12:00"));
        let _ = std::fs::remove_file(&path);
    }
}
