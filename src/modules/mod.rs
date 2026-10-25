//! 内置 MistModule 实现（#113）。
//!
//! `capability` 只放 trait 与 registry，不反向依赖具体模块，避免环。
//! Host 用 [`builtin_modules`] 组装官方清单。

pub mod cmd_audit_local;

use std::sync::{Arc, Mutex};

use crate::capability::MistModule;
use crate::core::CmdAuditEngine;

/// 官方内置模块。迁入新模块时只改这里，Host 仍走同一 registry。
pub fn builtin_modules(cmd_audit_engine: Arc<Mutex<CmdAuditEngine>>) -> Vec<Box<dyn MistModule>> {
    vec![Box::new(cmd_audit_local::CmdAuditLocalModule::new(
        cmd_audit_engine,
    ))]
}
