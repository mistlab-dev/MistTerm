#![cfg(unix)]

//! 三种上传路径集成测试：ZMODEM（`rz` 通道）、SCP、远端 `cat >` 重定向。
//!
//! ## 如何运行
//!
//! 默认从仓库根目录的 `sessions.json` 读取第一条会话（与 UI 相同解密逻辑）。
//! 也可指定路径：`MISTTERM_SESSIONS_JSON=/path/to/sessions.json`
//!
//! ```bash
//! cd /path/to/MistTerm
//! cargo test --test ssh_transfer_three_modes ssh_transfer_three_modes -- --nocapture
//! ```
//!
//! 无可用凭据或网络不可达时测试会 **跳过**（不 panic），避免 CI 误报。
//!
//! **ZMODEM**：单线程非阻塞全双工（`PTY↔sz`），远端 `rz -bye -y -e`，本机 `sz -t 1000 -bye -y -e`；`Channel` 仅在本线程访问（libssh2 会话锁不支持并发读/写）。

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::TcpStream;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use libc::{c_int, fcntl, nfds_t, poll, pollfd, POLLIN, POLLOUT};
use mistterm::core::session::SessionManager;
use ssh2::{BlockDirections, Session};

const ZMODEM_IO_TIMEOUT: Duration = Duration::from_secs(120);

fn io_would_block(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted)
}

