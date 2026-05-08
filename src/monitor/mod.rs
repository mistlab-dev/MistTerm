//! 服务器监控模块
//!
//! 通过 SSH 远程执行命令采集服务器资源状态

mod collector;
mod parser;

pub use collector::{Monitor, ServerStats};