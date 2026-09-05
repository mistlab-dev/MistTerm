//! Planner：NL → StepProposal（启发式；可手改；日后接 LLM）。

/// 下一步执行提议（尚未过门闩、未 SSH）。
#[derive(Debug, Clone)]
pub struct StepProposal {
    pub command: String,
    pub rationale: String,
    /// 是否建议结束（无命令可跑）。
    pub stop: bool,
}

/// 粗判：用户是否在要「上多机跑命令」而不是普通问答。
pub fn looks_like_host_ops_intent(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    let lower = t.to_lowercase();
    if lower.starts_with("多机:")
        || lower.starts_with("多机：")
        || lower.starts_with("@hosts")
        || lower.starts_with("/run")
    {
        return true;
    }
    const NEEDLES: &[&str] = &[
        "所有服务器",
        "所有主机",
        "全部服务器",
        "全部主机",
        "各台",
        "每台",
        "批量",
        "磁盘",
        "剩余空间",
        "硬盘",
        "内存",
        "负载",
        "cpu",
        "uptime",
        "df -",
        "free -",
        "查下服务器",
        "看看服务器",
        "服务器上",
        "集群",
    ];
    NEEDLES.iter().any(|n| lower.contains(&n.to_lowercase()) || t.contains(n))
}

/// 从自然语言启发式提议一条命令（可改）。
pub fn propose_step(user_text: &str) -> StepProposal {
    let t = user_text.trim();
    let stripped = strip_ops_prefix(t);
    let lower = stripped.to_lowercase();

    // 用户直接写了像命令的一行
    if looks_like_shell_line(stripped) {
        return StepProposal {
            command: stripped.to_string(),
            rationale: "按你输入的命令在目标主机上执行".into(),
            stop: false,
        };
    }

    if contains_any(&lower, stripped, &["磁盘", "disk", "空间", "filesystem", "df"]) {
        return StepProposal {
            command: "df -h".into(),
            rationale: "查各主机磁盘用量（可改命令）".into(),
            stop: false,
        };
    }
    if contains_any(&lower, stripped, &["内存", "memory", "mem ", "free"]) {
        return StepProposal {
            command: "free -h".into(),
            rationale: "查各主机内存（可改命令）".into(),
            stop: false,
        };
    }
    if contains_any(&lower, stripped, &["cpu", "负载", "load", "uptime"]) {
        return StepProposal {
            command: "uptime".into(),
            rationale: "查各主机负载与运行时间（可改命令）".into(),
            stop: false,
        };
    }
    if contains_any(&lower, stripped, &["谁在听", "端口", "listening", "ss -", "netstat"]) {
        return StepProposal {
            command: "ss -lntp".into(),
            rationale: "查监听端口（可改命令）".into(),
            stop: false,
        };
    }

    // 泛化：仍给可编辑默认，避免「必须写死场景」
    StepProposal {
        command: "uname -a && uptime".into(),
        rationale: "未识别具体指标，先用通用探活命令；请改成你要跑的命令".into(),
        stop: false,
    }
}

fn strip_ops_prefix(t: &str) -> &str {
    for p in ["多机:", "多机：", "@hosts ", "@hosts", "/run ", "/run"] {
        if let Some(rest) = t.strip_prefix(p) {
            return rest.trim();
        }
    }
    t
}

fn looks_like_shell_line(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.contains('\n') {
        return false;
    }
    if s.starts_with("sudo ") || s.starts_with("kubectl ") || s.starts_with("systemctl ") {
        return true;
    }
    let first = s.split_whitespace().next().unwrap_or("");
    matches!(
        first,
        "df" | "free" | "uptime" | "uname" | "ss" | "ps" | "top" | "cat" | "ls" | "hostname"
            | "whoami" | "id" | "date" | "journalctl" | "systemctl" | "docker" | "kubectl"
    )
}

fn contains_any(lower: &str, original: &str, needles: &[&str]) -> bool {
    needles
        .iter()
        .any(|n| lower.contains(&n.to_lowercase()) || original.contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_intent_proposes_df() {
        let p = propose_step("查下所有服务器上剩余磁盘空间");
        assert_eq!(p.command, "df -h");
        assert!(looks_like_host_ops_intent("查下所有服务器上剩余磁盘空间"));
    }

    #[test]
    fn memory_intent() {
        assert_eq!(propose_step("看看各台内存").command, "free -h");
    }

    #[test]
    fn plain_chat_not_ops() {
        assert!(!looks_like_host_ops_intent("解释一下这段报错是什么意思"));
    }

    #[test]
    fn prefix_forces_ops() {
        assert!(looks_like_host_ops_intent("多机: hostname"));
        assert_eq!(propose_step("多机: hostname").command, "hostname");
    }
}
