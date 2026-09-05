//! 门闩：按命令串决策 L0/L1/L2(不认「场景安全」)。

use crate::core::cmd_audit::{CmdAuditAction, CmdAuditResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateLevel {
    /// 拒跑
    L0Block,
    /// 计划确认即可
    L1,
    /// 须二次确认
    L2,
}

#[derive(Debug, Clone)]
pub struct GateDecision {
    pub level: GateLevel,
    pub mutate: bool,
    pub audit: CmdAuditResult,
    pub message: String,
}

/// 变更类命令粗匹配(选项 B：默认对话路径关闭变更时用于拒跑)。
pub fn looks_like_mutate_command(command: &str) -> bool {
    let c = command.to_lowercase();
    const PATS: &[&str] = &[
        "rm ",
        "rm\t",
        "mkfs",
        "dd if=",
        "reboot",
        "shutdown",
        "poweroff",
        "systemctl stop",
        "systemctl restart",
        "systemctl disable",
        "systemctl mask",
        "kubectl delete",
        "kubectl drain",
        "kubectl rollout",
        "drop table",
        "truncate ",
        "> /",
        "chmod 777",
        "chown -r",
    ];
    PATS.iter().any(|p| c.contains(p))
}

/// `allow_mutate`：设置项「允许对话发起变更」；MVP 默认 false。
pub fn gate_decision(audit: CmdAuditResult, command: &str, allow_mutate: bool) -> GateDecision {
    let mutate = looks_like_mutate_command(command);
    match audit.action {
        CmdAuditAction::Block => GateDecision {
            level: GateLevel::L0Block,
            mutate,
            message: format!(
                "已拦截：{}",
                audit
                    .matches
                    .first()
                    .map(|m| m.message.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("策略禁止")
            ),
            audit,
        },
        CmdAuditAction::Confirm => GateDecision {
            level: GateLevel::L2,
            mutate,
            message: "该命令需二次确认(审计 Confirm)".into(),
            audit,
        },
        CmdAuditAction::Alert | CmdAuditAction::Allow => {
            if mutate && !allow_mutate {
                GateDecision {
                    level: GateLevel::L0Block,
                    mutate: true,
                    message: "对话路径默认不允许变更类命令；请在终端执行，或日后开启「允许对话变更」"
                        .into(),
                    audit,
                }
            } else if mutate {
                GateDecision {
                    level: GateLevel::L2,
                    mutate: true,
                    message: "变更类命令：确认后仍需二次确认".into(),
                    audit,
                }
            } else {
                GateDecision {
                    level: GateLevel::L1,
                    mutate: false,
                    message: "只读/低危：确认后执行".into(),
                    audit,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cmd_audit::CmdAuditEngine;

    #[test]
    fn df_is_l1() {
        let eng = CmdAuditEngine::new();
        let d = gate_decision(eng.check("df -h"), "df -h", false);
        assert_eq!(d.level, GateLevel::L1);
    }

    #[test]
    fn restart_blocked_when_mutate_off() {
        let eng = CmdAuditEngine::new();
        let cmd = "systemctl restart nginx";
        let d = gate_decision(eng.check(cmd), cmd, false);
        assert_eq!(d.level, GateLevel::L0Block);
        assert!(d.mutate);
    }
}
