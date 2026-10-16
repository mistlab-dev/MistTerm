//! `mist ssh` — 交互式 Shell。
//!
//! 利用 crossterm 开启终端 raw mode，获取并同步终端行列尺寸；
//! 分离 stdin/stdout 传输通道，支持 SIGWINCH 调整大小与 `~.` 退出转义序列。

use anyhow::{Context, Result};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, size};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::{exec_gate, CliContext};
use crate::core::audit::AuditOutcome;
use crate::core::cmd_audit::{CmdAuditAction, CmdAuditEngine};
use crate::ssh::SshClient;

/// Tracks the line typed in an interactive `mist ssh` session, like the GUI's typed-line buffer.
/// Escape sequences (arrows, history recall) make the local copy unreliable, so the buffer is
/// cleared and that line is not checked — same as the GUI.
#[derive(Default)]
pub(crate) struct TypedLine {
    buf: String,
    in_escape: bool,
}

impl TypedLine {
    /// Feed one input byte (not Enter). Multi-byte UTF-8 is handled by the caller passing whole chars via `push_str`.
    pub(crate) fn feed(&mut self, b: u8) {
        if self.in_escape {
            // End of a CSI/SS3 sequence: a letter or '~'.
            if b.is_ascii_alphabetic() || b == b'~' {
                self.in_escape = false;
            }
            return;
        }
        match b {
            0x1b => {
                self.in_escape = true;
                self.buf.clear();
            }
            0x7f | 0x08 => {
                self.buf.pop();
            }
            0x15 | 0x03 | 0x04 => self.buf.clear(), // Ctrl-U / Ctrl-C / Ctrl-D
            0x17 => {
                let t = self.buf.trim_end_matches(' ').len();
                self.buf.truncate(t);
                let cut = self.buf.rfind(' ').map(|i| i + 1).unwrap_or(0);
                self.buf.truncate(cut);
            }
            0x09 => self.buf.clear(), // Tab completion changes the line remotely
            b if b >= 0x20 => self.buf.push(b as char),
            _ => {}
        }
    }

    pub(crate) fn push_str(&mut self, s: &str) {
        if !self.in_escape {
            self.buf.push_str(s);
        }
    }

    /// Enter pressed: returns the line to check (if any) and resets.
    pub(crate) fn take(&mut self) -> Option<String> {
        let line = self.buf.trim().to_string();
        self.buf.clear();
        self.in_escape = false;
        (!line.is_empty()).then_some(line)
    }
}

fn looks_like_secret_prompt(line: &str) -> bool {
    let l = line.to_lowercase();
    l.contains("password") || l.contains("passphrase") || l.contains("密码")
}

fn cli_zh() -> bool {
    for k in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(k) {
            if !v.is_empty() {
                return v.to_ascii_lowercase().starts_with("zh");
            }
        }
    }
    false
}

/// What to do with a typed line on Enter (team policy only, same as the GUI typed-Enter check).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TypedGate {
    Send,
    Confirm(String),
    Block(String),
}

