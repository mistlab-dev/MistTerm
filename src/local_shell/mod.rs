//! 本机交互式 shell（portable-pty：Windows ConPTY / Unix openpty）。
//!
//! 输出复用 [`crate::ssh::SshMessage`]，以便 [`crate::ui::terminal::TerminalView`] 无需重写消息泵。

use crate::ssh::{SshMessage, SshSessionId};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

static NEXT_LOCAL_SESSION_ID: AtomicUsize = AtomicUsize::new(10_000);
static REPAINT_CTX: std::sync::OnceLock<egui::Context> = std::sync::OnceLock::new();

/// UI 启动时注册一次，供本机 PTY 读线程唤醒重绘（小包回显）。
pub fn set_repaint_context(ctx: &egui::Context) {
    let _ = REPAINT_CTX.set(ctx.clone());
}

fn wake_ui() {
    if let Some(ctx) = REPAINT_CTX.get() {
        ctx.request_repaint();
    }
}

/// 解析本机默认 shell 可执行文件。
pub fn default_shell_program() -> String {
    #[cfg(windows)]
    {
        for cand in ["pwsh.exe", "pwsh", "powershell.exe", "powershell"] {
            if lookup_on_path(cand).is_some() {
                return cand.to_string();
            }
        }
        "cmd.exe".to_string()
    }
    #[cfg(not(windows))]
    {
        if let Ok(shell) = std::env::var("SHELL") {
            let t = shell.trim();
            if !t.is_empty() && Path::new(t).exists() {
                return t.to_string();
            }
        }
        for p in ["/bin/zsh", "/bin/bash", "/bin/sh"] {
            if Path::new(p).exists() {
                return p.to_string();
            }
        }
        "/bin/sh".to_string()
    }
}

fn lookup_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        #[cfg(windows)]
        {
            let stem = name.trim_end_matches(".exe");
            for ext in ["exe", "cmd", "bat", ""] {
                let candidate = if ext.is_empty() {
                    dir.join(stem)
                } else {
                    dir.join(format!("{stem}.{ext}"))
                };
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        #[cfg(not(windows))]
        {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn default_cwd() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn build_command(program: &str) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(program);
    #[cfg(windows)]
    {
        let lower = program.to_ascii_lowercase();
        if lower.contains("powershell") || lower.contains("pwsh") {
            cmd.args(["-NoLogo"]);
        }
    }
    if let Some(home) = dirs::home_dir() {
        let _ = cmd.cwd(home);
    }
    cmd
}

/// 本机 PTY 写入/尺寸句柄（可 Clone，供 UI 线程使用）。
#[derive(Clone)]
pub struct LocalShellHandle {
    pub session_id: SshSessionId,
    input_tx: SyncSender<Vec<u8>>,
    resize_tx: SyncSender<(u32, u32)>,
    alive: Arc<AtomicBool>,
}

impl LocalShellHandle {
    pub fn send_input(&self, data: &[u8]) -> Result<(), String> {
        if !self.alive.load(Ordering::Relaxed) {
            return Err("local shell not running".into());
        }
        self.input_tx
            .try_send(data.to_vec())
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => "local shell input queue full".into(),
                mpsc::TrySendError::Disconnected(_) => "local shell input disconnected".into(),
            })
    }

    pub fn resize_pty(&self, cols: u32, rows: u32) -> Result<(), String> {
        if !self.alive.load(Ordering::Relaxed) {
            return Err("local shell not running".into());
        }
        let cols = cols.clamp(20, 512);
        let rows = rows.clamp(5, 256);
        self.resize_tx
            .try_send((cols, rows))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => {
                    "local shell resize queue full (will retry next frame)".into()
                }
                mpsc::TrySendError::Disconnected(_) => "local shell resize disconnected".into(),
            })
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    /// 请求结束本机 shell（控制循环退出后会发 Disconnected）。
    pub fn request_stop(&self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = self.input_tx.try_send(Vec::new());
    }
}

