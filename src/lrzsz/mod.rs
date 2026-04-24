//! lrzsz 协议实现模块
//!
//! 支持 ZMODEM/XMODEM 协议，实现终端内文件传输

mod detector;
mod zmodem;
mod transfer;

pub use detector::{LrzszDetector, LrzszEvent};
pub use zmodem::{ZmodemTransfer, ZmodemState, TransferProgress, FileInfo};
pub use transfer::TransferManager;
