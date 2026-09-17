//! Planner：NL → StepProposal(启发式；可手改；日后接 LLM)。

#[derive(Debug, Clone)]
pub struct HostExecutionSummary {
    pub label: String,
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct LastBatchContext {
    pub command: String,
    pub hosts: Vec<HostExecutionSummary>,
}

/// 提议来源或引用的团队知识。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeReference {
    /// 来源类型："team_fragment" | "personal_fragment" | "team_doc"
    pub source_type: String,
    /// 来源标题（片段名称或文档标题）
    pub title: String,
    /// 引用锚点（如 fragment:xxx 或 doc:xxx）
    pub anchor: String,
    /// 推荐的原版标准命令或参考段落
    pub snippet: Option<String>,
}

/// 下一步执行提议(尚未过门闩、未 SSH)。
#[derive(Debug, Clone)]
pub struct StepProposal {
    pub command: String,
    pub rationale: String,
    /// 目标主机/分组过滤关键词（如 "web", "prod", "85.137" 等）。
    pub target_filter: Option<String>,
    /// 是否建议结束(无命令可跑)。
    pub stop: bool,
    /// 引用的团队知识库/片段来源（如有）。
    pub knowledge_ref: Option<KnowledgeReference>,
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
        "进程",
        "process",
        "df -",
        "free -",
        "查下服务器",
        "看看服务器",
        "服务器上",
        "集群",
        "排查报错",
        "排查失败",
        "看看报错",
        "报错的那台",
        "失败的那台",
        "清理日志",
        "清理大文件",
        "占用详情",
    ];
    NEEDLES.iter().any(|n| lower.contains(&n.to_lowercase()) || t.contains(n))
}

/// 从自然语言启发式提议一条命令(可改)。
pub fn propose_step(user_text: &str) -> StepProposal {
    propose_step_with_context(user_text, None)
}

/// 结合上一轮执行上下文提议命令(多轮下钻/处置支持)。
pub fn propose_step_with_context(
    user_text: &str,
    last_context: Option<&LastBatchContext>,
) -> StepProposal {
    let t = user_text.trim();
    let stripped = strip_ops_prefix(t);
    let lower = stripped.to_lowercase();

    // 1. 如果有上一轮上下文，优先尝试解析相对/跟进意图（如“排查报错的那台”、“清理磁盘超标机器”等）
    if let Some(ctx) = last_context {
        if contains_any(&lower, stripped, &["报错", "失败", "异常", "问题", "fail", "error"]) {
            let failed_host = ctx.hosts.iter().find(|h| !h.ok);
            if let Some(h) = failed_host {
                let host_part = h.label.split(" · ").last().unwrap_or(&h.label);
                return StepProposal {
                    command: "journalctl -xe -n 50 --no-pager".into(),
                    rationale: format!("针对上一轮报错主机 {host_part} 查看系统日志"),
                    target_filter: Some(host_part.to_string()),
                    stop: false,
                    knowledge_ref: None,
                };
            }
        }

        if (contains_any(&lower, stripped, &["清理", "大文件", "日志", "clean", "find"])
            && (ctx.command.contains("df") || contains_any(&lower, stripped, &["磁盘", "空间"])))
        {
            return StepProposal {
                command: "du -sh /var/log/* 2>/dev/null | sort -rh | head -n 10".into(),
                rationale: "检查各主机占用空间最大的日志文件以准备清理(只读排查)".into(),
                target_filter: extract_target_filter(user_text),
                stop: false,
                knowledge_ref: None,
            };
        }

        if (contains_any(&lower, stripped, &["top", "高占用", "详情", "detail", "谁占的"])
            && (ctx.command.contains("ps") || ctx.command.contains("free") || ctx.command.contains("uptime")))
        {
            return StepProposal {
                command: "ps aux --sort=-%mem | head -n 10".into(),
                rationale: "查看各主机内存/CPU占用最高的具体进程".into(),
                target_filter: extract_target_filter(user_text),
                stop: false,
                knowledge_ref: None,
            };
        }
    }

    // 用户直接写了像命令的一行
    if looks_like_shell_line(stripped) {
        return StepProposal {
            command: stripped.to_string(),
            rationale: "按你输入的命令在目标主机上执行".into(),
            target_filter: extract_target_filter(user_text),
            stop: false,
            knowledge_ref: None,
        };
    }

    let filter = extract_target_filter(user_text);

    if contains_any(&lower, stripped, &["磁盘", "disk", "空间", "filesystem", "df"]) {
        return StepProposal {
            command: "df -h".into(),
            rationale: "查各主机磁盘用量(可改命令)".into(),
            target_filter: filter,
            stop: false,
            knowledge_ref: None,
        };
    }
    if contains_any(&lower, stripped, &["内存", "memory", "mem ", "free"]) {
        return StepProposal {
            command: "free -h".into(),
            rationale: "查各主机内存(可改命令)".into(),
            target_filter: filter,
            stop: false,
            knowledge_ref: None,
        };
    }
    if contains_any(&lower, stripped, &["cpu", "负载", "load", "uptime"]) {
        return StepProposal {
            command: "uptime".into(),
            rationale: "查各主机负载与运行时间(可改命令)".into(),
            target_filter: filter,
            stop: false,
            knowledge_ref: None,
        };
    }
    if contains_any(
        &lower,
        stripped,
        &[
            "进程数量",
            "进程数",
            "进程个数",
            "多少进程",
            "process count",
            "process number",
            "nproc",
        ],
    ) || (contains_any(&lower, stripped, &["进程", "process", "processes"])
        && contains_any(
            &lower,
            stripped,
            &["数量", "个数", "多少", "count", "number", "num"],
        ))
    {
        return StepProposal {
            // pid= 无表头，输出即为进程数
            command: "ps -eo pid= | wc -l".into(),
            rationale: "统计各主机进程数量(可改命令)".into(),
            target_filter: filter,
            stop: false,
            knowledge_ref: None,
        };
    }
    if contains_any(&lower, stripped, &["进程", "process", "processes", "ps "]) {
        return StepProposal {
            command: "ps aux --sort=-%cpu | head -n 15".into(),
            rationale: "列出各主机占用 CPU 较高的进程(可改命令)".into(),
            target_filter: filter,
            stop: false,
            knowledge_ref: None,
        };
    }
    if contains_any(&lower, stripped, &["谁在听", "端口", "listening", "ss -", "netstat"]) {
        return StepProposal {
            command: "ss -lntp".into(),
            rationale: "查监听端口(可改命令)".into(),
            target_filter: filter,
            stop: false,
            knowledge_ref: None,
        };
    }

    // 泛化：仍给可编辑默认，避免「必须写死场景」
    StepProposal {
        command: "uname -a && uptime".into(),
        rationale: "未识别具体指标，先用通用探活命令；请改成你要跑的命令".into(),
        target_filter: filter,
        stop: false,
        knowledge_ref: None,
    }
}