/// 启动本机默认 shell，返回 UI 消息接收端与写入句柄。
pub fn spawn_local_shell(
    cols: u32,
    rows: u32,
) -> Result<(Receiver<SshMessage>, LocalShellHandle), String> {
    let program = default_shell_program();
    let cwd = default_cwd();
    let session_id = NEXT_LOCAL_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    let (msg_tx, msg_rx) = mpsc::channel::<SshMessage>();
    let (input_tx, input_rx) = sync_channel::<Vec<u8>>(256);
    let (resize_tx, resize_rx) = sync_channel::<(u32, u32)>(16);
    let alive = Arc::new(AtomicBool::new(true));
    let alive_thread = alive.clone();

    let handle = LocalShellHandle {
        session_id,
        input_tx,
        resize_tx,
        alive,
    };

    thread::Builder::new()
        .name(format!("local-shell-{session_id}"))
        .spawn(move || {
            if let Err(e) = run_local_shell(
                session_id,
                &program,
                &cwd,
                cols,
                rows,
                msg_tx.clone(),
                input_rx,
                resize_rx,
                alive_thread.clone(),
            ) {
                let _ = msg_tx.send(SshMessage::Error {
                    session_id,
                    error: e,
                });
                wake_ui();
            }
            alive_thread.store(false, Ordering::Relaxed);
            let _ = msg_tx.send(SshMessage::Disconnected { session_id });
            wake_ui();
        })
        .map_err(|e| format!("spawn local-shell thread: {e}"))?;

    Ok((msg_rx, handle))
}

