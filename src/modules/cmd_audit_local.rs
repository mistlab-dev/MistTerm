//! 本地命令检查模块 `cmd-audit-local`（#121 / EXTENSION-POINTS R4）。
//!
//! 规则仍由 [`CmdAuditEngine`] 读取 `assets/cmd-audit-patterns/*`，再叠加团队同步的
//! 策略与自定义规则。本模块只把「即将写入 PTY」的本地判定接到 `on_before_send`。
//!
//! 不解析、不吞掉服务器 `MIST_AUDIT` 通道。模块关闭后本地规则不再拦截 / 确认 / 告警；
//! 服务器回传的审计事件仍由 Host 原路径处理。
//!
//! `Alert` 不是门闩：钩子返回 [`SendVerdict::Allow`]，Host 再用引擎结果记一条告警后放行
//! （与迁入前一致）。

use std::sync::{Arc, Mutex};

use crate::capability::{BeforeSendCtx, MistModule, ModuleId, SendVerdict};
use crate::core::cmd_audit::{CmdAuditAction, CmdAuditEngine, CmdAuditMatch, CmdAuditResult};

/// 偏好设置与 `MISTTERM_DISABLE_MODULES` 使用的稳定 id。
pub const MODULE_ID: &str = "cmd-audit-local";

/// 与 Host 共享同一台引擎，这样团队同步（`apply_sync` / `apply_cached`）立刻作用到钩子。
pub struct CmdAuditLocalModule {
    engine: Arc<Mutex<CmdAuditEngine>>,
}

impl CmdAuditLocalModule {
    pub fn new(engine: Arc<Mutex<CmdAuditEngine>>) -> Self {
        Self { engine }
    }
}

impl MistModule for CmdAuditLocalModule {
    fn id(&self) -> ModuleId {
        MODULE_ID
    }

    fn name_zh(&self) -> &str {
        "本地命令检查"
    }

    fn name_en(&self) -> &str {
        "Local command check"
    }

    fn on_before_send(&self, ctx: &BeforeSendCtx<'_>) -> SendVerdict {
        let engine = self.engine.lock().unwrap_or_else(|e| e.into_inner());
        let audit = engine.check(ctx.text);
        match audit.action {
            CmdAuditAction::Block => SendVerdict::Block {
                reason: reason_of(&audit),
            },
            CmdAuditAction::Confirm => SendVerdict::Confirm {
                reason: reason_of(&audit),
            },
            // Alert 放行，由 Host 用引擎结果补记告警。
            CmdAuditAction::Alert | CmdAuditAction::Allow => SendVerdict::Allow,
        }
    }
}

/// 策略未生效、或模块关闭时的本地结果（不拦截）。
pub(crate) fn allow_audit() -> CmdAuditResult {
    CmdAuditResult {
        allowed: true,
        action: CmdAuditAction::Allow,
        matches: Vec::new(),
    }
}

/// 手敲回车是否要先扣住 `\r`。模块关闭时与「策略未生效」一样，回车照旧直接发出。
pub(crate) fn typed_enter_should_gate(module_enabled: bool, engine_active: bool) -> bool {
    module_enabled && engine_active
}

/// 把钩子裁决与本地引擎结果合成 Host 继续用的 [`CmdAuditResult`]。
///
/// - 模块关闭：忽略 `engine_audit`（即使引擎自己会 Block），除非别的模块给出更严的裁决。
/// - 模块开启：保留引擎的匹配详情（时间线、确认框、告警文案）。`Alert` 随 Allow 裁决留下。
/// - 其它模块的 Block / Confirm 只收紧，不放宽本地结果。
pub(crate) fn decide_local_send(
    verdict: SendVerdict,
    module_enabled: bool,
    engine_audit: CmdAuditResult,
) -> CmdAuditResult {
    let local = if module_enabled {
        engine_audit
    } else {
        allow_audit()
    };
    match verdict {
        SendVerdict::Block { reason } => {
            if local.action == CmdAuditAction::Block {
                local
            } else {
                synthetic(CmdAuditAction::Block, reason)
            }
        }
        SendVerdict::Confirm { reason } => {
            if matches!(
                local.action,
                CmdAuditAction::Block | CmdAuditAction::Confirm
            ) {
                local
            } else {
                synthetic(CmdAuditAction::Confirm, reason)
            }
        }
        SendVerdict::Allow => local,
    }
}

fn reason_of(audit: &CmdAuditResult) -> String {
    audit
        .matches
        .first()
        .map(|m| m.message.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| fallback_reason(audit.action))
}