/// 构建用于让 LLM 进行运维规划的 System Prompt，并可选择性注入团队已沉淀的标准知识库（Snippets 与 Docs）。
pub fn build_planner_system_prompt_with_knowledge(knowledge_context: Option<&str>) -> String {
    let mut base = "你是 MistTerm 的多主机智能运维规划器 (Planner)。\
用户的目标是在一批 Linux 服务器上排查或执行运维任务。\
你需要根据用户的自然语言意图以及历史执行记录，规划出下一步应当执行的单条 shell 命令。\
请严格以 JSON 格式输出，不要输出任何非 JSON 的闲聊文本。格式如下：\
{\n  \"command\": \"具体要执行的 shell 命令\",\n  \"rationale\": \"提议该命令的简要理由（中文）\",\n  \"target_filter\": \"可选的目标主机过滤词（如 web, db, prod 等，无则为 null）\",\n  \"stop\": false,\n  \"referenced_knowledge_anchor\": \"若采用了下方参考知识，填写对应 anchor（如 fragment:id 或 doc:id）；否则为 null\"\n}\
注意：\
1. 优先输出只读、安全的排查与诊断命令（如 df, free, ps, journalctl, ss, du 等）。\
2. 若用户意图与下方给出的【团队已验证的标准运维知识/命令片段】相匹配，应优先采纳团队的标准做法，不要自行臆测随意编写高风险命令。\
3. 尽量避免破坏性命令；若必须变更，保持最小化影响。\
4. command 必须可以直接在 bash/sh 下执行，不要包含交互式提问参数。"
        .to_string();

    if let Some(kc) = knowledge_context {
        if !kc.trim().is_empty() {
            base.push_str("\n\n【团队知识库与标准片段参考（肌肉记忆）】：\n");
            base.push_str(kc);
        }
    }
    base
}

/// 构建用于让 LLM 进行运维规划的 System Prompt（无额外知识上下文）。
pub fn build_planner_system_prompt() -> String {
    build_planner_system_prompt_with_knowledge(None)
}

/// 解析 LLM 返回的 JSON 规划结果，失败则平滑降级。
pub fn parse_llm_plan_response(response: &str) -> Option<StepProposal> {
    let text = response.trim();
    // 兼容 ```json ... ``` 包裹
    let clean = if let Some(start) = text.find('{') {
        if let Some(end) = text.rfind('}') {
            if end > start {
                &text[start..=end]
            } else {
                text
            }
        } else {
            text
        }
    } else {
        text
    };

    #[derive(serde::Deserialize)]
    struct RawPlan {
        command: Option<String>,
        rationale: Option<String>,
        target_filter: Option<String>,
        stop: Option<bool>,
        referenced_knowledge_anchor: Option<String>,
    }

    if let Ok(raw) = serde_json::from_str::<RawPlan>(clean) {
        if let Some(cmd) = raw.command {
            if !cmd.trim().is_empty() {
                let knowledge_ref = raw.referenced_knowledge_anchor.filter(|a| !a.trim().is_empty()).map(|anchor| {
                    KnowledgeReference {
                        source_type: if anchor.starts_with("doc:") { "team_doc".into() } else { "team_fragment".into() },
                        title: "团队标准实践引用".into(),
                        anchor,
                        snippet: None,
                    }
                });
                return Some(StepProposal {
                    command: cmd.trim().to_string(),
                    rationale: raw.rationale.unwrap_or_else(|| "AI 规划的执行命令".into()),
                    target_filter: raw.target_filter.filter(|s| !s.trim().is_empty()),
                    stop: raw.stop.unwrap_or(false),
                    knowledge_ref,
                });
            }
        }
    }
    None
}

