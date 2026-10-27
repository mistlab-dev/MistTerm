//! 终端层 - 终端模拟与 ANSI/VT100 解析

mod alacritty;
mod command_block;
pub mod style;

pub use alacritty::{BlockNav, CopyBlock, NavDir, SearchHit, Terminal, ViewportCursor};
pub use style::TerminalShellStyle;
