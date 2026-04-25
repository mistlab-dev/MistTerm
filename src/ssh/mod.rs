//! SSH 层 - 负责 SSH 连接和通信

mod client;
mod manager;
mod lrzsz;

pub use client::SshConfig;
pub use manager::{SshManager, SshMessage, SshSessionHandle};
pub use lrzsz::{LrzszTransfer, TransferEvent};