fn fallback_reason(action: CmdAuditAction) -> String {
    match action {
        CmdAuditAction::Block => "本地检查：该命令被禁止".into(),
        CmdAuditAction::Confirm => "本地检查：该命令需要确认".into(),
        _ => String::new(),
    }
}

fn synthetic(action: CmdAuditAction, reason: String) -> CmdAuditResult {
    let reason = if reason.trim().is_empty() {
        fallback_reason(action)
    } else {
        reason
    };
    CmdAuditResult {
        allowed: matches!(action, CmdAuditAction::Allow | CmdAuditAction::Alert),
        action,
        matches: vec![CmdAuditMatch {
            rule_id: "module".into(),
            name: reason.clone(),
            source: "module".into(),
            level: match action {
                CmdAuditAction::Block => "block",
                CmdAuditAction::Confirm => "confirm",
                CmdAuditAction::Alert => "alert",
                CmdAuditAction::Allow => "allow",
            }
            .into(),
            message: reason,
            action,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{
        BeforeSendCtx, MistModule, ModuleRegistry, OutputLineCtx, SendVerdict,
    };
    use crate::core::cmd_audit::{
        CmdAuditAction, CmdAuditEngine, CmdAuditPolicy, CmdAuditRule, CmdAuditSyncPayload,
        ServerAuditProbe,
    };
    use std::sync::{Arc, Mutex};

    fn policy_engine(
        dangerous: CmdAuditAction,
        sensitive: CmdAuditAction,
        unknown: CmdAuditAction,
        rules: Vec<CmdAuditRule>,
    ) -> CmdAuditEngine {
        let mut engine = CmdAuditEngine::new();
        engine.apply_sync(CmdAuditSyncPayload {
            enabled: true,
            policy: Some(CmdAuditPolicy {
                team_id: "t".into(),
                enabled: true,
                dangerous_action: dangerous,
                sensitive_action: sensitive,
                unknown_action: unknown,
                confirm_timeout: 300,
            }),
            rules,
            sync_interval_sec: 300,
        });
        engine
    }

    fn rule(id: &str, pattern: &str, action: &str) -> CmdAuditRule {
        CmdAuditRule {
            id: id.into(),
            name: id.into(),
            pattern: pattern.into(),
            match_type: "contains".into(),
            scope: "command".into(),
            action: action.into(),
            description: id.into(),
            priority: 1,
            enabled: true,
        }
    }

    fn registered(engine: Arc<Mutex<CmdAuditEngine>>) -> ModuleRegistry {
        ModuleRegistry::with_modules(vec![Box::new(CmdAuditLocalModule::new(engine))])
    }

    #[test]
    fn module_id_and_default_on() {
        let engine = Arc::new(Mutex::new(CmdAuditEngine::new()));
        let reg = registered(engine);
        let info = reg.snapshot();
        assert_eq!(info.len(), 1);
        assert_eq!(info[0].id, "cmd-audit-local");
        assert_eq!(info[0].name_zh, "本地命令检查");
        assert!(info[0].enabled);
        assert!(info[0].enabled_by_default);
    }

    #[test]
    fn inactive_engine_does_not_intercept() {
        // 未同步团队策略时与迁入前一样：内置规则包在，但不拦截。
        let engine = Arc::new(Mutex::new(CmdAuditEngine::new()));
        let reg = registered(engine);
        assert_eq!(
            reg.on_before_send(&BeforeSendCtx { text: "rm -rf /" }),
            SendVerdict::Allow
        );
    }

    #[test]
    fn enabled_module_blocks_builtin_dangerous_pattern() {
        let engine = Arc::new(Mutex::new(policy_engine(
            CmdAuditAction::Block,
            CmdAuditAction::Confirm,
            CmdAuditAction::Allow,
            Vec::new(),
        )));
        let reg = registered(Arc::clone(&engine));
        let verdict = reg.on_before_send(&BeforeSendCtx { text: "rm -rf /" });
        match verdict {
            SendVerdict::Block { reason } => {
                assert!(!reason.is_empty(), "block reason should reach the host UX");
            }
            other => panic!("expected Block, got {other:?}"),
        }
        let audit = engine.lock().unwrap().check("rm -rf /");
        assert_eq!(audit.action, CmdAuditAction::Block);
        assert!(
            audit.matches.iter().any(|m| m.rule_id.contains("DEL-")),
            "still matches assets/cmd-audit-patterns, got {:?}",
            audit.matches
        );
        let decided = decide_local_send(
            reg.on_before_send(&BeforeSendCtx { text: "rm -rf /" }),
            true,
            audit,
        );
        assert_eq!(decided.action, CmdAuditAction::Block);
        assert!(!decided.matches.is_empty());
    }

    #[test]
    fn confirm_and_alert_keep_previous_semantics() {
        let engine = Arc::new(Mutex::new(policy_engine(
            CmdAuditAction::Block,
            CmdAuditAction::Confirm,
            CmdAuditAction::Allow,
            vec![
                rule("c1", "need-confirm", "confirm"),
                rule("a1", "just-alert", "alert"),
            ],
        )));
        let reg = registered(Arc::clone(&engine));

        let confirm = reg.on_before_send(&BeforeSendCtx {
            text: "need-confirm now",
        });
        assert!(matches!(confirm, SendVerdict::Confirm { .. }));
        let confirm_audit = engine.lock().unwrap().check("need-confirm now");
        assert_eq!(confirm_audit.action, CmdAuditAction::Confirm);
        assert_eq!(
            decide_local_send(confirm, true, confirm_audit).action,
            CmdAuditAction::Confirm
        );

        let alert = reg.on_before_send(&BeforeSendCtx {
            text: "just-alert now",
        });
        assert_eq!(alert, SendVerdict::Allow);
        let alert_audit = engine.lock().unwrap().check("just-alert now");
        assert_eq!(alert_audit.action, CmdAuditAction::Alert);
        let decided = decide_local_send(SendVerdict::Allow, true, alert_audit);
        assert_eq!(decided.action, CmdAuditAction::Alert);
        assert!(decided.allowed);
    }

    #[test]
    fn disabled_module_skips_local_rules_but_server_probe_still_parses() {
        let engine = Arc::new(Mutex::new(policy_engine(
            CmdAuditAction::Block,
            CmdAuditAction::Confirm,
            CmdAuditAction::Allow,
            Vec::new(),
        )));
        let reg = registered(Arc::clone(&engine));
        reg.set_enabled(MODULE_ID, false);
        assert!(!reg.is_enabled(MODULE_ID));

        let verdict = reg.on_before_send(&BeforeSendCtx { text: "rm -rf /" });
        assert_eq!(verdict, SendVerdict::Allow);

        // 引擎本身仍会 Block；Host 在模块关闭时不得拿它当门闩。
        let engine_audit = engine.lock().unwrap().check("rm -rf /");
        assert_eq!(engine_audit.action, CmdAuditAction::Block);
        let decided = decide_local_send(verdict, false, engine_audit);
        assert_eq!(decided.action, CmdAuditAction::Allow);
        assert!(decided.matches.is_empty());
        assert!(!typed_enter_should_gate(false, true));

        // 服务器通道独立：关掉本地模块不影响 MIST_AUDIT 解析。
        let mut probe = ServerAuditProbe::new();
        let (out, events) = probe.feed(
            b"ok\nMIST_AUDIT\t{\"action\":\"block\",\"message\":\"server-nope\",\"rule\":\"srv\",\"command\":\"rm -rf /\"}\n",
        );
        assert_eq!(out, b"ok\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, CmdAuditAction::Block);
        assert_eq!(events[0].message, "server-nope");
        assert_eq!(events[0].command, "rm -rf /");
    }

    #[test]
    fn other_module_verdict_only_tightens() {
        let allow = allow_audit();
        let blocked = decide_local_send(
            SendVerdict::Block {
                reason: "别的模块拦住了".into(),
            },
            true,
            allow.clone(),
        );
        assert_eq!(blocked.action, CmdAuditAction::Block);
        assert_eq!(blocked.matches[0].message, "别的模块拦住了");

        let confirmed = decide_local_send(
            SendVerdict::Confirm {
                reason: "请确认".into(),
            },
            false,
            allow,
        );
        assert_eq!(confirmed.action, CmdAuditAction::Confirm);
        assert_eq!(confirmed.matches[0].message, "请确认");
    }

    #[test]
    fn typed_enter_gate_follows_module_and_policy() {
        assert!(!typed_enter_should_gate(true, false));
        assert!(!typed_enter_should_gate(false, true));
        assert!(typed_enter_should_gate(true, true));
    }

    #[test]
    fn output_line_hook_stays_empty() {
        // 本地审计不占用 on_output_line，避免和服务器标记行抢输出。
        let module = CmdAuditLocalModule::new(Arc::new(Mutex::new(CmdAuditEngine::new())));
        assert!(module
            .on_output_line(&OutputLineCtx {
                line: "MIST_AUDIT\t{}",
            })
            .is_empty());
    }
}
