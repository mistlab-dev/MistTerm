//! 交互式 PTY 命令级捕获（#156）。
//!
//! 回车提交的一行走既有本地审计（`on_before_send` / `CmdAuditEngine`），
//! 并往 [`crate::core::exec_history`] 的 JSONL 追加一条。这里不另做策略判定。
//!
//! 边界是实用的：回车 / 下一行 shell 提示符，或 shell integration 的 OSC 133 `D`。
//! 不解析完整 shell 语法。全屏程序、口令行、多行粘贴由调用方跳过。

use std::time::Instant;

use super::exec_history::{ExecLogRecord, OUTPUT_SUMMARY_MAX_CHARS};

/// JSONL `tags` 里标识交互式回车提交。
pub const INTERACTIVE_PTY_TAG: &str = "interactive_pty";

/// 偏好开，且本地策略或该主机的服务器审计至少一侧生效，才捕获。
///
/// 未登录团队时两侧都是 false，手输不拦截、不写 JSONL。
pub fn interactive_capture_active(
    preference_on: bool,
    local_policy_active: bool,
    server_audit_for_host: bool,
) -> bool {
    preference_on && (local_policy_active || server_audit_for_host)
}

/// 手敲回车是否先扣住 `\r`。偏好关时与未启用审计一样，回车直接发出。
pub fn interactive_typed_enter_gate(
    preference_on: bool,
    module_enabled: bool,
    engine_active: bool,
) -> bool {
    preference_on && module_enabled && engine_active
}

/// 本地策略是否真的会拦（模块开且引擎已有启用的团队策略）。
pub fn local_policy_active(module_enabled: bool, engine_active: bool) -> bool {
    module_enabled && engine_active
}

/// 不经过 PTY 输出（拦截 / 取消）时直接造一条记录。
pub fn interactive_exec_record(
    session_id: &str,
    target: &str,
    command: &str,
    outcome: &str,
    exit_code: Option<i32>,
    output: &str,
    duration_ms: u64,
) -> ExecLogRecord {
    let denied = is_denied_outcome(outcome);
    let code = if denied { None } else { exit_code };
    let ok = if denied {
        false
    } else {
        code.map(|c| c == 0).unwrap_or(true)
    };
    let mut rec = ExecLogRecord::new(
        session_id,
        target,
        command.trim(),
        code,
        ok,
        duration_ms,
        output,
        INTERACTIVE_PTY_TAG,
    );
    if !outcome.is_empty() {
        rec.tags.push(outcome.to_string());
    }
    rec
}

/// 把已经落入 pending 的记录改成服务器事后结论（例如先放行、随即被 Agent 拦住）。
pub fn apply_interactive_outcome(rec: &mut ExecLogRecord, outcome: &str) {
    rec.tags.retain(|tag| !is_outcome_tag(tag));
    if !rec.tags.iter().any(|tag| tag == INTERACTIVE_PTY_TAG) {
        rec.tags.push(INTERACTIVE_PTY_TAG.to_string());
    }
    if !outcome.is_empty() {
        rec.tags.push(outcome.to_string());
    }
    if is_denied_outcome(outcome) {
        rec.ok = false;
        rec.exit_code = None;
    }
}

fn is_denied_outcome(outcome: &str) -> bool {
    matches!(outcome, "blocked" | "cancelled")
}

fn is_outcome_tag(tag: &str) -> bool {
    matches!(
        tag,
        "allowed" | "alert" | "confirmed" | "blocked" | "cancelled"
    )
}

/// 一行是不是 shell 提示符。用来给「一条命令结束」划界，不追求认全所有 PS1。
pub fn is_shell_prompt_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() || t.chars().count() > 240 {
        return false;
    }
    if matches!(t, "$" | "#" | "%" | ">") {
        return true;
    }
    if t.starts_with("PS ") && t.ends_with('>') {
        return true;
    }
    let Some(last) = t.chars().last() else {
        return false;
    };
    if !matches!(last, '$' | '#' | '%' | '>') {
        return false;
    }
    let body = t.trim_end_matches(|c: char| matches!(c, '$' | '#' | '%' | '>' | ' '));
    if body.is_empty() {
        return true;
    }
    body.contains('@')
        || body.contains('~')
        || body.contains('/')
        || body.contains('\\')
        || body.contains(':')
}