/// 启发式提取目标主机过滤关键词（如 "web", "db", "prod", "staging", "85.137" 等）。
pub fn extract_target_filter(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    const SCOPES: &[&str] = &[
        "web", "api", "db", "mysql", "redis", "nginx", "prod", "production", "dev", "test",
        "staging", "qa",
    ];
    for scope in SCOPES {
        let pat_node = format!("{scope}节点");
        let pat_host = format!("{scope}主机");
        let pat_server = format!("{scope}服务器");
        let pat_env = format!("{scope}环境");
        if lower.contains(&pat_node)
            || lower.contains(&pat_host)
            || lower.contains(&pat_server)
            || lower.contains(&pat_env)
            || lower.contains(&format!(" {scope} "))
        {
            return Some((*scope).to_string());
        }
    }
    None
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
    fn process_count_intent() {
        let p = propose_step("查询 所有服务器进程数量");
        assert_eq!(p.command, "ps -eo pid= | wc -l");
        assert!(looks_like_host_ops_intent("查询 所有服务器进程数量"));
    }

    #[test]
    fn process_list_intent() {
        assert_eq!(
            propose_step("看看各台进程").command,
            "ps aux --sort=-%cpu | head -n 15"
        );
    }

    #[test]
    fn plain_chat_not_ops() {
        assert!(!looks_like_host_ops_intent("解释一下这段报错是什么意思"));
    }

    #[test]
    fn extract_target_filter_scopes() {
        assert_eq!(extract_target_filter("查下web节点的负载"), Some("web".into()));
        assert_eq!(extract_target_filter("检查prod环境的磁盘"), Some("prod".into()));
        assert_eq!(extract_target_filter("所有服务器内存"), None);
    }

    #[test]
    fn followup_failed_host() {
        let last = LastBatchContext {
            command: "systemctl status nginx".into(),
            hosts: vec![
                HostExecutionSummary {
                    label: "web-1 · 10.0.0.1".into(),
                    ok: true,
                    exit_code: Some(0),
                    summary: "active".into(),
                },
                HostExecutionSummary {
                    label: "web-2 · 10.0.0.2".into(),
                    ok: false,
                    exit_code: Some(3),
                    summary: "inactive".into(),
                },
            ],
        };
        let p = propose_step_with_context("看看报错那台的日志", Some(&last));
        assert_eq!(p.command, "journalctl -xe -n 50 --no-pager");
        assert_eq!(p.target_filter, Some("10.0.0.2".into()));
    }

    #[test]
    fn parse_llm_plan_json() {
        let json = r#"{"command": "du -sh /var/log/* | sort -hr | head -n 5", "rationale": "排查前5大日志文件", "target_filter": "web", "stop": false}"#;
        let p = parse_llm_plan_response(json).unwrap();
        assert_eq!(p.command, "du -sh /var/log/* | sort -hr | head -n 5");
        assert_eq!(p.rationale, "排查前5大日志文件");
        assert_eq!(p.target_filter, Some("web".into()));
        assert!(!p.stop);
    }

    #[test]
    fn parse_llm_plan_with_knowledge_ref() {
        let json = r#"{"command": "systemctl restart nginx", "rationale": "按团队标准重启", "target_filter": "web", "stop": false, "referenced_knowledge_anchor": "fragment:frag_123"}"#;
        let p = parse_llm_plan_response(json).unwrap();
        assert_eq!(p.command, "systemctl restart nginx");
        assert_eq!(p.knowledge_ref.as_ref().unwrap().anchor, "fragment:frag_123");
        assert_eq!(p.knowledge_ref.as_ref().unwrap().source_type, "team_fragment");
    }

    #[test]
    fn parse_llm_plan_markdown_wrapped() {
        let resp = "```json\n{\n  \"command\": \"ps aux --sort=-%cpu | head -n 10\",\n  \"rationale\": \"CPU高占用进程\"\n}\n```";
        let p = parse_llm_plan_response(resp).unwrap();
        assert_eq!(p.command, "ps aux --sort=-%cpu | head -n 10");
        assert_eq!(p.rationale, "CPU高占用进程");
        assert_eq!(p.target_filter, None);
    }
}
