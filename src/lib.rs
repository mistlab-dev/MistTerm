//! MistTerm 库入口：供集成测试与二进制共用同一套模块。
//!
//! GUI（`src/main.rs`）与 CLI（`src/bin/mist.rs`）仅作入口，模块树只维护于此。

pub mod cli;
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
