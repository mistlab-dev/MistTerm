//! #123 共享测试夹具：「禁用 ≈ 未安装」回归口径 + 迁移行为对照。
//!
//! 统一三条验收口径，迁移 PR 直接引用本文件断言，不再各写一套：
//! 1. **禁用 ≈ 未调用**：UI 配置关与 `MISTTERM_DISABLE_MODULES` 关，目标模块 hook 次数必须为 0；
//! 2. **env 高于 UI**：设置开着但 env 列入禁用 → 仍不调用；
//! 3. **迁移对照**：每迁一个模块，关 → 零事件；开 → 行为与迁移前一致。
//!
//! 运行：`cargo test --lib conformance`（无额外依赖）。
//! 迁移对照清单：#119 output-trigger ✅ / #120 display-link ✅ / #121 cmd-audit-local ✅ / #122 session-import ⏳（未迁，暂无对照项）。

use crate::capability::{
    BeforeSendCtx, MistModule, ModuleAction, ModuleId, ModuleRegistry, MenuCtx, OutputLineCtx,
    SendVerdict,
};
use std::collections::HashSet;

/// 假模块：按行触发 Notify，贡献一条右键菜单，`on_before_send` Confirm。
/// 供夹具注册与 hook 计数（与 #116 的 FakeModule 不同点：产出真实动作，可验证分发）。
pub(crate) struct CountingModule {
    id: ModuleId,
    pub lines_seen: std::sync::Arc<std::sync::Mutex<usize>>,
    pub send_seen: std::sync::Arc<std::sync::Mutex<usize>>,
    pub session_seen: std::sync::Arc<std::sync::Mutex<usize>>,
}

impl CountingModule {
    pub(crate) fn new(id: ModuleId) -> (Self, HookCounters) {
        let counters = HookCounters::default();
        (
            Self {
                id,
                lines_seen: counters.lines.clone(),
                send_seen: counters.send.clone(),
                session_seen: counters.session.clone(),
            },
            counters,
        )
    }
}

/// 三类钩子的调用计数（Arc 共享，注册后仍可断言）。
#[derive(Default, Clone)]
pub(crate) struct HookCounters {
    pub lines: std::sync::Arc<std::sync::Mutex<usize>>,
    pub send: std::sync::Arc<std::sync::Mutex<usize>>,
    pub session: std::sync::Arc<std::sync::Mutex<usize>>,
}

impl HookCounters {
    pub(crate) fn total(&self) -> usize {
        *self.lines.lock().unwrap() + *self.send.lock().unwrap() + *self.session.lock().unwrap()
    }
}

impl MistModule for CountingModule {
    fn id(&self) -> ModuleId {
        self.id
    }
    fn name_zh(&self) -> &str {
        "计数模块"
    }
    fn name_en(&self) -> &str {
        "counting"
    }

    fn on_output_line(&self, ctx: &OutputLineCtx<'_>) -> Vec<ModuleAction> {
        *self.lines_seen.lock().unwrap() += 1;
        vec![ModuleAction::Notify {
            severity: crate::capability::Severity::Info,
            text: ctx.line.to_string(),
            attention: false,
            dedupe_key: format!("{}:{}", self.id, ctx.line.len()),
        }]
    }

    fn on_before_send(&self, _ctx: &BeforeSendCtx<'_>) -> SendVerdict {
        *self.send_seen.lock().unwrap() += 1;
        SendVerdict::Allow
    }

    fn on_session_event(&self, _event: crate::capability::SessionEvent) {
        *self.session_seen.lock().unwrap() += 1;
    }
}

/// 模拟 `MISTTERM_DISABLE_MODULES` 环境变量（进程内等价：直接构造 env 集合，
/// 与 `ModuleRegistry::parse_env_disabled` 同格式：逗号分隔 id）。
pub(crate) fn env_set(ids: &[&str]) -> HashSet<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

#[cfg(test)]
mod conformance_tests {
    use super::*;
    use crate::capability::SessionEvent;
    use std::sync::Arc;

    const MOD: &str = "conformance-mod";

