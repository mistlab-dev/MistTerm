//! 内置 MistModule 实现（#113）。
//!
//! `capability` 只放 trait 与 registry，不反向依赖具体模块，避免环。
//! Host 用 [`builtin_modules`] 组装官方清单。

pub mod cmd_audit_local;
pub mod output_trigger;

use std::sync::{Arc, Mutex};

use crate::capability::MistModule;
use crate::core::CmdAuditEngine;

/// 官方内置模块。迁入新模块时只改这里，Host 仍走同一 registry。
pub fn builtin_modules(cmd_audit_engine: Arc<Mutex<CmdAuditEngine>>) -> Vec<Box<dyn MistModule>> {
    vec![
        Box::new(cmd_audit_local::CmdAuditLocalModule::new(cmd_audit_engine)),
        Box::new(output_trigger::OutputTriggerModule::new()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{ModuleAction, ModuleRegistry, OutputLineCtx, Severity};

    #[test]
    fn builtin_modules_registers_audit_and_output_trigger() {
        let engine = Arc::new(Mutex::new(CmdAuditEngine::new()));
        let reg = ModuleRegistry::with_modules(builtin_modules(engine));
        let snap = reg.snapshot();
        let trigger = snap
            .iter()
            .find(|m| m.id == output_trigger::MODULE_ID)
            .expect("output-trigger");
        assert!(trigger.enabled);
        assert!(trigger.enabled_by_default);
        assert!(snap
            .iter()
            .any(|m| m.id == cmd_audit_local::MODULE_ID && m.enabled));

        let actions = reg.on_output_line(&OutputLineCtx {
            line: "ECONNREFUSED",
        });
        assert!(matches!(
            actions.as_slice(),
            [ModuleAction::Notify {
                severity: Severity::Error,
                dedupe_key,
                attention: true,
                ..
            }] if dedupe_key == "TR-ERR-001"
        ));

        // 只关 output-trigger 时，cmd-audit-local 仍在；trigger 不再产出 Notify。
        reg.sync_effective_disabled(&[output_trigger::MODULE_ID.to_string()]);
        assert!(!reg.is_enabled(output_trigger::MODULE_ID));
        assert!(reg.is_enabled(cmd_audit_local::MODULE_ID));
        assert!(reg
            .on_output_line(&OutputLineCtx {
                line: "ECONNREFUSED",
            })
            .is_empty());
    }
}
