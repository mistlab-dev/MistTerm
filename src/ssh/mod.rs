//! SSH 层 - 负责 SSH 连接和通信
//!
//! **三种独立的「传文件」方式**（入口与实现互不合并）：
//! 1. **ZMODEM / lrzsz**：终端里 `rz`/`sz` 与 `LrzszTransfer` + 专用 shell 泵线程 `ZmodemWrite`（`sync_channel`）；收发协议均由 `zmodem2` 状态机实现。
//! 2. **SFTP**：侧栏 SFTP 面板，独立会话/逻辑（见 UI）。
//! 3. **直传**：`TerminalView::start_upload` → [`ScpUpload`]（SCP，分块排在 shell 泵线程执行），不经 ZMODEM。

/// 阻塞模式下单次 libssh2 调用最长等待（毫秒）。远端不回应时报错返回，而不是永久挂住调用线程。
pub const DEFAULT_BLOCKING_TIMEOUT_MS: u32 = 30_000;

/// RAII guard：临时将 libssh2 `Session` 切到阻塞模式并设超时，drop 时恢复非阻塞与原超时。
///
/// libssh2 的 `set_blocking` / `set_timeout` 是 **Session 级别全局设置**，会影响该 Session 上所有
/// channel（包括 shell_pump 正在跑的 PTY channel）。任何临时切阻塞的操作都**必须**在退出前切回
/// 非阻塞，否则 shell_pump 线程会永久阻塞在 `channel.read()` 上（终端卡死、菜单能动但输入不响应）。
///
/// 用法：
/// ```ignore
/// let _g = SessionBlockingGuard::new(&session);
/// // ... 阻塞读写操作，无论中间是 ? 提前返回、panic、还是正常结束，drop 都会自动恢复
/// ```
pub struct SessionBlockingGuard {
    session: ssh2::Session,
    prev_timeout_ms: u32,
}

impl SessionBlockingGuard {
    pub fn new(session: &ssh2::Session) -> Self {
        Self::with_timeout(session, DEFAULT_BLOCKING_TIMEOUT_MS)
    }

    /// `timeout_ms == 0` 表示不限时，仅用于独立连接上跑用户命令等允许长时间无输出的场景。
    pub fn with_timeout(session: &ssh2::Session, timeout_ms: u32) -> Self {
        let prev_timeout_ms = session.timeout();
        session.set_blocking(true);
        session.set_timeout(timeout_ms);
        Self {
            session: session.clone(),
            prev_timeout_ms,
        }
    }
}

impl Drop for SessionBlockingGuard {
    fn drop(&mut self) {
        self.session.set_timeout(self.prev_timeout_ms);
        self.session.set_blocking(false);
    }
}

mod client;
mod jump;
mod known_hosts;
mod port_forward;
mod socks_proxy;
mod proxy_command;
mod user_facing;
mod manager;
mod lrzsz;
mod lrzsz_zmodem2_send;
mod lrzsz_external_sz;
mod zmodem_pty_pipeline;
mod file_transfer;
mod scp_upload;
#[cfg(test)]
mod pty_write_drain_tests;
pub mod zmodem_pty_prefix;
pub mod sftp;

pub use client::{SshClient, SshConfig};
pub use port_forward::{
    spawn_local_forward_controllable, spawn_remote_forward_controllable, ForwardControl,
    LocalPortForward, RemotePortForward,
};
pub use socks_proxy::{spawn_dynamic_forward_controllable, DynamicPortForward};
pub use jump::{JumpHop, parse_jump_chain, parse_jump_endpoint};
pub use user_facing::format_ssh_connect_error;
pub use sftp::{SftpClient, SftpEntry};
pub use manager::{SshManager, SshMessage, SshSessionHandle, SshSessionId};
pub use lrzsz::{LrzszTransfer, TransferEvent};
pub use file_transfer::{FileTransfer, ProgressTracker};
pub use scp_upload::ScpUpload;

#[cfg(test)]
mod blocking_guard_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn guard_sets_default_timeout_and_restores_previous_state() {
        let session = ssh2::Session::new().unwrap();
        session.set_timeout(500);
        {
            let _g = SessionBlockingGuard::new(&session);
            assert!(session.is_blocking());
            assert_eq!(session.timeout(), DEFAULT_BLOCKING_TIMEOUT_MS);
        }
        assert!(!session.is_blocking());
        assert_eq!(session.timeout(), 500);
    }

    #[test]
    fn guard_with_zero_timeout_means_unbounded() {
        let session = ssh2::Session::new().unwrap();
        let _g = SessionBlockingGuard::with_timeout(&session, 0);
        assert!(session.is_blocking());
        assert_eq!(session.timeout(), 0);
    }

    // 回归：远端不回应时，阻塞模式调用必须在超时后报错返回，而不是永久挂住调用线程。
    #[test]
    fn blocking_call_under_guard_times_out_instead_of_hanging() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (_server_side, _) = listener.accept().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut session = ssh2::Session::new().unwrap();
            session.set_tcp_stream(client);
            let started = Instant::now();
            let res = {
                let _g = SessionBlockingGuard::with_timeout(&session, 300);
                session.handshake()
            };
            let _ = tx.send((res.is_err(), started.elapsed()));
        });
        let (errored, elapsed) = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("blocking call hung despite SessionBlockingGuard timeout");
        assert!(errored);
        assert!(elapsed < Duration::from_secs(3), "took {elapsed:?}");
    }
}