/// 一条已提交、还在等输出 / 提示符的命令。
pub struct InteractiveExecDraft {
    session_id: String,
    target: String,
    command: String,
    started: Instant,
    timestamp: String,
    outcome: String,
    output: String,
    output_capped: bool,
    exit_code: Option<i32>,
    scanner: PlainScanner,
    line: String,
    dropped_echo: bool,
    saw_newline: bool,
    started_output: bool,
    done: bool,
}

impl InteractiveExecDraft {
    pub fn start(session_id: &str, target: &str, command: &str, outcome: &str) -> Self {
        let outcome = if outcome.is_empty() {
            "allowed".to_string()
        } else {
            outcome.to_string()
        };
        Self {
            session_id: session_id.to_string(),
            target: target.to_string(),
            command: command.trim().to_string(),
            started: Instant::now(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            outcome,
            output: String::new(),
            output_capped: false,
            exit_code: None,
            scanner: PlainScanner::default(),
            line: String::new(),
            dropped_echo: false,
            saw_newline: false,
            started_output: false,
            done: false,
        }
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn set_outcome(&mut self, outcome: &str) {
        if !outcome.is_empty() {
            self.outcome = outcome.to_string();
        }
    }

    /// 喂一块 PTY 输出。返回 true 表示这条命令可以落盘了。
    pub fn feed(&mut self, data: &[u8]) -> bool {
        if self.done || data.is_empty() {
            return self.done;
        }
        let events = self.scanner.feed(data);
        for ev in events {
            match ev {
                ScanEvent::Text(text) => {
                    if !self.done {
                        self.push_text(&text);
                    }
                }
                ScanEvent::CommandFinished(code) => {
                    if let Some(code) = code {
                        self.exit_code = Some(code);
                    }
                    self.done = true;
                }
            }
        }
        self.done
    }

    pub fn into_record(self) -> ExecLogRecord {
        let duration_ms = self.started.elapsed().as_millis() as u64;
        let mut rec = interactive_exec_record(
            &self.session_id,
            &self.target,
            &self.command,
            &self.outcome,
            self.exit_code,
            &self.output,
            duration_ms,
        );
        rec.timestamp = self.timestamp;
        rec
    }

    fn push_text(&mut self, text: &str) {
        for ch in text.chars() {
            if self.done {
                return;
            }
            if ch == '\r' {
                continue;
            }
            if ch == '\n' {
                self.finish_line();
                continue;
            }
            if self.line.chars().count() < 512 {
                self.line.push(ch);
            }
        }
        self.consider_prompt_tail();
    }

    fn finish_line(&mut self) {
        let line = std::mem::take(&mut self.line);
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if self.started_output {
                self.push_output_line("");
            }
            self.saw_newline = true;
            return;
        }
        if !self.dropped_echo && trimmed == self.command {
            self.dropped_echo = true;
            self.saw_newline = true;
            return;
        }
        self.saw_newline = true;
        if is_shell_prompt_line(trimmed) {
            self.done = true;
            return;
        }
        self.started_output = true;
        self.push_output_line(trimmed);
    }

    fn consider_prompt_tail(&mut self) {
        if self.done || !self.saw_newline {
            return;
        }
        if is_shell_prompt_line(self.line.trim()) {
            self.done = true;
            self.line.clear();
        }
    }

    fn push_output_line(&mut self, line: &str) {
        if self.output_capped {
            return;
        }
        if !self.output.is_empty() {
            self.output.push('\n');
        }
        self.output.push_str(line);
        if self.output.chars().count() > OUTPUT_SUMMARY_MAX_CHARS {
            self.output_capped = true;
            // 只留一小段溢出，最终由 ExecLogRecord 截断一次并带上后缀。
            if self.output.chars().count() > OUTPUT_SUMMARY_MAX_CHARS + 64 {
                self.output = self
                    .output
                    .chars()
                    .take(OUTPUT_SUMMARY_MAX_CHARS + 64)
                    .collect();
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scan {
    Ground,
    Esc,
    Csi,
    Osc,
    OscEsc,
}

enum ScanEvent {
    Text(String),
    CommandFinished(Option<i32>),
}

#[derive(Default)]
struct PlainScanner {
    state: Scan,
    osc: Vec<u8>,
    utf8_hold: Vec<u8>,
}

impl Default for Scan {
    fn default() -> Self {
        Self::Ground
    }
}

impl PlainScanner {
    fn feed(&mut self, data: &[u8]) -> Vec<ScanEvent> {
        let mut out = Vec::new();
        let mut plain: Vec<u8> = Vec::new();
        for &b in data {
            match self.state {
                Scan::Ground => match b {
                    0x1b => self.state = Scan::Esc,
                    _ => plain.push(b),
                },
                Scan::Esc => match b {
                    b'[' => self.state = Scan::Csi,
                    b']' => {
                        self.state = Scan::Osc;
                        self.osc.clear();
                    }
                    _ => self.state = Scan::Ground,
                },
                Scan::Csi => {
                    if (0x40..=0x7e).contains(&b) {
                        self.state = Scan::Ground;
                    }
                }
                Scan::Osc => match b {
                    0x07 => {
                        self.flush_text(&mut plain, &mut out);
                        if let Some(ev) = self.finish_osc() {
                            out.push(ev);
                        }
                        self.state = Scan::Ground;
                    }
                    0x1b => self.state = Scan::OscEsc,
                    _ => {
                        if self.osc.len() < 96 {
                            self.osc.push(b);
                        }
                    }
                },
                Scan::OscEsc => {
                    self.state = Scan::Ground;
                    if b == b'\\' {
                        self.flush_text(&mut plain, &mut out);
                        if let Some(ev) = self.finish_osc() {
                            out.push(ev);
                        }
                    } else {
                        self.osc.clear();
                        if b == 0x1b {
                            self.state = Scan::Esc;
                        } else {
                            plain.push(b);
                        }
                    }
                }
            }
        }
        self.flush_text(&mut plain, &mut out);
        out
    }

    fn flush_text(&mut self, plain: &mut Vec<u8>, out: &mut Vec<ScanEvent>) {
        if plain.is_empty() && self.utf8_hold.is_empty() {
            return;
        }
        let mut buf = std::mem::take(&mut self.utf8_hold);
        buf.append(plain);
        let (text, rest) = split_utf8(&buf);
        self.utf8_hold = rest;
        if !text.is_empty() {
            out.push(ScanEvent::Text(text));
        }
    }

    fn finish_osc(&mut self) -> Option<ScanEvent> {
        let osc = std::mem::take(&mut self.osc);
        parse_command_finished(&osc).map(ScanEvent::CommandFinished)
    }
}

fn split_utf8(buf: &[u8]) -> (String, Vec<u8>) {
    match std::str::from_utf8(buf) {
        Ok(text) => (text.to_string(), Vec::new()),
        Err(err) => {
            let valid = err.valid_up_to();
            if err.error_len().is_none() {
                let (head, tail) = buf.split_at(valid);
                (String::from_utf8_lossy(head).into_owned(), tail.to_vec())
            } else {
                (String::from_utf8_lossy(buf).into_owned(), Vec::new())
            }
        }
    }
}

/// `Some(code)`：OSC 133 D 带退出码。`Some(None)`：有 D 但没有数字。`None`：不是这条序列。
fn parse_command_finished(osc: &[u8]) -> Option<Option<i32>> {
    let text = std::str::from_utf8(osc).ok()?.trim();
    let rest = text.strip_prefix("133;D")?;
    if rest.is_empty() {
        return Some(None);
    }
    let rest = rest.strip_prefix(';')?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return Some(None);
    }
    Some(digits.parse::<i32>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cmd_audit::{
        CmdAuditAction, CmdAuditEngine, CmdAuditPolicy, CmdAuditSyncPayload,
    };
    use crate::core::exec_history::{append_record_at, read_records_at};

    fn policy_engine(dangerous: CmdAuditAction) -> CmdAuditEngine {
        let mut engine = CmdAuditEngine::new();
        engine.apply_sync(CmdAuditSyncPayload {
            enabled: true,
            policy: Some(CmdAuditPolicy {
                team_id: "t".into(),
                enabled: true,
                dangerous_action: dangerous,
                sensitive_action: CmdAuditAction::Confirm,
                unknown_action: CmdAuditAction::Allow,
                confirm_timeout: 300,
            }),
            rules: Vec::new(),
            sync_interval_sec: 300,
        });
        engine
    }

    #[test]
    fn interactive_exec_preference_off_skips_gate_and_capture() {
        assert!(!interactive_typed_enter_gate(false, true, true));
        assert!(!interactive_capture_active(false, true, true));
        assert!(!interactive_capture_active(true, false, false));
        assert!(interactive_typed_enter_gate(true, true, true));
        assert!(interactive_capture_active(true, true, false));
        assert!(interactive_capture_active(true, false, true));
        assert!(!interactive_typed_enter_gate(true, true, false));
        assert!(!interactive_typed_enter_gate(true, false, true));
    }

    #[test]
    fn interactive_exec_block_writes_one_jsonl_line() {
        let engine = policy_engine(CmdAuditAction::Block);
        let audit = engine.check("rm -rf /");
        assert_eq!(audit.action, CmdAuditAction::Block);

        let rec = interactive_exec_record("sess-1", "10.0.0.8", "rm -rf /", "blocked", None, "", 0);
        assert_eq!(rec.target, "10.0.0.8");
        assert_eq!(rec.command, "rm -rf /");
        assert!(rec.timestamp.contains('T'), "{}", rec.timestamp);
        assert!(rec.exit_code.is_none());
        assert!(!rec.ok);
        assert!(rec.tags.iter().any(|t| t == INTERACTIVE_PTY_TAG));
        assert!(rec.tags.iter().any(|t| t == "blocked"));

        let path = std::env::temp_dir().join(format!(
            "mistterm-interactive-exec-{}.jsonl",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        append_record_at(&path, &rec);
        let rows = read_records_at(&path).expect("read");
        assert_eq!(rows.len(), 1, "one command, one JSONL line");
        assert_eq!(rows[0].target, "10.0.0.8");
        assert_eq!(rows[0].command, "rm -rf /");
        assert!(rows[0].timestamp.contains('T'));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn interactive_exec_allow_records_output_once() {
        let engine = policy_engine(CmdAuditAction::Allow);
        // 无害命令走 unknown → allow，与片段发送同一套引擎。
        let audit = engine.check("echo hello");
        assert_eq!(audit.action, CmdAuditAction::Allow);

        let mut draft = InteractiveExecDraft::start("sess", "web-1", "echo hello", "allowed");
        assert!(!draft.feed(b"echo hello\r\nhello\r\n"));
        assert!(draft.feed(b"user@web-1:~$ "));
        let rec = draft.into_record();
        assert_eq!(rec.command, "echo hello");
        assert_eq!(rec.target, "web-1");
        assert!(rec.timestamp.contains('T'));
        assert!(rec.tags.iter().any(|t| t == "allowed"));
        assert!(rec.output_summary.contains("hello"));
        assert!(!rec.output_summary.contains("user@web-1"));
        assert!(rec.exit_code.is_none());
        assert!(rec.ok);
    }

    #[test]
    fn interactive_exec_osc_133_sets_exit_code_across_chunks() {
        let mut draft = InteractiveExecDraft::start("sess", "web-1", "false", "allowed");
        assert!(!draft.feed(b"false\r\n\x1b]133;D;"));
        assert!(draft.feed(b"7\x07\r\nuser@web-1:~$ "));
        let rec = draft.into_record();
        assert_eq!(rec.exit_code, Some(7));
        assert!(!rec.ok);
        assert_eq!(rec.tags.iter().filter(|t| *t == "allowed").count(), 1);
    }

    #[test]
    fn interactive_exec_prompt_shapes() {
        assert!(is_shell_prompt_line("user@host:~$"));
        assert!(is_shell_prompt_line("root@debian:/var/log#"));
        assert!(is_shell_prompt_line("alice@host ~/work>"));
        assert!(is_shell_prompt_line("PS C:\\Users\\Admin>"));
        assert!(is_shell_prompt_line("$"));
        assert!(!is_shell_prompt_line("rm -rf /"));
        assert!(!is_shell_prompt_line("hello world"));
        assert!(!is_shell_prompt_line("Price is $5"));
    }

    #[test]
    fn interactive_exec_server_block_retags_pending_record() {
        let mut rec = interactive_exec_record("s", "h", "rm -rf /", "allowed", None, "", 0);
        apply_interactive_outcome(&mut rec, "blocked");
        assert!(!rec.ok);
        assert!(rec.exit_code.is_none());
        assert!(rec.tags.iter().any(|t| t == "blocked"));
        assert!(!rec.tags.iter().any(|t| t == "allowed"));
        assert_eq!(
            rec.tags
                .iter()
                .filter(|t| *t == INTERACTIVE_PTY_TAG)
                .count(),
            1
        );
    }

    #[test]
    fn interactive_exec_utf8_output_is_kept() {
        let mut draft = InteractiveExecDraft::start("s", "h", "echo 中文", "allowed");
        let chunk = "echo 中文\r\n中文输出\r\nuser@h:~$ ".as_bytes();
        assert!(draft.feed(chunk));
        let rec = draft.into_record();
        assert!(rec.output_summary.contains("中文输出"));
        assert!(!rec.output_summary.contains("echo 中文"));
    }
}
