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

use crate::ssh::SshClient;
use super::CliContext;

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
    let stdin_handle = thread::spawn(move || {
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

                        if b == b'\r' || b == b'\n' {
                            at_newline = true;
                        } else {
                            at_newline = false;
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