fn run_local_shell(
    session_id: SshSessionId,
    program: &str,
    cwd: &Path,
    cols: u32,
    rows: u32,
    msg_tx: mpsc::Sender<SshMessage>,
    input_rx: Receiver<Vec<u8>>,
    resize_rx: Receiver<(u32, u32)>,
    alive: Arc<AtomicBool>,
) -> Result<(), String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: rows.clamp(5, 256) as u16,
            cols: cols.clamp(20, 512) as u16,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {e}"))?;

    let mut cmd = build_command(program);
    let _ = cmd.cwd(cwd);
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn {program}: {e}"))?;
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("clone reader: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take writer: {e}"))?;
    let writer = Arc::new(Mutex::new(writer));
    let master: Arc<Mutex<Box<dyn MasterPty + Send>>> = Arc::new(Mutex::new(pair.master));

    let _ = msg_tx.send(SshMessage::Connected { session_id });
    wake_ui();

    let reader_alive = alive.clone();
    let reader_tx = msg_tx.clone();
    let reader_join = thread::Builder::new()
        .name(format!("local-shell-rd-{session_id}"))
        .spawn(move || {
            let mut buf = vec![0u8; 32 * 1024];
            while reader_alive.load(Ordering::Relaxed) {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = reader_tx.send(SshMessage::Output {
                            session_id,
                            data: buf[..n].to_vec(),
                        });
                        wake_ui();
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => {
                        log::debug!("local shell read end: {e}");
                        break;
                    }
                }
            }
        })
        .map_err(|e| format!("spawn local-shell reader: {e}"))?;

    while alive.load(Ordering::Relaxed) {
        // 输入（含空包 stop 信号）
        match input_rx.recv_timeout(Duration::from_millis(16)) {
            Ok(chunk) => {
                if chunk.is_empty() {
                    alive.store(false, Ordering::Relaxed);
                    break;
                }
                let mut w = writer
                    .lock()
                    .map_err(|_| "local shell writer poisoned".to_string())?;
                w.write_all(&chunk)
                    .map_err(|e| format!("pty write: {e}"))?;
                let _ = w.flush();
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                alive.store(false, Ordering::Relaxed);
                break;
            }
        }

        // 尺寸（合并为最后一次）
        let mut last_size = None;
        while let Ok(sz) = resize_rx.try_recv() {
            last_size = Some(sz);
        }
        if let Some((c, r)) = last_size {
            if let Ok(m) = master.lock() {
                let _ = m.resize(PtySize {
                    rows: r as u16,
                    cols: c as u16,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            }
        }

        match child.try_wait() {
            Ok(Some(status)) => {
                log::info!("local shell exited: {status:?}");
                break;
            }
            Ok(None) => {}
            Err(e) => {
                log::debug!("local shell try_wait: {e}");
                break;
            }
        }
    }

    alive.store(false, Ordering::Relaxed);
    let _ = child.kill();
    let _ = child.wait();
    // 关掉 writer，促使读端 EOF
    drop(writer);
    let _ = reader_join.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_program_non_empty() {
        let p = default_shell_program();
        assert!(!p.trim().is_empty(), "default shell must be non-empty");
    }

    #[test]
    fn lookup_common_shell_or_fallback() {
        #[cfg(windows)]
        {
            let p = default_shell_program();
            assert!(
                p.to_ascii_lowercase().contains("cmd")
                    || p.to_ascii_lowercase().contains("powershell")
                    || p.to_ascii_lowercase().contains("pwsh"),
                "unexpected windows shell {p}"
            );
        }
        #[cfg(not(windows))]
        {
            let p = default_shell_program();
            assert!(p.contains('/'), "unix shell should be a path: {p}");
        }
    }

    #[test]
    fn spawn_local_shell_emits_connected() {
        let (rx, handle) = spawn_local_shell(80, 24).expect("spawn local shell");
        let msg = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("Connected within 5s");
        match msg {
            SshMessage::Connected { session_id } => {
                assert_eq!(session_id, handle.session_id);
            }
            other => panic!("expected Connected, got {other:?}"),
        }
        // 写入应成功（回显因 shell/提示符差异不强制断言）
        let _ = handle.send_input(b"echo mist-local-shell\r");
        handle.request_stop();
        let mut saw_disconnect = false;
        for _ in 0..50 {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(SshMessage::Disconnected { .. }) => {
                    saw_disconnect = true;
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        assert!(saw_disconnect, "expected Disconnected after request_stop");
    }

    /// 端到端：spawn → 等待提示/输出 → echo 标记串 → 在 PTY 输出中看到该串。
    #[test]
    fn spawn_local_shell_echo_roundtrip() {
        let marker = "mist_local_echo_9f3a";
        let (rx, handle) = spawn_local_shell(80, 24).expect("spawn local shell");

        // Connected
        let mut connected = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        let mut output = Vec::<u8>::new();
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(SshMessage::Connected { .. }) => connected = true,
                Ok(SshMessage::Output { data, .. }) => output.extend_from_slice(&data),
                Ok(SshMessage::Error { error, .. }) => panic!("local shell error: {error}"),
                Ok(SshMessage::Disconnected { .. }) if !connected => {
                    panic!("disconnected before Connected")
                }
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if connected {
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => panic!("channel closed early"),
            }
            if connected && !output.is_empty() {
                // 已有一些初始输出（提示符等）即可发命令
                break;
            }
        }
        assert!(connected, "did not receive Connected");

        // 给 shell 一点时间就绪后再发
        thread::sleep(Duration::from_millis(200));
        #[cfg(windows)]
        let cmd = format!("echo {marker}\r");
        #[cfg(not(windows))]
        let cmd = format!("printf '%s\\n' {marker}\r");
        handle
            .send_input(cmd.as_bytes())
            .expect("send echo command");

        let echo_deadline = std::time::Instant::now() + Duration::from_secs(8);
        let mut saw_marker = false;
        while std::time::Instant::now() < echo_deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(SshMessage::Output { data, .. }) => {
                    output.extend_from_slice(&data);
                    let text = String::from_utf8_lossy(&output);
                    if text.contains(marker) {
                        saw_marker = true;
                        break;
                    }
                }
                Ok(SshMessage::Disconnected { .. }) => break,
                Ok(SshMessage::Error { error, .. }) => panic!("error after echo: {error}"),
                Ok(_) => {}
                Err(_) => {}
            }
        }
        handle.request_stop();
        assert!(
            saw_marker,
            "PTY output did not contain marker {marker}; got: {}",
            String::from_utf8_lossy(&output)
                .chars()
                .take(800)
                .collect::<String>()
        );
    }
}
