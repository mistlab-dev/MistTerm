//! `mist exec` / `mist frag run` 执行前的把关，和桌面版一致：
//!
//! - 团队命令策略（桌面版同步下来的缓存）：拦截的不执行；要求确认的需要确认；告警的照常执行并记录。
//! - 会改动服务器的命令、看不出是否只读的命令：需要确认（和桌面 AI 助手一样，只读命令才直接执行）。
//! - 确认方式：在终端里直接运行时会问一句；被脚本或 AI 助手调用（没有终端）时不执行，
//!   提示「确认后加 --yes 再运行」。拦截的命令加 --yes 也不执行。
//! - 每次执行、确认、拦截都写进审计日志（和桌面同一份），执行结果另记执行历史。

use std::io::{BufRead, IsTerminal, Write};

use crate::core::agent::{classify_command, gate_decision, CommandClass, GateLevel};
use crate::core::audit::{command_preview, record_audit_blocking, AuditCategory, AuditEvent, AuditOutcome};
use crate::core::cmd_audit::{CmdAuditAction, CmdAuditCacheStore, CmdAuditEngine, CmdAuditResult};
use crate::core::team::TeamState;

/// 被团队策略拦截时的退出码。
pub const EXIT_BLOCKED: i32 = 77;
/// 需要确认但没有确认（没有终端可问，也没加 --yes）时的退出码。
pub const EXIT_NEEDS_CONFIRM: i32 = 76;
/// 在终端里问了，用户没同意。
pub const EXIT_DECLINED: i32 = 75;

/// 把关结论。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateVerdict {
    /// 直接执行（只读，策略允许或只告警）。
    Run,
    /// 需要人确认；`reason` 是给人看的原因。
    NeedsConfirm { reason: String },
    /// 不执行。
    Blocked { reason: String },
}

/// 与桌面版相同的命令策略引擎：登录了团队时用桌面同步下来的策略缓存。
pub fn load_engine() -> CmdAuditEngine {
    let mut engine = CmdAuditEngine::new();
    let state = TeamState::load();
    if state.user.is_some() {
        if let Some(tid) = state.current_team_id.as_deref() {
            if let Some(payload) = CmdAuditCacheStore::load().payload_for_team(tid) {
                engine.apply_sync(payload);
            }
        }
    }
    engine
}

/// 纯判断：策略结果 + 命令本身 → 结论。
pub fn verdict(audit: &CmdAuditResult, command: &str) -> GateVerdict {
    // allow_mutate = true：变更类不直接拒绝，而是要求确认（L2）。
    let decision = gate_decision(audit.clone(), command, true);
    match decision.level {
        GateLevel::L0Block => GateVerdict::Blocked {
            reason: decision.message,
        },
        GateLevel::L2 => GateVerdict::NeedsConfirm {
            reason: if audit.action == CmdAuditAction::Confirm {
                let m = audit
                    .matches
                    .first()
                    .map(|m| m.message.clone())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_default();
                if m.is_empty() {
                    "团队命令策略要求这条命令先确认".to_string()
                } else {
                    format!("团队命令策略要求先确认：{m}")
                }
            } else {
                match classify_command(command) {
                    CommandClass::Mutating { reason } => format!("这条命令会改动服务器（{reason}）"),
                    _ => "这条命令会改动服务器".to_string(),
                }
            },
        },
        GateLevel::L1 => match classify_command(command) {
            CommandClass::ReadOnly { .. } => GateVerdict::Run,
            CommandClass::Unknown { reason } => GateVerdict::NeedsConfirm {
                reason: if reason.is_empty() {
                    "看不出这条命令是否只读".to_string()
                } else {
                    format!("看不出这条命令是否只读（{reason}）")
                },
            },
            CommandClass::Mutating { reason } => GateVerdict::NeedsConfirm {
                reason: format!("这条命令会改动服务器（{reason}）"),
            },
        },
    }
}