fn set_fd_nonblocking(fd: c_int, on: bool) -> Result<(), String> {
    unsafe {
        let flags = fcntl(fd, libc::F_GETFL);
        if flags < 0 {
            return Err(format!("F_GETFL: {}", std::io::Error::last_os_error()));
        }
        let newflags = if on {
            flags | libc::O_NONBLOCK
        } else {
            flags & !libc::O_NONBLOCK
        };
        if fcntl(fd, libc::F_SETFL, newflags) < 0 {
            return Err(format!("F_SETFL: {}", std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

/// ZMODEM 段结束后恢复阻塞与 fd 标志，供后续 libssh2 同步调用使用。
fn restore_zmodem_fds(
    session: &Session,
    tcp_dup: &TcpStream,
    fd: c_int,
    sz_stdout: &mut std::process::ChildStdout,
    sz_stdin_opt: &mut Option<std::process::ChildStdin>,
) -> Result<(), String> {
    session.set_blocking(true);
    let _ = tcp_dup.set_nonblocking(false);
    set_fd_nonblocking(fd, false)?;
    set_fd_nonblocking(sz_stdout.as_raw_fd(), false)?;
    if let Some(s) = sz_stdin_opt {
        set_fd_nonblocking(s.as_raw_fd(), false)?;
    }
    Ok(())
}

fn sessions_path() -> PathBuf {
    if let Ok(p) = std::env::var("MISTTERM_SESSIONS_JSON") {
        PathBuf::from(p)
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sessions.json")
    }
}

fn try_load_first_session() -> Option<mistterm::core::session::SessionConfig> {
    let path = sessions_path();
    if !path.exists() {
        eprintln!("SKIP: sessions 文件不存在: {}", path.display());
        return None;
    }
    let mgr = SessionManager::with_sessions_path(path);
    let s = mgr.list_sessions().first().cloned()?;
    if s.password.is_empty() {
        eprintln!("SKIP: 第一条会话密码为空（无法在本机解密或尚未配置）");
        return None;
    }
    Some(s)
}

/// 返回 `(Session, socket 克隆)`，克隆与 Session 底层共用同一 fd，仅用于切换非阻塞位。
fn connect(cfg: &mistterm::core::session::SessionConfig) -> Result<(Session, TcpStream), String> {
    let addr = format!("{}:{}", cfg.host, cfg.port);
    let tcp = TcpStream::connect(&addr).map_err(|e| format!("TCP: {}", e))?;
    tcp.set_read_timeout(Some(Duration::from_secs(60))).ok();
    let tcp_dup = tcp
        .try_clone()
        .map_err(|e| format!("TCP try_clone（ZMODEM 需 fd 共享）: {}", e))?;
    let mut session = Session::new().map_err(|e| format!("session: {}", e))?;
    session.set_tcp_stream(tcp);
    session.set_blocking(true);
    session.handshake().map_err(|e| format!("handshake: {}", e))?;
    session
        .userauth_password(&cfg.username, &cfg.password)
        .map_err(|e| format!("auth: {}", e))?;
    Ok((session, tcp_dup))
}

fn exec_capture(session: &Session, cmd: &str) -> Result<Vec<u8>, String> {
    let mut ch = session
        .channel_session()
        .map_err(|e| format!("channel: {}", e))?;
    ch.exec(cmd).map_err(|e| format!("exec: {}", e))?;
    let mut out = Vec::new();
    ch.read_to_end(&mut out).map_err(|e| format!("read: {}", e))?;
    let _ = ch.wait_close();
    Ok(out)
}

fn exec_status_ok(session: &Session, cmd: &str) -> Result<(), String> {
    let mut ch = session
        .channel_session()
        .map_err(|e| format!("channel: {}", e))?;
    ch.exec(cmd).map_err(|e| format!("exec: {}", e))?;
    let mut sink = Vec::new();
    let _ = ch.read_to_end(&mut sink);
    ch.wait_close().map_err(|e| format!("wait_close: {}", e))?;
    let code = ch.exit_status().unwrap_or(-1);
    if code != 0 {
        return Err(format!("命令非零退出: {} (code={})", cmd, code));
    }
    Ok(())
}

fn upload_via_scp(session: &Session, local: &Path, remote: &str) -> Result<(), String> {
    let data = fs::read(local).map_err(|e| format!("read local: {}", e))?;
    let len = data.len() as u64;
    let mut scp = session
        .scp_send(Path::new(remote), 0o644, len, None)
        .map_err(|e| format!("scp_send: {}", e))?;
    scp.write_all(&data).map_err(|e| format!("scp write: {}", e))?;
    scp.send_eof().map_err(|e| format!("scp eof: {}", e))?;
    scp.wait_eof().map_err(|e| format!("scp wait_eof: {}", e))?;
    scp.close().map_err(|e| format!("scp close: {}", e))?;
    scp.wait_close().map_err(|e| format!("scp wait_close: {}", e))?;
    Ok(())
}

fn upload_via_cat_redirect(session: &Session, local: &Path, remote: &str) -> Result<(), String> {
    let data = fs::read(local).map_err(|e| format!("read local: {}", e))?;
    let mut ch = session
        .channel_session()
        .map_err(|e| format!("channel: {}", e))?;
    let remote_escaped = shell_single_quote(remote);
    ch.exec(&format!("cat > {}", remote_escaped))
        .map_err(|e| format!("exec: {}", e))?;
    ch.write_all(&data).map_err(|e| format!("write: {}", e))?;
    ch.send_eof().map_err(|e| format!("send_eof: {}", e))?;
    let mut stderr_out = Vec::new();
    let _ = ch.read_to_end(&mut stderr_out);
    ch.wait_close().map_err(|e| format!("wait_close: {}", e))?;
    let code = ch.exit_status().unwrap_or(-1);
    if code != 0 {
        return Err(format!("cat> 退出码 {}", code));
    }
    Ok(())
}

fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

fn remote_file_len(session: &Session, remote: &str) -> Result<u64, String> {
    let out = exec_capture(
        session,
        &format!("wc -c < {}", shell_single_quote(remote)),
    )?;
    let text = String::from_utf8_lossy(&out);
    text.trim()
        .parse::<u64>()
        .map_err(|_| format!("解析 wc -c 失败: {:?}", text))
}

fn download_via_cat(session: &Session, remote: &str) -> Result<Vec<u8>, String> {
    let mut ch = session
        .channel_session()
        .map_err(|e| format!("channel: {}", e))?;
    ch.exec(&format!("cat {}", shell_single_quote(remote)))
        .map_err(|e| format!("exec: {}", e))?;
    let mut buf = Vec::new();
    ch.read_to_end(&mut buf).map_err(|e| format!("read: {}", e))?;
    let _ = ch.wait_close();
    Ok(buf)
}

/// 使用本机 `sz` 与远端 `rz` 做 **权威 ZMODEM** 集成验证（与 lrzsz 参考实现一致）。
///
/// 需要本机 PATH 中有 `sz`（macOS: `brew install lrzsz`）。
/// `tcp_dup`：与 `session` 共用 fd；ZMODEM 段内会恢复为阻塞模式（与 libssh2 阻塞 API 一致）。
fn upload_via_zmodem_sz_to_rz(
    session: &Session,
    tcp_dup: &TcpStream,
    local: &Path,
    remote_dir: &str,
) -> Result<(), String> {
    if local.file_name().and_then(|n| n.to_str()).is_none() {
        return Err("本地文件名无效".to_string());
    }

    let sz_check = std::process::Command::new("which")
        .arg("sz")
        .output()
        .map_err(|e| format!("which sz 失败：{}", e))?;
    if !sz_check.status.success() {
        return Err("本机未找到 sz（例如 macOS: brew install lrzsz）；无法用参考实现校验 ZMODEM".into());
    }

    let mut ch = session
        .channel_session()
        .map_err(|e| format!("channel: {}", e))?;
    ch.request_pty("xterm-256color", None, Some((80u32, 24u32, 640u32, 480u32)))
        .map_err(|e| format!("request_pty: {}", e))?;
    let dir_escaped = remote_dir.replace('\'', "'\"'\"'");
    // `-e`：发送/接收双方约定转义，lrzsz 文档推荐在 telnet/SSH 等非透明链路上与 `sz -e` 同用。
    // 不在此强行 `stty raw`（部分环境会导致 rz 行为异常）；依赖 request_pty + 下列握手判定。
    let cmd = format!(
        r#"unset PROMPT_COMMAND; LANG=C LC_ALL=C; exec bash --noprofile --norc -c 'cd {} && exec rz -bye -y -e'"#,
        dir_escaped,
    );
    ch.exec(&cmd).map_err(|e| format!("exec rz: {}", e))?;
    thread::sleep(Duration::from_millis(300));

    // ZMODEM 在同一条全双工链路上协商：rz 写出的帧必须进入 sz 的 stdin（此前仅读信道却不喂给 sz 会导致 pathname 超时 / 退出 128）。
    // -t：十分之一秒，lrzsz 允许 10..=1000；1000 => 100s。
    let mut child = std::process::Command::new("sz")
        .args(["-t", "1000", "-bye", "-y", "-e"])
        .arg(local)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 sz：{}", e))?;

    let mut sz_stderr = child
        .stderr
        .take()
        .ok_or_else(|| "sz 无 stderr".to_string())?;
    let err_thread = thread::spawn(move || {
        let mut v = Vec::new();
        let _ = sz_stderr.read_to_end(&mut v);
        v
    });

    let mut sz_stdin_opt = Some(
        child
            .stdin
            .take()
            .ok_or_else(|| "sz 无 stdin".to_string())?,
    );
    let mut sz_stdout = child
        .stdout
        .take()
        .ok_or_else(|| "sz 无 stdout".to_string())?;

    let fd = tcp_dup.as_raw_fd();
    set_fd_nonblocking(fd, true)?;
    session.set_blocking(false);
    set_fd_nonblocking(sz_stdout.as_raw_fd(), true)?;
    if let Some(s) = &sz_stdin_opt {
        set_fd_nonblocking(s.as_raw_fd(), true)?;
    }

    let deadline = Instant::now() + ZMODEM_IO_TIMEOUT;
    let mut buf = [0u8; 64 * 1024];
    let mut pending_rz_to_sz: Vec<u8> = Vec::new();
    let mut pending_sz_to_ch: Vec<u8> = Vec::new();
    let mut sz_stdout_done = false;
    let mut ch_stdout_eof = false;
    let mut ch_sent_eof = false;
    let mut child_done: Option<std::process::ExitStatus> = None;

    let st = loop {
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = restore_zmodem_fds(session, tcp_dup, fd, &mut sz_stdout, &mut sz_stdin_opt);
            return Err("ZMODEM I/O 超时（120s）".into());
        }

        let mut progressed = false;

        if !ch_stdout_eof {
            match ch.read(&mut buf) {
                Ok(0) => {
                    ch_stdout_eof = true;
                    progressed = true;
                }
                Ok(n) => {
                    pending_rz_to_sz.extend_from_slice(&buf[..n]);
                    progressed = true;
                }
                Err(e) if io_would_block(&e) => {}
                Err(e) => {
                    let _ = restore_zmodem_fds(session, tcp_dup, fd, &mut sz_stdout, &mut sz_stdin_opt);
                    return Err(format!("PTY 读: {}", e));
                }
            }
        }

        if let Some(ref mut si) = sz_stdin_opt {
            let mut woff = 0usize;
            while woff < pending_rz_to_sz.len() {
                match si.write(&pending_rz_to_sz[woff..]) {
                    Ok(0) => {
                        let _ = restore_zmodem_fds(session, tcp_dup, fd, &mut sz_stdout, &mut sz_stdin_opt);
                        return Err("sz stdin 写 0".into());
                    }
                    Ok(n) => {
                        woff += n;
                        progressed = true;
                    }
                    Err(e) if io_would_block(&e) => break,
                    Err(e) => {
                        let _ = restore_zmodem_fds(session, tcp_dup, fd, &mut sz_stdout, &mut sz_stdin_opt);
                        return Err(format!("sz stdin: {}", e));
                    }
                }
            }
            if woff > 0 {
                pending_rz_to_sz.drain(..woff);
            }
        }
        if ch_stdout_eof && pending_rz_to_sz.is_empty() {
            sz_stdin_opt.take();
        }

        if !sz_stdout_done {
            match sz_stdout.read(&mut buf) {
                Ok(0) => {
                    sz_stdout_done = true;
                    progressed = true;
                }
                Ok(n) => {
                    pending_sz_to_ch.extend_from_slice(&buf[..n]);
                    progressed = true;
                }
                Err(e) if io_would_block(&e) => {}
                Err(e) => {
                    let _ = restore_zmodem_fds(session, tcp_dup, fd, &mut sz_stdout, &mut sz_stdin_opt);
                    return Err(format!("sz stdout: {}", e));
                }
            }
        }

        if !ch_sent_eof {
            let mut woff = 0usize;
            while woff < pending_sz_to_ch.len() {
                match ch.write(&pending_sz_to_ch[woff..]) {
                    Ok(0) => {
                        let _ = restore_zmodem_fds(session, tcp_dup, fd, &mut sz_stdout, &mut sz_stdin_opt);
                        return Err("PTY 写 0".into());
                    }
                    Ok(n) => {
                        woff += n;
                        progressed = true;
                    }
                    Err(e) if io_would_block(&e) => break,
                    Err(e) => {
                        let _ = restore_zmodem_fds(session, tcp_dup, fd, &mut sz_stdout, &mut sz_stdin_opt);
                        return Err(format!("PTY 写: {}", e));
                    }
                }
            }
            if woff > 0 {
                pending_sz_to_ch.drain(..woff);
            }
            let _ = ch.flush();
        }

        if sz_stdout_done && pending_sz_to_ch.is_empty() && !ch_sent_eof {
            let _ = ch.flush();
            let _ = ch.send_eof();
            ch_sent_eof = true;
            progressed = true;
        }

        if child_done.is_none() {
            match child.try_wait() {
                Ok(Some(st)) => {
                    child_done = Some(st);
                    progressed = true;
                }
                Ok(None) => {}
                Err(e) => {
                    let _ = restore_zmodem_fds(session, tcp_dup, fd, &mut sz_stdout, &mut sz_stdin_opt);
                    return Err(format!("sz try_wait: {}", e));
                }
            }
        }

        if child_done.is_some()
            && sz_stdout_done
            && pending_sz_to_ch.is_empty()
            && ch_sent_eof
        {
            restore_zmodem_fds(session, tcp_dup, fd, &mut sz_stdout, &mut sz_stdin_opt)?;
            break child_done.take().expect("child_done");
        }

        if !progressed {
            // libssh2 非阻塞：在 EAGAIN 后须按 block_directions 等在 socket 上，否则可能长期无进展。
            let ssh_ev: i16 = match session.block_directions() {
                BlockDirections::None => (POLLIN | POLLOUT) as i16,
                BlockDirections::Inbound => POLLIN as i16,
                BlockDirections::Outbound => POLLOUT as i16,
                BlockDirections::Both => (POLLIN | POLLOUT) as i16,
            };
            let mut fds = vec![
                pollfd {
                    fd,
                    events: ssh_ev,
                    revents: 0,
                },
                pollfd {
                    fd: sz_stdout.as_raw_fd(),
                    events: POLLIN as i16,
                    revents: 0,
                },
            ];
            if let Some(ref s) = sz_stdin_opt {
                if !pending_rz_to_sz.is_empty() {
                    fds.push(pollfd {
                        fd: s.as_raw_fd(),
                        events: POLLOUT as i16,
                        revents: 0,
                    });
                }
            }
            unsafe {
                let _ = poll(
                    fds.as_mut_ptr(),
                    fds.len() as nfds_t,
                    50,
                );
            }
        }
    };
    let stderr_bytes = err_thread.join().map_err(|_| "收集 sz stderr 线程失败".to_string())?;
    if !st.success() {
        return Err(format!(
            "sz 非零退出：{:?}，stderr：{}",
            st.code(),
            String::from_utf8_lossy(&stderr_bytes)
        ));
    }

    Ok(())
}

#[test]
fn ssh_transfer_three_modes() {
    let Some(cfg) = try_load_first_session() else {
        return;
    };

    let (session, tcp_dup) = match connect(&cfg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("SKIP: 无法连接 SSH: {}", e);
            return;
        }
    };

    let remote_dir = format!("/tmp/mistterm_itest_{}", std::process::id());
    if let Err(e) = exec_status_ok(
        &session,
        &format!("mkdir -p {}", shell_single_quote(&remote_dir)),
    ) {
        panic!("准备远端目录失败: {}", e);
    }

    let tmp = std::env::temp_dir();
    let stamp = format!(
        "mistterm-itest-{}\n",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let base_local = tmp.join(format!("mistterm_payload_{}.bin", std::process::id()));
    let mut payload: Vec<u8> = stamp.as_bytes().to_vec();
    payload.extend((0..4096u32).map(|i| (i % 251) as u8));
    fs::write(&base_local, &payload).expect("写本地临时文件");

    // 1) SCP 上传 + cat 下载校验
    let r_scp = format!("{}/scp_mode.bin", remote_dir);
    upload_via_scp(&session, &base_local, &r_scp).expect("SCP 上传");
    assert_eq!(
        remote_file_len(&session, &r_scp).expect("远程大小"),
        payload.len() as u64
    );
    let back = download_via_cat(&session, &r_scp).expect("下载 scp 文件");
    assert_eq!(back, payload);

    // 2) cat 重定向上传 + 下载校验
    let r_cat = format!("{}/redirect_mode.bin", remote_dir);
    upload_via_cat_redirect(&session, &base_local, &r_cat).expect("重定向上传");
    assert_eq!(
        remote_file_len(&session, &r_cat).expect("远程大小"),
        payload.len() as u64
    );
    let back2 = download_via_cat(&session, &r_cat).expect("下载 redirect 文件");
    assert_eq!(back2, payload);

    // 3) ZMODEM（lrzsz：本机 sz → 远端 rz）
    let local_z = tmp.join(format!("mistterm_zmodem_src_{}.bin", std::process::id()));
    fs::write(&local_z, &payload).expect("写 zmodem 源文件");
    upload_via_zmodem_sz_to_rz(&session, &tcp_dup, &local_z, &remote_dir).expect("ZMODEM sz→rz");
    let dest = format!(
        "{}/{}",
        remote_dir,
        local_z.file_name().unwrap().to_string_lossy()
    );
    assert_eq!(
        remote_file_len(&session, &dest).expect("zmodem 远程大小"),
        payload.len() as u64
    );
    let back3 = download_via_cat(&session, &dest).expect("下载 zmodem 文件");
    assert_eq!(back3, payload);

    let _ = fs::remove_file(&base_local);
    let _ = fs::remove_file(&local_z);
    let _ = exec_status_ok(
        &session,
        &format!("rm -rf {}", shell_single_quote(&remote_dir)),
    );
}
