//! SSH 层 - 负责 SSH 连接和通信
//!
//! **三种独立的「传文件」方式**（入口与实现互不合并）：
//! 1. **ZMODEM / lrzsz**：终端里 `rz`/`sz` 与 `LrzszTransfer` + 专用 shell 泵线程 `ZmodemWrite`（`sync_channel`）；收发协议均由 `zmodem2` 状态机实现。
//! 2. **SFTP**：侧栏 SFTP 面板，独立会话/逻辑（见 UI）。
//! 3. **直传**：`TerminalView::start_upload`（当前实现为 SCP）、`start_upload_to_remote`（`cat >`）等，不经 ZMODEM。

mod client;
mod user_facing;
mod manager;
mod lrzsz;
mod lrzsz_zmodem2_send;
mod lrzsz_external_sz;
mod zmodem_pty_pipeline;
mod file_transfer;
pub mod zmodem_pty_prefix;
pub mod sftp;

pub use client::SshConfig;
pub use user_facing::format_ssh_connect_error;
pub use sftp::{SftpClient, SftpEntry};
pub use manager::{SshManager, SshMessage, SshSessionHandle, SshSessionId};
pub use lrzsz::{LrzszTransfer, TransferEvent};
pub use file_transfer::{FileTransfer, ProgressTracker};
