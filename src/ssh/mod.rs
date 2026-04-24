//! SSH 层 - 负责 SSH 连接和通信

mod client;
mod manager;

pub use client::{SshClient, SshConfig};
pub use manager::{SshManager, SshMessage, SshSessionId, SshSessionHandle};
