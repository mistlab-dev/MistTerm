//! 策略可读性解释：把拦截/确认规则说成人话。

use crate::core::cmd_audit::{CmdAuditAction, CmdAuditResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyExplanation {
    /// 标题摘要
    pub title: String,
    /// 详细成因
    pub reason: String,
    /// 风险级别短标签
    pub risk_tier: String,
    /// 更安全的替代做法
    pub suggestion: Option<String>,
    /// 怎样才能继续执行
    pub pass_condition: String,
}

/// 对命令和审计结果做可读解释
pub fn explain_policy_decision(command: &str, audit: &CmdAuditResult, is_mutate: bool) -> PolicyExplanation {
    let cmd = command.trim();
    let cmd_lower = cmd.to_lowercase();

    let matched_msg = audit
        .matches
        .first()
        .map(|m| m.message.trim().to_string())
        .filter(|s| !s.is_empty());
    let matched_rm = audit
        .matches
        .first()
        .is_some_and(|m| m.rule_id.contains("rm"));

    if matched_rm || cmd_lower.starts_with("rm ") || cmd_lower.contains("rm -rf") {
        return PolicyExplanation {
            title: "高危删除操作".into(),
            reason: match &matched_msg {
                Some(msg) => format!(
                    "检测到强制或递归删除（{msg}）。在生产或批量环境中很容易误删，且往往无法恢复。"
                ),
                None => "检测到强制或递归删除。在生产或批量环境中很容易误删，且往往无法恢复。"
                    .to_string(),
            },
            risk_tier: "严重高危".into(),
            suggestion: Some("可先改用移动到临时目录做备份，或用带预览/演练参数的清理脚本。".into()),
            pass_condition: "请在单台主机的交互终端里人工确认后再执行；不要在 AI 助手里对多台机器无人值守批量跑。".into(),
        };
    }

    if cmd_lower.contains("drop ") || cmd_lower.contains("truncate ") {
        return PolicyExplanation {
            title: "数据库破坏性操作".into(),
            reason: "检测到 DROP 或 TRUNCATE，可能直接清空或删掉生产数据。".into(),
            risk_tier: "数据高危".into(),
            suggestion: Some("请走数据库变更审批，或先备份/打快照再操作。".into()),
            pass_condition: "需管理员审批，并按受控的 SQL 变更流程执行。".into(),
        };
    }

    if cmd_lower.contains("iptables") || cmd_lower.contains("nft ") {
        return PolicyExplanation {
            title: "防火墙规则变更".into(),
            reason: "清空或大幅改动防火墙规则，可能导致主机暴露，或立刻断开 SSH 运维通道。".into(),
            risk_tier: "网络高危".into(),
            suggestion: Some("建议先用「iptables -L -n -v」查看现有规则，再按需单条增删。".into()),
            pass_condition: "确认有备用登录方式，或在单机上准备好可回滚的任务后再执行。".into(),
        };
    }

    if cmd_lower.contains("reboot") || cmd_lower.contains("shutdown") || cmd_lower.contains("poweroff") {
        return PolicyExplanation {
            title: "主机重启或关机".into(),
            reason: "将重启或关闭整台机器，正在运行的业务会立即中断。".into(),
            risk_tier: "可用性高危".into(),
            suggestion: Some("可先用 uptime 看运行状态，或按排水/灰度流程分批重启。".into()),
            pass_condition: "确认该节点已从负载中摘除，并在约定的变更窗口内操作。".into(),
        };
    }

    if let Some(m) = audit.matches.first() {
        if audit.action == CmdAuditAction::Block {
            let msg = m.message.trim();
            return PolicyExplanation {
                title: "命中安全策略，已拦截".into(),
                reason: if !msg.is_empty() {
                    msg.to_string()
                } else {
                    "命令匹配了团队禁止的模式。".to_string()
                },
                risk_tier: "已拦截".into(),
                suggestion: None,
                pass_condition: "团队策略不允许执行；若确有必要，请联系管理员申请例外。".into(),
            };
        }
    }

    if is_mutate {
        if cmd_lower.contains("systemctl") || cmd_lower.contains("service") {
            return PolicyExplanation {
                title: "系统服务变更".into(),
                reason: "将启停或重载系统服务，可能导致短时抖动或服务不可用。".into(),
                risk_tier: "变更需确认".into(),
                suggestion: Some("建议先运行 systemctl status <服务名> 查看状态与依赖。".into()),
                pass_condition: "需再次确认；批量执行时会一台一台跑，首台失败即停止。".into(),
            };
        }

        if cmd_lower.contains("kill") || cmd_lower.contains("pkill") {
            return PolicyExplanation {
                title: "强制结束进程".into(),
                reason: "会直接结束运行中的进程，未保存的数据可能丢失。".into(),
                risk_tier: "变更需确认".into(),
                suggestion: Some("建议先用 ps 确认进程 ID 与所属用户。".into()),
                pass_condition: "确认目标进程无重要业务后再二次确认执行。".into(),
            };
        }

        return PolicyExplanation {
            title: "会改动系统状态".into(),
            reason: "该命令可能改写文件、权限或配置，不是单纯的查看类操作。".into(),
            risk_tier: "变更需确认".into(),
            suggestion: Some("建议先跑只读命令检查当前环境。".into()),
            pass_condition: "需再次确认后才会执行。".into(),
        };
    }

    PolicyExplanation {
        title: "只读命令，风险较低".into(),
        reason: "看起来是查看状态类操作，一般不会改坏主机。".into(),
        risk_tier: "只读".into(),
        suggestion: None,
        pass_condition: "确认后即可执行。".into(),
    }
}

/// English text for the fixed Chinese strings above (UI display only; never sent to the model).
const EN_TABLE: &[(&str, &str)] = &[
    ("高危删除操作", "Dangerous delete"),
    ("检测到强制或递归删除。在生产或批量环境中很容易误删，且往往无法恢复。", "Forced or recursive delete detected. In production or batch runs it is easy to delete the wrong thing, and it often can't be undone."),
    ("严重高危", "Critical"),
    ("可先改用移动到临时目录做备份，或用带预览/演练参数的清理脚本。", "Move files to a temporary folder as a backup first, or use a cleanup script with a preview / dry-run option."),
    ("请在单台主机的交互终端里人工确认后再执行；不要在 AI 助手里对多台机器无人值守批量跑。", "Confirm it by hand in an interactive terminal on a single host; don't run it unattended on many hosts from the AI assistant."),
    ("数据库破坏性操作", "Destructive database operation"),
    ("检测到 DROP 或 TRUNCATE，可能直接清空或删掉生产数据。", "DROP or TRUNCATE detected; this can wipe or delete production data."),
    ("数据高危", "Data risk"),
    ("请走数据库变更审批，或先备份/打快照再操作。", "Go through database change approval, or take a backup / snapshot first."),
    ("需管理员审批，并按受控的 SQL 变更流程执行。", "Needs admin approval and must follow the controlled SQL change process."),
    ("防火墙规则变更", "Firewall rule change"),
    ("清空或大幅改动防火墙规则，可能导致主机暴露，或立刻断开 SSH 运维通道。", "Flushing or heavily changing firewall rules can expose the host or cut off your SSH access immediately."),
    ("网络高危", "Network risk"),
    ("建议先用「iptables -L -n -v」查看现有规则，再按需单条增删。", "Check current rules with \"iptables -L -n -v\" first, then add or remove rules one at a time."),
    ("确认有备用登录方式，或在单机上准备好可回滚的任务后再执行。", "Make sure you have a backup way to log in, or a rollback job ready on the host, before running it."),
    ("主机重启或关机", "Host reboot or shutdown"),
    ("将重启或关闭整台机器，正在运行的业务会立即中断。", "This reboots or shuts down the whole machine; running services stop immediately."),
    ("可用性高危", "Availability risk"),
    ("可先用 uptime 看运行状态，或按排水/灰度流程分批重启。", "Check status with uptime first, or reboot in batches following your drain / canary process."),
    ("确认该节点已从负载中摘除，并在约定的变更窗口内操作。", "Make sure the node is out of the load balancer and you are inside the agreed change window."),
    ("命中安全策略，已拦截", "Blocked by security policy"),
    ("命令匹配了团队禁止的模式。", "The command matches a pattern your team has blocked."),
    ("已拦截", "Blocked"),
    ("团队策略不允许执行；若确有必要，请联系管理员申请例外。", "Team policy doesn't allow this; if you really need it, ask an admin for an exception."),
    ("系统服务变更", "System service change"),
    ("将启停或重载系统服务，可能导致短时抖动或服务不可用。", "This starts, stops or reloads a system service and may cause a brief disruption or outage."),
    ("变更需确认", "Change — needs confirmation"),
    ("建议先运行 systemctl status <服务名> 查看状态与依赖。", "Run systemctl status <service> first to check its state and dependencies."),
    ("需再次确认；批量执行时会一台一台跑，首台失败即停止。", "Needs a second confirmation; batch runs go one host at a time and stop at the first failure."),
    ("强制结束进程", "Force-kill process"),
    ("会直接结束运行中的进程，未保存的数据可能丢失。", "This ends running processes directly; unsaved data may be lost."),
    ("建议先用 ps 确认进程 ID 与所属用户。", "Check the process ID and owner with ps first."),
    ("确认目标进程无重要业务后再二次确认执行。", "Make sure the target process isn't doing important work, then confirm again to run."),
    ("会改动系统状态", "Changes system state"),
    ("该命令可能改写文件、权限或配置，不是单纯的查看类操作。", "This command may change files, permissions or config; it isn't just a read-only check."),
    ("建议先跑只读命令检查当前环境。", "Run a read-only command first to check the current state."),
    ("需再次确认后才会执行。", "Runs only after a second confirmation."),
    ("只读命令，风险较低", "Read-only command, low risk"),
    ("看起来是查看状态类操作，一般不会改坏主机。", "Looks like a status check; it normally won't break the host."),
    ("只读", "Read-only"),
    ("确认后即可执行。", "Runs after you confirm."),
];

fn to_en(s: &str) -> String {
    if let Some((_, en)) = EN_TABLE.iter().find(|(zh, _)| *zh == s) {
        return (*en).to_string();
    }
    // rm reason carrying the matched team rule message.
    if let Some(msg) = s
        .strip_prefix("检测到强制或递归删除（")
        .and_then(|r| r.strip_suffix("）。在生产或批量环境中很容易误删，且往往无法恢复。"))
    {
        return format!("Forced or recursive delete detected ({msg}). In production or batch runs it is easy to delete the wrong thing, and it often can't be undone.");
    }
    // Team-defined rule messages pass through unchanged.
    s.to_string()
}

impl PolicyExplanation {
    /// English copy of this explanation for non-Chinese UI. Team rule messages are kept as written.
    pub fn to_english(&self) -> PolicyExplanation {
        PolicyExplanation {
            title: to_en(&self.title),
            reason: to_en(&self.reason),
            risk_tier: to_en(&self.risk_tier),
            suggestion: self.suggestion.as_deref().map(to_en),
            pass_condition: to_en(&self.pass_condition),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cmd_audit::CmdAuditEngine;

    #[test]
    fn test_explain_rm_rf() {
        let eng = CmdAuditEngine::new();
        let res = eng.check("rm -rf /tmp/abc");
        let exp = explain_policy_decision("rm -rf /tmp/abc", &res, true);
        assert!(exp.title.contains("删除"));
        assert!(exp.suggestion.is_some());
    }

    #[test]
    fn test_explain_df_h() {
        let eng = CmdAuditEngine::new();
        let res = eng.check("df -h");
        let exp = explain_policy_decision("df -h", &res, false);
        assert!(exp.title.contains("只读"));
        assert_eq!(exp.risk_tier, "只读");
    }

    #[test]
    fn test_to_english_covers_builtin_text() {
        let eng = CmdAuditEngine::new();
        for (cmd, mutate) in [
            ("rm -rf /tmp/abc", true),
            ("DROP TABLE t", true),
            ("iptables -F", true),
            ("reboot", true),
            ("systemctl restart nginx", true),
            ("pkill node", true),
            ("touch /tmp/x", true),
            ("df -h", false),
        ] {
            let res = eng.check(cmd);
            let en = explain_policy_decision(cmd, &res, mutate).to_english();
            for s in [&en.title, &en.risk_tier, &en.pass_condition]
                .into_iter()
                .chain(en.suggestion.iter())
            {
                assert!(!s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)), "{cmd}: {s}");
            }
        }
    }
}