/// 写一条命令类审计事件（和桌面 `record_cmd_audit_event` 同样的字段，另加 `source: cli`）。
pub fn record(action: &str, outcome: AuditOutcome, command: &str, audit: &CmdAuditResult, targets: &[String], extra: serde_json::Value) {
    let matches: Vec<serde_json::Value> = audit
        .matches
        .iter()
        .map(|m| {
            serde_json::json!({
                "rule_id": m.rule_id,
                "source": m.source,
                "level": m.level,
                "message": m.message,
                "action": format!("{:?}", m.action).to_lowercase(),
            })
        })
        .collect();
    let mut detail = serde_json::json!({
        "source": "cli",
        "command_preview": command_preview(command, 200),
        "policy_action": format!("{:?}", audit.action).to_lowercase(),
        "matches": matches,
        "targets": targets,
    });
    if let (Some(d), Some(e)) = (detail.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            d.insert(k.clone(), v.clone());
        }
    }
    let mut ev = AuditEvent::new(AuditCategory::Command, action, outcome).with_detail(detail);
    if let [one] = targets {
        ev = ev.with_host(one.clone());
    }
    record_audit_blocking(ev);
}

/// 执行前把关。返回 `Ok(())` 表示可以执行；`Err(code)` 表示不执行，调用方用这个退出码退出。
pub fn check_before_exec(command: &str, targets: &[String], yes: bool, json: bool) -> Result<(), i32> {
    let engine = load_engine();
    let audit = engine.check(command);
    match verdict(&audit, command) {
        GateVerdict::Run => {
            if audit.action == CmdAuditAction::Alert {
                record("command.alert", AuditOutcome::Success, command, &audit, targets, serde_json::json!({}));
            }
            record("command.submit", AuditOutcome::Success, command, &audit, targets, serde_json::json!({ "confirmed": false }));
            Ok(())
        }
        GateVerdict::Blocked { reason } => {
            record("command.blocked", AuditOutcome::Denied, command, &audit, targets, serde_json::json!({}));
            report(json, "blocked", &reason, "这条命令不会执行。");
            Err(EXIT_BLOCKED)
        }
        GateVerdict::NeedsConfirm { reason } => {
            if yes {
                record("command.confirmed", AuditOutcome::Success, command, &audit, targets, serde_json::json!({ "confirmed_by": "--yes" }));
                record("command.submit", AuditOutcome::Success, command, &audit, targets, serde_json::json!({ "confirmed": true }));
                return Ok(());
            }
            let interactive = !json && std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
            if interactive {
                eprintln!("{reason}");
                eprintln!("  目标：{}", targets.join(", "));
                eprintln!("  命令：{command}");
                eprint!("确认执行吗？输入 y 回车执行，直接回车取消：");
                let _ = std::io::stderr().flush();
                let mut line = String::new();
                let _ = std::io::stdin().lock().read_line(&mut line);
                if matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                    record("command.confirmed", AuditOutcome::Success, command, &audit, targets, serde_json::json!({ "confirmed_by": "prompt" }));
                    record("command.submit", AuditOutcome::Success, command, &audit, targets, serde_json::json!({ "confirmed": true }));
                    return Ok(());
                }
                record("command.cancelled", AuditOutcome::Denied, command, &audit, targets, serde_json::json!({}));
                eprintln!("已取消，没有执行。");
                return Err(EXIT_DECLINED);
            }
            record("command.needs_confirm", AuditOutcome::Denied, command, &audit, targets, serde_json::json!({}));
            report(
                json,
                "needs_confirm",
                &reason,
                "没有执行。请先让人确认，确认后在 mist exec 后面加 --yes 再运行。",
            );
            Err(EXIT_NEEDS_CONFIRM)
        }
    }
}

