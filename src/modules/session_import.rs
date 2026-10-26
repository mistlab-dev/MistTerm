//! 会话导入模块（#122）：Xshell / FinalShell / ssh_config 导入入口的模块化包装。
//!
//! **纯工具模块**：不实现输出钩子（`on_output_line` 等空默认），验证 registry
//! 「不只能挂输出钩子」。导入逻辑本体留在 `core::foreign_import` /
//! `core::ssh_config_importer`（行为字节级不动）；本模块只负责：
//! - 稳定 id 注册（`session-import`），供设置开关与菜单 gating；
//! - Host 判断「导入入口是否可用」走 [`SessionImportModule::import_enabled`]。
//!
//! 模块关闭 → Host 隐藏/禁用导入菜单项；**已导入会话数据保留**（不删用户数据）。

use crate::capability::{MistModule, ModuleId};

pub const MODULE_ID: ModuleId = "session-import";

/// 会话导入能力模块（无钩子纯工具模块，#122）。
pub struct SessionImportModule;

impl Default for SessionImportModule {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionImportModule {
    pub fn new() -> Self {
        Self
    }

    /// Host 菜单/入口 gating：模块启用时才暴露「导入」用户入口。
    pub fn import_enabled(reg: &crate::capability::ModuleRegistry) -> bool {
        reg.is_enabled(MODULE_ID)
    }
}

impl MistModule for SessionImportModule {
    fn id(&self) -> ModuleId {
        MODULE_ID
    }

    fn name_zh(&self) -> &str {
        "会话导入"
    }

    fn name_en(&self) -> &str {
        "Session Import"
    }

    // 纯工具模块：5 钩子全部空默认实现（#122 明确不要求输出钩子）。
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::ModuleRegistry;

    #[test]
    fn module_id_and_default_on() {
        let reg = ModuleRegistry::with_modules(vec![Box::new(SessionImportModule::new())]);
        let info = reg.snapshot();
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].id, MODULE_ID);
        assert_eq!(info[0].name_zh, "会话导入");
        assert!(info[0].enabled);
        assert!(info[0].enabled_by_default);
    }

    #[test]
    fn import_enabled_gates_on_registry() {
        let reg = ModuleRegistry::with_modules(vec![Box::new(SessionImportModule::new())]);
        assert!(SessionImportModule::import_enabled(&reg));

        reg.set_enabled(MODULE_ID, false);
        assert!(!SessionImportModule::import_enabled(&reg));
    }

    #[test]
    fn pure_tool_module_no_output_hooks() {
        // 无钩子模块：行事件与 before_send 均不产出任何动作/拦截。
        let reg = ModuleRegistry::with_modules(vec![Box::new(SessionImportModule::new())]);
        let actions = reg.on_output_line(&crate::capability::OutputLineCtx::pty("ECONNREFUSED"));
        assert!(actions.is_empty());
        assert_eq!(
            reg.on_before_send(&crate::capability::BeforeSendCtx { text: "rm -rf /" }),
            crate::capability::SendVerdict::Allow
        );
    }
}