pub(crate) fn typed_gate(engine: &CmdAuditEngine, line: &str, target: &str) -> TypedGate {
    let audit = engine.check(line);
    let targets = vec![target.to_string()];
    let msg = audit
        .matches
        .first()
        .map(|m| m.message.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_default();
    match audit.action {
        CmdAuditAction::Block => {
            exec_gate::record(
                "command.blocked",
                AuditOutcome::Denied,
                line,
                &audit,
                &targets,
                serde_json::json!({ "via": "mist ssh" }),
            );
            TypedGate::Block(msg)
        }
        CmdAuditAction::Confirm => TypedGate::Confirm(msg),
        _ => {
            if audit.action == CmdAuditAction::Alert {
                exec_gate::record(
                    "command.alert",
                    AuditOutcome::Success,
                    line,
                    &audit,
                    &targets,
                    serde_json::json!({ "via": "mist ssh" }),
                );
            }
            exec_gate::record(
                "command.submit",
                AuditOutcome::Success,
                line,
                &audit,
                &targets,
                serde_json::json!({ "via": "mist ssh" }),
            );
            TypedGate::Send
        }
    }
}

pub fn run_ssh(ctx: &mut CliContext, target: &str) -> Result<i32> {
    let session_cfg = ctx.resolve_target(target)?;
    let config = ctx.ssh_config(&session_cfg)?;
    let mut client = SshClient::new(config);

    client
        .connect()
        .map_err(|e| anyhow::anyhow!("连接失败 {}: {}", session_cfg.name, e))?;
    ctx.mark_connected(&session_cfg);

    let is_tty = crossterm::tty::IsTty::is_tty(&io::stdin());
    let (init_cols, init_rows) = if is_tty {
        size().unwrap_or((80, 24))
    } else {
        (80, 24)
    };

    let mut channel = client
        .open_shell(init_cols as u32, init_rows as u32)
        .map_err(|e| anyhow::anyhow!("打开交互 Shell 失败: {e}"))?;

    // 仅在真实 TTY 下开启 raw mode
    if is_tty {
        enable_raw_mode().context("Failed to enable terminal raw mode")?;
    }

    struct RawModeGuard {
        active: bool,
    }
    impl Drop for RawModeGuard {
        fn drop(&mut self) {
            if self.active {
                let _ = disable_raw_mode();
            }
        }
    }
    let _guard = RawModeGuard { active: is_tty };

    let running = Arc::new(AtomicBool::new(true));
    let r_stdin = running.clone();

    // 监听与处理输入线程（支持 `~.` 退出转义）
    // 通道为非阻塞或通过共享 channel 写入
    let mut write_channel = channel.clone();
    let engine = exec_gate::load_engine();
    let target_name = session_cfg.name.clone();
    // Last line of remote output, to skip the check at password prompts (like the GUI).
    let out_line = Arc::new(std::sync::Mutex::new(String::new()));
    let out_line_in = out_line.clone();
    let stdin_handle = thread::spawn(move || {
        let zh = cli_zh();
        let mut typed = TypedLine::default();
        let mut pending_confirm: Option<String> = None;
        let mut stdin = io::stdin();
        let mut buf = [0u8; 1024];
        let mut at_newline = true;
        let mut seen_tilde = false;

        while r_stdin.load(Ordering::Relaxed) {
            match stdin.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let mut i = 0;
                    while i < n {
                        let b = buf[i];
                        if at_newline && b == b'~' {
                            seen_tilde = true;
                            i += 1;
                            continue;
                        }

                        if seen_tilde {
                            seen_tilde = false;
                            if b == b'.' {
                                // 触发 `~.` 退出
                                r_stdin.store(false, Ordering::Relaxed);
                                return;
                            } else {
                                // 还原之前吞掉的 '~'
                                let _ = write_channel.write_all(b"~");
                            }
                        }

                        // Answer to a "confirm to run" prompt: y runs, anything else cancels.
                        if let Some(line) = pending_confirm.take() {
                            let audit = engine.check(&line);
                            let targets = vec![target_name.clone()];
                            if b == b'y' || b == b'Y' {
                                exec_gate::record(
                                    "command.confirmed",
                                    AuditOutcome::Success,
                                    &line,
                                    &audit,
                                    &targets,
                                    serde_json::json!({ "via": "mist ssh", "confirmed_by": "prompt" }),
                                );
                                exec_gate::record(
                                    "command.submit",
                                    AuditOutcome::Success,
                                    &line,
                                    &audit,
                                    &targets,
                                    serde_json::json!({ "via": "mist ssh", "confirmed": true }),
                                );
                                let _ = write_channel.write_all(b"\r");
                            } else {
                                exec_gate::record(
                                    "command.cancelled",
                                    AuditOutcome::Denied,
                                    &line,
                                    &audit,
                                    &targets,
                                    serde_json::json!({ "via": "mist ssh" }),
                                );
                                eprint!(
                                    "{}",
                                    if zh {
                                        "\r\n[mist] 已取消，没有执行。\r\n"
                                    } else {
                                        "\r\n[mist] Cancelled, not run.\r\n"
                                    }
                                );
                                let _ = write_channel.write_all(b"\x15\r");
                            }
                            let _ = write_channel.flush();
                            at_newline = true;
                            i += 1;
                            continue;
                        }

                        if b == b'\r' || b == b'\n' {
                            at_newline = true;
                            let secret = out_line_in
                                .lock()
                                .map(|l| looks_like_secret_prompt(&l))
                                .unwrap_or(false);
                            if let Some(line) = typed.take().filter(|_| !secret) {
                                match typed_gate(&engine, &line, &target_name) {
                                    TypedGate::Send => {}
                                    TypedGate::Block(msg) => {
                                        let head = if zh {
                                            "团队策略已拦截，没有执行"
                                        } else {
                                            "Blocked by team policy, not run"
                                        };
                                        if msg.is_empty() {
                                            eprint!("\r\n[mist] {head}\r\n");
                                        } else {
                                            eprint!("\r\n[mist] {head}: {msg}\r\n");
                                        }
                                        // Clear the typed line on the remote, then get a fresh prompt.
                                        let _ = write_channel.write_all(b"\x15\r");
                                        let _ = write_channel.flush();
                                        i += 1;
                                        continue;
                                    }
                                    TypedGate::Confirm(msg) => {
                                        let q = if zh {
                                            "团队策略：确认后执行"
                                        } else {
                                            "Team policy: confirm to run"
                                        };
                                        let ask = if zh {
                                            "按 y 执行，其他键取消"
                                        } else {
                                            "press y to run, any other key to cancel"
                                        };
                                        let why = if msg.is_empty() {
                                            String::new()
                                        } else {
                                            format!(" ({msg})")
                                        };
                                        eprint!("\r\n[mist] {q}{why}: {line}\r\n[mist] {ask} ");
                                        pending_confirm = Some(line);
                                        i += 1;
                                        continue;
                                    }
                                }
                            }
                        } else {
                            at_newline = false;
                            if b < 0x80 {
                                typed.feed(b);
                            }
                        }
                        if b >= 0x80 {
                            // Collect a whole UTF-8 char for the typed line.
                            let len = match b {
                                0xc0..=0xdf => 2,
                                0xe0..=0xef => 3,
                                0xf0..=0xf7 => 4,
                                _ => 1,
                            };
                            let end = (i + len).min(n);
                            if let Ok(ch) = std::str::from_utf8(&buf[i..end]) {
                                typed.push_str(ch);
                            }
                            let _ = write_channel.write_all(&buf[i..end]);
                            i = end;
                            continue;
                        }

                        let _ = write_channel.write_all(&[b]);
                        i += 1;
                    }
                    let _ = write_channel.flush();
                }
                Err(_) => break,
            }
        }
        r_stdin.store(false, Ordering::Relaxed);
    });

    // 主线程负责读取通道输出并写到 stdout，同时轮询窗口大小变化
    let mut stdout = io::stdout();
    let mut out_buf = [0u8; 4096];
    let mut last_size = (init_cols, init_rows);

    // 设置 channel 会话为非阻塞以支持并发轮询
    let sess = client.get_session();
    sess.set_blocking(false);

    while running.load(Ordering::Relaxed) {
        // 检查窗口变化
        if let Ok(cur_size) = size() {
            if cur_size != last_size {
                last_size = cur_size;
                let (c, r) = cur_size;
                let _ = channel.request_pty_size(c as u32, r as u32, None, None);
            }
        }

        // 读取远端输出
        match channel.read(&mut out_buf) {
            Ok(0) => {
                // 远端关闭 channel
                break;
            }
            Ok(n) => {
                if let Ok(mut l) = out_line.lock() {
                    let chunk = String::from_utf8_lossy(&out_buf[..n]);
                    match chunk.rfind(['\n', '\r']) {
                        Some(p) => *l = chunk[p + 1..].to_string(),
                        None => l.push_str(&chunk),
                    }
                    if l.len() > 512 {
                        let cut = l.len() - 256;
                        let cut = (cut..l.len())
                            .find(|&c| l.is_char_boundary(c))
                            .unwrap_or(l.len());
                        *l = l[cut..].to_string();
                    }
                }
                let _ = stdout.write_all(&out_buf[..n]);
                let _ = stdout.flush();
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::WouldBlock {
                    thread::sleep(Duration::from_millis(10));
                } else {
                    break;
                }
            }
        }

        if channel.eof() {
            break;
        }
    }

    running.store(false, Ordering::Relaxed);
    let _ = channel.close();
    let _ = channel.wait_close();
    let exit_code = channel.exit_status().unwrap_or(0);

    drop(_guard); // 提前显式恢复 normal mode

    // 等待 stdin 线程退出（如果是远端退出，给 100ms 超时）
    let _ = stdin_handle.join();
    client.disconnect();

    Ok(exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(bytes: &[u8]) -> Option<String> {
        let mut t = TypedLine::default();
        for &b in bytes {
            t.feed(b);
        }
        t.take()
    }

    #[test]
    fn typed_line_editing() {
        assert_eq!(
            typed(b"systemctl restart nginx").as_deref(),
            Some("systemctl restart nginx")
        );
        assert_eq!(typed(b"rm -rf /tmpx\x7f").as_deref(), Some("rm -rf /tmp"));
        assert_eq!(typed(b"abc\x15ls").as_deref(), Some("ls"));
        assert_eq!(typed(b"\x1b[A").as_deref(), None); // history recall: not checked
        assert_eq!(typed(b"ls -l\x17").as_deref(), Some("ls"));
    }

    #[test]
    fn typed_gate_follows_policy() {
        let team = |action: &str| {
            let payload: crate::core::cmd_audit::CmdAuditSyncPayload = serde_json::from_value(serde_json::json!({
                "enabled": true,
                "policy": { "enabled": true, "dangerous_action": "block", "sensitive_action": "confirm", "unknown_action": "allow" },
                "rules": [ { "id": "r1", "name": "no-df", "pattern": "df", "match_type": "prefix", "action": action, "description": "test rule" } ]
            }))
            .unwrap();
            let mut e = CmdAuditEngine::new();
            e.apply_sync(payload);
            e
        };
        assert_eq!(
            typed_gate(&CmdAuditEngine::new(), "ls -la", "t"),
            TypedGate::Send
        );
        assert_eq!(typed_gate(&team("block"), "ls -la", "t"), TypedGate::Send);
        assert!(matches!(
            typed_gate(&team("block"), "df -h", "t"),
            TypedGate::Block(_)
        ));
        assert!(matches!(
            typed_gate(&team("confirm"), "df -h", "t"),
            TypedGate::Confirm(_)
        ));
        assert_eq!(typed_gate(&team("alert"), "df -h", "t"), TypedGate::Send);
    }

    #[test]
    fn secret_prompt_detected() {
        assert!(looks_like_secret_prompt("[sudo] password for deploy: "));
        assert!(!looks_like_secret_prompt("deploy@web:~$ "));
    }
}