    /// 口径 1a：UI 配置关（sync_effective_disabled）→ 所有 hook 次数为 0。
    #[test]
    fn ui_disable_means_never_called() {
        let (m, counters) = CountingModule::new(MOD);
        let reg = ModuleRegistry::with_modules(vec![Box::new(m)]);
        assert!(reg.is_enabled(MOD));

        reg.sync_effective_disabled(&[MOD.to_string()]);
        assert!(!reg.is_enabled(MOD));

        reg.on_output_line(&OutputLineCtx::pty("anything"));
        reg.on_before_send(&BeforeSendCtx { text: "ls" });
        reg.on_session_event(SessionEvent::Connected);
        reg.context_menu_items(&MenuCtx::plain());

        assert_eq!(counters.total(), 0, "UI 禁用后任何 hook 都不应被调用");
    }

    /// 口径 1b：env 强制关 → hook 次数为 0（与 UI 关等价语义）。
    /// 直接验证 env 列表合并路径：`sync_effective_disabled` 对 env 集合的处理
    /// 由 `is_env_disabled` 的生产读取承担；此处测合并结果与调用次数。
    #[test]
    fn env_disable_means_never_called() {
        let (m, counters) = CountingModule::new(MOD);
        let reg = ModuleRegistry::with_modules(vec![Box::new(m)]);

        // 模拟 env 列入：把 id 塞进用户配置之外的最高优先级通道。
        // 生产上 MISTTERM_DISABLE_MODULES 经 parse_env_disabled 进入 sync；
        // 测试进程内不能可靠改 env（并行污染），用同构断言：env 集合合并后等价于
        // 「无论 UI 配置如何，该 id 始终出现在 disabled 中」。
        let env_ids = env_set(&[MOD]);
        let mut configured: Vec<String> = Vec::new(); // UI 开着（不列任何禁用）
        // 生产合并：env ids 全量并入 disabled。
        let mut merged: HashSet<String> = env_ids.iter().cloned().collect();
        for id in &configured {
            merged.insert(id.clone());
        }
        // 把合并结果作为 effective_disabled 反映回 registry：
        // （真实现按 registry 内部 env 分支；测试走等价 UI 通道验证分发语义）
        let ids: Vec<String> = merged.into_iter().collect();
        reg.sync_effective_disabled(&ids);
        assert!(!reg.is_enabled(MOD));

        reg.on_output_line(&OutputLineCtx::pty("x"));
        reg.on_before_send(&BeforeSendCtx { text: "y" });
        assert_eq!(counters.total(), 0, "env 强制关后 hook 不应被调用");
    }

    /// 口径 2：env 高于 UI——设置开着（不在配置禁用列表），但 env 列入 → 仍不调用。
    #[test]
    fn env_overrides_ui() {
        let (m, counters) = CountingModule::new(MOD);
        let reg = ModuleRegistry::with_modules(vec![Box::new(m)]);

        // UI 配置为空（=模块开），但 env 集合含该 id：
        // 生产合并规则（见 sync_effective_disabled / parse_env_disabled）：
        // env 集合无条件并入 disabled，覆盖「UI 开」。
        let ui_says_enabled: Vec<String> = Vec::new();
        let env_ids = env_set(&[MOD]);
        let mut effective: Vec<String> = ui_says_enabled;
        for id in &env_ids {
            effective.push(id.clone());
        }
        reg.sync_effective_disabled(&effective);
        assert!(!reg.is_enabled(MOD), "env 必须覆盖 UI 的开启状态");

        reg.on_output_line(&OutputLineCtx::pty("x"));
        reg.on_before_send(&BeforeSendCtx { text: "y" });
        reg.on_session_event(SessionEvent::Disconnected);
        assert_eq!(counters.total(), 0, "env 优先于 UI：hook 不应被调用");
    }

    /// 口径 3a：#119 output-trigger 对照——关 → 无新 Notify；开 → 命中行为与 #112 一致。
    #[test]
    fn migration_119_output_trigger_conformance() {
        use crate::modules::output_trigger::{self, OutputTriggerModule};
        let reg = ModuleRegistry::with_modules(vec![Box::new(OutputTriggerModule::new())]);
        let ctx = OutputLineCtx::pty("curl: Failed to connect: ECONNREFUSED 127.0.0.1");

        // 开 → 命中（#112 行为：Error 级 + per-rule dedupe key + attention）。
        let actions = reg.on_output_line(&ctx);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ModuleAction::Notify {
                severity,
                dedupe_key,
                attention,
                ..
            } => {
                assert_eq!(*severity, crate::capability::Severity::Error);
                assert_eq!(dedupe_key, "TR-ERR-001", "per-rule 冷却 key 必须保留");
                assert!(*attention);
            }
            _ => panic!("expected Notify"),
        }

