//! 策略可读性解释器（Policy Explainer）
//!
//! 将底层的拦截规则、特征码转换为工程师易懂的“人话”解释，
//! 并提供明确的风险根因、安全放行条件及安全替代命令建议。

use crate::core::cmd_audit::{CmdAuditAction, CmdAuditResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyExplanation {
    /// 标题摘要，如「触发系统底层修改保护」
    pub title: String,
    /// 详细通俗成因说明
    pub reason: String,
    /// 安全隐患级别描述：高危 / 变更 / 敏感
    pub risk_tier: String,
    /// 推荐的安全替代命令或操作建议
    pub suggestion: Option<String>,
    /// 安全放行条件说明（如何合规操作）
    pub pass_condition: String,
}

/// 对命令和审计结果进行可读性解析
pub fn explain_policy_decision(command: &str, audit: &CmdAuditResult, is_mutate: bool) -> PolicyExplanation {
    let cmd = command.trim();
    let cmd_lower = cmd.to_lowercase();

    // 1. 基于命令文本的高危模式（独立于是否命中本地审计规则；
    //    AI 智控台调用时传入的 audit.matches 可能为空，因此文本特征优先判定）
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
            title: "触发高危删除防护 (Root/Recursive Delete)".into(),
            reason: match &matched_msg {
                Some(msg) => format!(
                    "检测到递归或强制删除操作（{msg}）。在生产或集群批量环境中极易导致不可逆的数据丢失。"
                ),
                None => "检测到递归或强制删除操作。在生产或集群批量环境中极易导致不可逆的数据丢失。"
                    .to_string(),
            },
            risk_tier: "严重高危 (Destructive)".into(),
            suggestion: Some("建议改用 'mv ... /tmp/' 软归档，或使用具备测试参数的清理脚本。".into()),
            pass_condition: "需在单一主机交互式终端中人工确认后执行，禁止在 AI 智控台批量无监督运行。".into(),
        };
    }

    if cmd_lower.contains("drop ") || cmd_lower.contains("truncate ") {
        return PolicyExplanation {
            title: "触发数据库高危 DDL 保护 (Data Destruction)".into(),
            reason: "检测到 DROP 或 TRUNCATE 数据库对象操作，直接影响生产持久化数据。".into(),
            risk_tier: "严重高危 (Data Loss)".into(),
            suggestion: Some("请通过专门的数据库变更审批流执行，或先备份快照后再做迁移。".into()),
            pass_condition: "需团队 Admin 审批，并走受控的 SQL 变更工单流程。".into(),
        };
    }

    if cmd_lower.contains("iptables") || cmd_lower.contains("nft ") {
        return PolicyExplanation {
            title: "触发网络防火墙安全门闩 (Network Isolation Risk)".into(),
            reason: "刷新或清空防火墙规则可能导致主机公网暴露，或立即切断 SSH 运维通道导致失联。".into(),
            risk_tier: "网络高危 (Connectivity Loss)".into(),
            suggestion: Some("建议使用 'iptables -L -n -v' 先检查现有规则，单条定向增删。".into()),
            pass_condition: "需具备带外带内自愈机制或在单机带定时回滚任务下执行。".into(),
        };
    }

    if cmd_lower.contains("reboot") || cmd_lower.contains("shutdown") || cmd_lower.contains("poweroff") {
        return PolicyExplanation {
            title: "触发主机电源与停机防线 (Host Shutdown)".into(),
            reason: "下发了整机重启或关机指令，会导致正在承载的业务立即中断。".into(),
            risk_tier: "可用性致命 (Outage Risk)".into(),
            suggestion: Some("建议使用 'uptime' 查看运行状态，或走分批灰度排水与重启流程。".into()),
            pass_condition: "需确认该节点已完成负载下线，并在变更窗口期内操作。".into(),
        };
    }

    // 2. 命中审计黑名单（依赖本地审计规则命中）
    if let Some(m) = audit.matches.first() {
        if audit.action == CmdAuditAction::Block {
            let msg = m.message.trim();
            return PolicyExplanation {
                title: format!("命中安全黑名单策略 ({})", m.rule_id),
                reason: if !msg.is_empty() { msg.to_string() } else { "命令包含团队安全基线明确禁止的特征模式。".to_string() },
                risk_tier: "规则拦截 (Policy Block)".into(),
                suggestion: None,
                pass_condition: "该命令被团队策略严令禁止；如确有必要，请联系安全管理员申请策略豁免。".into(),
            };
        }
    }

    // 2. 变更类命令拦截（即便本地 audit 没报错，但 Gate 判定为变更）
    if is_mutate {
        if cmd_lower.contains("systemctl") || cmd_lower.contains("service") {
            return PolicyExplanation {
                title: "系统服务状态变更 (Service Mutation)".into(),
                reason: "命令将变更关键系统或后台服务的启停状态，可能引发服务雪崩或短时抖动。".into(),
                risk_tier: "变更高危 (Service Impact)".into(),
                suggestion: Some("建议先运行 'systemctl status <name>' 查看运行状态与依赖。".into()),
                pass_condition: "AI 智控台已自动启用「串行执行与首台失败熔断」防线，需二次人工确认。".into(),
            };
        }

        if cmd_lower.contains("kill") || cmd_lower.contains("pkill") {
            return PolicyExplanation {
                title: "进程强制终止操作 (Process Termination)".into(),
                reason: "直接杀死运行中的进程，未持久化的内存数据可能丢失。".into(),
                risk_tier: "变更高危 (Process Impact)".into(),
                suggestion: Some("建议先通过 'ps -aux | grep <name>' 确认 PID 及归属用户。".into()),
                pass_condition: "需人工确认目标进程无核心业务锁，开启二次确认放行。".into(),
            };
        }

        return PolicyExplanation {
            title: "检测到系统变更意图 (State Mutation)".into(),
            reason: "该命令涉及文件改写、权限更动或配置刷新，非纯只读巡检指令。".into(),
            risk_tier: "变更操作 (L2 Gate)".into(),
            suggestion: Some("建议先执行只读命令检查当前环境状态。".into()),
            pass_condition: "需经二次确认（L2 Armed），并在执行引擎中启用串行熔断机制。".into(),
        };
    }

    // 3. 默认只读通过
    PolicyExplanation {
        title: "安全审计只读放行 (Readonly Verified)".into(),
        reason: "经安全模式分析，该命令为只读巡检与状态观测操作，无主机破坏性副作用。".into(),
        risk_tier: "只读放行 (L1 Safe)".into(),
        suggestion: None,
        pass_condition: "正常确认即可执行并发抓取。".into(),
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
        assert!(exp.title.contains("删除防护"));
        assert!(exp.suggestion.is_some());
    }

    #[test]
    fn test_explain_df_h() {
        let eng = CmdAuditEngine::new();
        let res = eng.check("df -h");
        let exp = explain_policy_decision("df -h", &res, false);
        assert!(exp.title.contains("只读放行"));
        assert_eq!(exp.risk_tier, "只读放行 (L1 Safe)");
    }
}
