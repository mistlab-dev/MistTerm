//! 使用 **PATH 上的 `sz`（lrzsz）子进程** 完成本机→远端 `rz` 的上传，与内置 `zmodem2::Sender` 对照。
//!
//! - **远端**仍是 `rz`（接收）；**本机**必须跑 **`sz`（发送）**，不是 `rz`。
//! - `sz` 的 **stdin** 接 PTY 上读回来的字节（对端 `rz` 的应答）；**stdout** 上的 ZMODEM 流经 `ZmodemWrite` 写入 SSH。
//!
//! 启用：`MISTTERM_ZMODEM_USE_EXTERNAL_SZ=1`。可选：`MISTTERM_ZMODEM_SZ_BIN`（默认 `sz`，如 Homebrew 的 `/opt/homebrew/bin/sz`）。

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::ssh::lrzsz::TransferEvent;
use crate::ssh::manager::{ShellPumpCommand, ShellPumpTx};

const CHUNK: usize = 64 * 1024;
const TRANSFER_DEADLINE: Duration = Duration::from_secs(300);
const STDIN_CHUNK: usize = 32 * 1024;
const PROGRESS_STEP: u64 = 256 * 1024;

/// PTY 旁路队列 → `sz` stdin（**不做**握手续剥，由 lrzsz 自行解析）。
fn drain_upload_queue_to_stdin(
    upload_pty_rx: &Arc<Mutex<Vec<u8>>>,
    pty_pull: &AtomicU64,
    stdin: &mut impl Write,
) -> std::io::Result<bool> {
    let mut g = upload_pty_rx.lock().unwrap();
    if g.is_empty() {
        return Ok(false);
    }
    let n = g.len().min(STDIN_CHUNK);
    let chunk: Vec<u8> = g.drain(..n).collect();
    pty_pull.fetch_add(n as u64, Ordering::Relaxed);
    drop(g);
    stdin.write_all(&chunk)?;
    Ok(true)
}

pub(super) fn run_upload_external_sz(
    file_path: &Path,
    file_name: &str,
    file_size: u64,
    pump_tx: &ShellPumpTx,
    upload_pty_rx: &Arc<Mutex<Vec<u8>>>,
    is_active: &Arc<AtomicBool>,
    received_bytes: &AtomicU64,
    tx: &Sender<TransferEvent>,
    upload_pty_pull_bytes: &Arc<AtomicU64>,
) -> Result<(), String> {
    let bin = std::env::var("MISTTERM_ZMODEM_SZ_BIN").unwrap_or_else(|_| "sz".to_string());
    log::info!(
        "ZMODEM 外部 sz bin={} file={}（stdin←PTY 旁路 stdout→泵；对照内置 zmodem2）",
        bin,
        file_path.display()
    );

    let mut child = Command::new(&bin)
        .args(["-y"])
        .arg(file_path.as_os_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "启动 {} 失败: {}（请安装 lrzsz：如 brew install lrzsz；或设置 MISTTERM_ZMODEM_SZ_BIN）",
                bin, e
            )
        })?;

    let mut stdin = child.stdin.take().ok_or_else(|| "sz stdin 不可用".to_string())?;
    let mut stdout = child.stdout.take().ok_or_else(|| "sz stdout 不可用".to_string())?;
    let stderr = child.stderr.take().ok_or_else(|| "sz stderr 不可用".to_string())?;

    let finished = Arc::new(AtomicBool::new(false));
    let finished_feed = finished.clone();
    let pump_tx = pump_tx.clone();
    let upload_pty_rx_t = upload_pty_rx.clone();
    let pull_atomic = upload_pty_pull_bytes.clone();
    let is_active_feed = is_active.clone();

    let feeder = thread::spawn(move || {
        while !finished_feed.load(Ordering::Acquire) && is_active_feed.load(Ordering::Relaxed) {
            match drain_upload_queue_to_stdin(&upload_pty_rx_t, pull_atomic.as_ref(), &mut stdin) {
                Ok(true) => {}
                Ok(false) => thread::sleep(Duration::from_millis(1)),
                Err(e) => {
                    log::debug!("external sz stdin write finished: {}", e);
                    break;
                }
            }
        }
        let _ = stdin.flush();
    });

    let stderr_h = thread::spawn(move || {
        let mut r = stderr;
        let mut out = Vec::new();
        let mut b = [0u8; 1024];
        loop {
            match r.read(&mut b) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&b[..n]),
                Err(_) => break,
            }
        }
        out
    });

    let deadline = Instant::now() + TRANSFER_DEADLINE;
    let mut buf = vec![0u8; CHUNK];
    let mut pumped = 0u64;
    let mut last_progress_at = 0u64;
    let file_name_owned = file_name.to_string();

    loop {
        if !is_active.load(Ordering::Relaxed) {
            let _ = child.kill();
            finished.store(true, Ordering::Release);
            let _ = feeder.join();
            let err_tail = stderr_h.join().unwrap_or_default();
            if !err_tail.is_empty() {
                log::warn!(
                    "外部 sz stderr: {}",
                    String::from_utf8_lossy(&err_tail)
                );
            }
            return Err("Transfer cancelled by user".to_string());
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            finished.store(true, Ordering::Release);
            let _ = feeder.join();
            let _ = stderr_h.join();
            return Err(format!(
                "外部 sz 超时（{} 秒）",
                TRANSFER_DEADLINE.as_secs()
            ));
        }

        match stdout.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                pumped += n as u64;
                received_bytes.store(pumped.min(file_size), Ordering::Relaxed);
                pump_tx
                    .send(ShellPumpCommand::ZmodemWrite(buf[..n].to_vec()))
                    .map_err(|e| format!("SSH shell 泵队列断开: {}", e))?;
                if pumped.saturating_sub(last_progress_at) >= PROGRESS_STEP {
                    last_progress_at = pumped;
                    let _ = tx.send(TransferEvent::FileProgress {
                        filename: file_name_owned.clone(),
                        received: pumped.min(file_size),
                        total: file_size,
                    });
                }
            }
            Err(e) => {
                finished.store(true, Ordering::Release);
                let _ = feeder.join();
                let err_tail = stderr_h.join().unwrap_or_default();
                if !err_tail.is_empty() {
                    log::warn!(
                        "外部 sz stderr: {}",
                        String::from_utf8_lossy(&err_tail)
                    );
                }
                return Err(format!("读取 sz stdout: {}", e));
            }
        }
    }

    finished.store(true, Ordering::Release);
    let _ = feeder.join();
    let err_tail = stderr_h.join().unwrap_or_default();
    if !err_tail.is_empty() {
        let s = String::from_utf8_lossy(&err_tail);
        if !s.trim().is_empty() {
            log::info!("external sz stderr: {}", s.trim());
        }
    }

    let status = child.wait().map_err(|e| format!("等待 sz: {}", e))?;
    if !status.success() {
        return Err(format!(
            "sz 退出码 {:?}（stderr 已打日志）",
            status.code()
        ));
    }

    received_bytes.store(file_size, Ordering::Relaxed);
    let _ = tx.send(TransferEvent::FileComplete {
        filename: file_name.to_string(),
        path: file_path.to_path_buf(),
    });
    Ok(())
}
