//! 内置 MistModule 实现（#113）。
//!
//! `capability` 只放 trait 与 registry，不反向依赖具体模块，避免环。
//! Host 用 [`builtin_modules`] 组装官方清单。新模块追加在 [`builtin_modules_with_output_trigger`]。

pub mod cmd_audit_local;
pub mod output_trigger;
pub mod display_link;
pub mod session_import;
#[cfg(test)]
pub mod conformance;

use std::sync::{Arc, Mutex};

use crate::capability::MistModule;
use crate::core::CmdAuditEngine;

/// 官方内置模块。测试用内置输出触发规则，不读本机配置目录。
///
/// `cmd_audit_engine` 只交给 `cmd-audit-local`。output-trigger 与 display-link
/// 不读这台引擎，参数仍要穿过工厂，避免再拆一个无参注册入口。
/// 新模块追加在 [`builtin_modules_with_output_trigger`]。
pub fn builtin_modules(cmd_audit_engine: Arc<Mutex<CmdAuditEngine>>) -> Vec<Box<dyn MistModule>> {
    builtin_modules_with_output_trigger(
        cmd_audit_engine,
        output_trigger::OutputTriggerStore::builtin_ephemeral(),
    )
}

/// 与 [`builtin_modules`] 同一份清单，但输出触发规则与 UI 共享。
/// 测试仍走 [`builtin_modules`]，以免读到本机配置目录里的用户规则。
pub fn builtin_modules_with_output_trigger(
    cmd_audit_engine: Arc<Mutex<CmdAuditEngine>>,
    output_trigger_rules: output_trigger::OutputTriggerStore,
) -> Vec<Box<dyn MistModule>> {
    vec![
        Box::new(cmd_audit_local::CmdAuditLocalModule::new(cmd_audit_engine)),
        Box::new(output_trigger::OutputTriggerModule::with_store(
            output_trigger_rules,
        )),
        Box::new(display_link::DisplayLinkModule::new()),
        Box::new(session_import::SessionImportModule::new()),
    ]
}

/// 测试辅助（#123）：当前已注册的官方模块 id 列表。
#[cfg(test)]
pub fn builtin_modules_names() -> Vec<&'static str> {
    vec![
        cmd_audit_local::MODULE_ID,
        output_trigger::MODULE_ID,
        display_link::MODULE_ID,
        session_import::MODULE_ID,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{ModuleAction, ModuleRegistry, OutputLineCtx, Severity};

    #[test]
    fn builtin_modules_registers_audit_trigger_and_display_link() {
        let engine = Arc::new(Mutex::new(CmdAuditEngine::new()));
        let reg = ModuleRegistry::with_modules(builtin_modules(engine));
        let ids: Vec<String> = reg.snapshot().into_iter().map(|m| m.id).collect();
        assert_eq!(
            ids,
            vec![
                cmd_audit_local::MODULE_ID.to_string(),
                output_trigger::MODULE_ID.to_string(),
                display_link::MODULE_ID.to_string(),
                session_import::MODULE_ID.to_string(),
            ]
        );
        assert!(reg.is_enabled(cmd_audit_local::MODULE_ID));
        assert!(reg.is_enabled(output_trigger::MODULE_ID));
        assert!(reg.is_enabled(display_link::MODULE_ID));
        assert!(reg.is_enabled(session_import::MODULE_ID));
        assert!(ModuleRegistry::builtin().snapshot().is_empty());

        // PTY 行只出 Notify。display-link 不在这条来源上装饰。
        let actions = reg.on_output_line(&OutputLineCtx::pty("ECONNREFUSED"));
        assert!(matches!(
            actions.as_slice(),
            [ModuleAction::Notify {
                severity: Severity::Error,
                dedupe_key,
                attention: true,
                ..
            }] if dedupe_key == "TR-ERR-001"
        ));

        // 只关 output-trigger 时，另外两个仍在；trigger 不再产出 Notify。
        reg.sync_effective_disabled(&[output_trigger::MODULE_ID.to_string()]);
        assert!(!reg.is_enabled(output_trigger::MODULE_ID));
        assert!(reg.is_enabled(cmd_audit_local::MODULE_ID));
        assert!(reg.is_enabled(display_link::MODULE_ID));
        assert!(reg
            .on_output_line(&OutputLineCtx::pty("ECONNREFUSED"))
            .is_empty());
    }
}
