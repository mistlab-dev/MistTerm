//! SSH 层 - 负责 SSH 连接和通信
//!
//! **三种独立的「传文件」方式**（入口与实现互不合并）：
//! 1. **ZMODEM / lrzsz**：终端里 `rz`/`sz` 与 `LrzszTransfer` + 专用 shell 泵线程 `ZmodemWrite`（`sync_channel`）；收发协议均由 `zmodem2` 状态机实现。
//! 2. **SFTP**：侧栏 SFTP 面板，独立会话/逻辑（见 UI）。
//! 3. **直传**：`TerminalView::start_upload`（当前实现为 SCP）、`start_upload_to_remote`（`cat >`）等，不经 ZMODEM。

/// RAII guard：临时将 libssh2 `Session` 切到阻塞模式，drop 时无条件恢复非阻塞。
///
/// libssh2 的 `set_blocking` 是 **Session 级别全局设置**，会影响该 Session 上所有 channel（包括
/// shell_pump 正在跑的 PTY channel）。任何临时切阻塞的操作都**必须**在退出前切回非阻塞，否则
/// shell_pump 线程会永久阻塞在 `channel.read()` 上（终端卡死、菜单能动但输入不响应）。
///
/// 用法：
/// ```ignore
/// let _g = SessionBlockingGuard::new(&session);
/// // ... 阻塞读写操作，无论中间是 ? 提前返回、panic、还是正常结束，drop 都会自动恢复
/// ```
pub struct SessionBlockingGuard {
    /// 用 raw pointer 而非 `&'a Session` 持有，避免与后续 `&mut` / 重新赋值冲突。
    /// 本项目中调用 guard 的 Session 均在当前线程独占，无悬垂风险。
    session: std::ptr::NonNull<ssh2::Session>,
}

impl SessionBlockingGuard {
    pub fn new(session: &ssh2::Session) -> Self {
        session.set_blocking(true);
        Self {
            session: std::ptr::NonNull::from(session),
        }
    }
}

impl Drop for SessionBlockingGuard {
    fn drop(&mut self) {
        // SAFETY: guard 生命周期不跨出 Session 本身，调用期间 Session 肯定有效。
        unsafe {
            self.session.as_ref().set_blocking(false);
        }
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
pub mod zmodem_pty_prefix;
pub mod sftp;

pub use client::{SshClient, SshConfig};
pub use port_forward::{ForwardControl, LocalPortForward, RemotePortForward};
pub use socks_proxy::DynamicPortForward;
pub use jump::{JumpHop, parse_jump_chain, parse_jump_endpoint};
pub use user_facing::format_ssh_connect_error;
pub use sftp::{SftpClient, SftpEntry};
pub use manager::{SshManager, SshMessage, SshSessionHandle, SshSessionId};
pub use lrzsz::{LrzszTransfer, TransferEvent};
pub use file_transfer::{FileTransfer, ProgressTracker};