/// `--yes` 必须紧跟在子命令后面（`mist exec --yes …`、`mist frag run --yes …`）。
///
/// 这样 AI 助手的「执行前问我」规则只要按开头匹配 `mist exec --yes` 就能拦住所有确认执行，
/// 不会因为 `--yes` 写在后面而漏掉。`sub` 是子命令的词，如 `["exec"]`、`["frag", "run"]`。
pub fn yes_flag_right_after(args: &[String], sub: &[&str]) -> bool {
    let n = sub.len();
    (0..args.len().saturating_sub(n)).any(|i| {
        args[i..i + n].iter().map(String::as_str).eq(sub.iter().copied())
            && matches!(args.get(i + n).map(String::as_str), Some("--yes" | "-y"))
    })
}

fn report(json: bool, status: &str, reason: &str, tail: &str) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": false,
                "status": status,
                "reason": reason,
                "message": tail,
            }))
            .unwrap_or_default()
        );
    } else {
        eprintln!("{reason}。{tail}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cmd_audit::CmdAuditSyncPayload;

    fn allow() -> CmdAuditResult {
        CmdAuditEngine::new().check("anything")
    }

    #[test]
    fn readonly_runs_mutating_needs_confirm() {
        for c in [
            "df -h",
            "free -m; uptime",
            "journalctl -u nginx --since '1 hour ago' | grep -i error | tail -n 50",
            "ps aux --sort=-%mem | head",
            "curl -fsS http://127.0.0.1:8080/health",
            "for f in /var/log/*.log; do wc -l \"$f\"; done",
        ] {
            assert_eq!(verdict(&allow(), c), GateVerdict::Run, "{c}");
        }
        for c in [
            "rm -rf /tmp/x",
            "systemctl restart nginx",
            "echo hi > /etc/motd",
            "sed -i 's/a/b/' /etc/hosts",
            "curl -X POST http://x/api",
        ] {
            assert!(matches!(verdict(&allow(), c), GateVerdict::NeedsConfirm { .. }), "{c}");
        }
        // 看不出是否只读：也要确认
        assert!(matches!(verdict(&allow(), "mysql -e 'select 1'"), GateVerdict::NeedsConfirm { .. }));
    }

    #[test]
    fn yes_must_follow_subcommand() {
        let a = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        assert!(yes_flag_right_after(&a("mist exec --yes web -- rm x"), &["exec"]));
        assert!(yes_flag_right_after(&a("mist --json exec -y web -- rm x"), &["exec"]));
        assert!(!yes_flag_right_after(&a("mist exec web --yes -- rm x"), &["exec"]));
        assert!(!yes_flag_right_after(&a("mist exec --json --yes web -- rm x"), &["exec"]));
        assert!(yes_flag_right_after(&a("mist frag run --yes 重启 web"), &["frag", "run"]));
        assert!(!yes_flag_right_after(&a("mist frag run 重启 --yes web"), &["frag", "run"]));
    }

    fn team_engine(action: &str) -> CmdAuditEngine {
        let payload: CmdAuditSyncPayload = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "policy": { "enabled": true, "dangerous_action": "block", "sensitive_action": "confirm", "unknown_action": "allow" },
            "rules": [ { "id": "r1", "name": "no-df", "pattern": "df", "match_type": "prefix", "action": action, "description": "测试规则" } ]
        }))
        .unwrap();
        let mut e = CmdAuditEngine::new();
        e.apply_sync(payload);
        e
    }

    #[test]
    fn team_policy_is_respected() {
        let blocked = team_engine("block").check("df -h");
        assert!(matches!(verdict(&blocked, "df -h"), GateVerdict::Blocked { .. }));
        let confirm = team_engine("confirm").check("df -h");
        match verdict(&confirm, "df -h") {
            GateVerdict::NeedsConfirm { reason } => assert!(reason.contains("测试规则"), "{reason}"),
            v => panic!("{v:?}"),
        }
        let alert = team_engine("alert").check("df -h");
        assert_eq!(alert.action, CmdAuditAction::Alert);
        assert_eq!(verdict(&alert, "df -h"), GateVerdict::Run);
    }
}