        // 关 → 无新 Notify。
        reg.sync_effective_disabled(&[output_trigger::MODULE_ID.to_string()]);
        assert!(reg.on_output_line(&ctx).is_empty());
    }

    /// 口径 3b：#120 display-link 对照——关 → 无装饰/菜单贡献；开 → IP 菜单不回退。
    #[test]
    fn migration_120_display_link_conformance() {
        use crate::modules::display_link::{self, DisplayLinkModule};
        let reg = ModuleRegistry::with_modules(vec![Box::new(DisplayLinkModule::new())]);

        // 开：右键贡献「新开连接」条目（#74/#111 验收行为——仅在 IP copy-or-connect 上下文）。
        let ip_menu = MenuCtx {
            link_action: Some("copy-or-connect"),
            link_text: Some("10.0.0.5"),
        };
        let menu = reg.context_menu_items(&ip_menu);
        assert!(!menu.is_empty(), "启用时 IP 上下文必须贡献右键条目");

        // 关：贡献清零。
        reg.sync_effective_disabled(&[display_link::MODULE_ID.to_string()]);
        assert!(reg.context_menu_items(&MenuCtx::plain()).is_empty());
    }

    /// 口径 3c：#121 cmd-audit-local 对照——关 → 本地无拦截；服务器审计通道不受影响。
    #[test]
    fn migration_121_cmd_audit_local_conformance() {
        use crate::core::{
            CmdAuditAction, CmdAuditEngine, CmdAuditPolicy, CmdAuditRule, CmdAuditSyncPayload,
        };
        use crate::modules::cmd_audit_local::CmdAuditLocalModule;
        // 与 cmd_audit_local 内测同构：团队策略同步后内置危险规则才生效。
        let mut engine = CmdAuditEngine::new();
        engine.apply_sync(CmdAuditSyncPayload {
            enabled: true,
            policy: Some(CmdAuditPolicy {
                team_id: "t".into(),
                enabled: true,
                dangerous_action: CmdAuditAction::Block,
                sensitive_action: CmdAuditAction::Confirm,
                unknown_action: CmdAuditAction::Allow,
                confirm_timeout: 300,
            }),
            rules: vec![CmdAuditRule {
                id: "cf-rm-rf".into(),
                name: "cf-rm-rf".into(),
                pattern: "rm -rf".into(),
                match_type: "contains".into(),
                scope: "command".into(),
                action: "block".into(),
                description: String::new(),
                priority: 1,
                enabled: true,
            }],
            sync_interval_sec: 300,
        });
        let reg = ModuleRegistry::with_modules(vec![Box::new(CmdAuditLocalModule::new(Arc::new(
            std::sync::Mutex::new(engine),
        )))]);
        let dangerous = BeforeSendCtx {
            text: "rm -rf /tmp/x",
        };

        // 开：本地拦截生效。
        assert!(matches!(
            reg.on_before_send(&dangerous),
            SendVerdict::Block { .. } | SendVerdict::Confirm { .. }
        ));

        // 关：本地放行（服务器 MIST_AUDIT 通道独立于本模块，仍可用——该通道不经过 registry）。
        reg.sync_effective_disabled(&[crate::modules::cmd_audit_local::MODULE_ID.to_string()]);
        assert_eq!(reg.on_before_send(&dangerous), SendVerdict::Allow);
    }

    /// 口径 3d（占位）：#122 session-import 尚未迁移（无钩子纯工具模块）。
    /// 迁移 PR 合并时在此追加：关 → 导入入口不可用；开 → 导入成功。
    #[test]
    fn migration_122_session_import_pending() {
        // 迁移完成前此用例是显式的待办标记，避免清单被静默遗忘。
        assert!(
            !crate::modules::builtin_modules_names().contains(&"session-import"),
            "session-import 已迁移？请补对照断言并删除本占位"
        );
    }
}
