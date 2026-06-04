//! MistTerm 库入口：供集成测试与二进制共用同一套模块。
//!
//! 与 `main.rs` 中的模块树保持一致。

pub mod core;
pub mod i18n;
pub mod platform;
pub mod ssh;
pub mod terminal;
pub mod ui;
pub mod security;
pub mod monitor;

#[doc(hidden)]
pub mod test_support;
