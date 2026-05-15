//! 终端层 - 终端模拟与 ANSI/VT100 解析

mod alacritty;
pub mod style;

pub use alacritty::Terminal;
pub use style::TerminalShellStyle;
