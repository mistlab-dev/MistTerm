//! AI 助手增强：上下文感知、智能补全、快捷操作。

use crate::core::ai_client::prepare_terminal_context;
use crate::core::command_history::CommandHistory;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 上下文感知：自动收集终端状态供 AI 参考。
#[derive(Debug, Clone, Default)]
pub struct AiContext {
    /// 最近 N 条命令历史
    pub recent_commands: Vec<String>,
    /// 当前终端选中文本
    pub selected_text: Option<String>,
    /// 最后一次命令的输出
    pub last_output: Option<String>,
    /// 当前工作目录
    pub cwd: Option<String>,
    /// 当前主机名
    pub hostname: Option<String>,
    /// SSH 连接信息
    pub ssh_info: Option<SshInfo>,
    /// 错误输出（如果有）
    pub error_output: Option<String>,
    /// 最近一次失败的命令（来自命令历史）
    pub last_failed_command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshInfo {
    pub host: String,
    pub port: u16,
    pub user: String,
}

/// 智能补全候选
#[derive(Debug, Clone, Serialize)]
pub struct CompletionCandidate {
    pub command: String,
    pub score: f32,
    pub last_used: Option<i64>,
    pub use_count: usize,
}

/// AI 快捷操作类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuickAction {
    /// 解释选中文本
    ExplainSelection,
    /// 重试失败命令（修正后）
    RetryFixed,
    /// 生成类似命令
    GenerateSimilar,
    /// 总结终端输出
    SummarizeOutput,
    /// 生成运维脚本
    GenerateScript,
    /// 翻译错误信息
    TranslateError,
}

impl AiContext {
    /// 从命令历史构建上下文
    pub fn from_history(history: &CommandHistory, max_recent: usize) -> Self {
        let recent: Vec<String> = history
            .entries_newest_first()
            .take(max_recent)
            .map(|e| e.command.clone())
            .collect();

        Self {
            recent_commands: recent,
            ..Default::default()
        }
    }

    /// 构建系统提示词上下文块
    pub fn build_context_block(&self) -> String {
        let mut parts = Vec::new();

        // 最近命令
        if !self.recent_commands.is_empty() {
            parts.push("## 最近执行的命令".to_string());
            for (i, cmd) in self.recent_commands.iter().enumerate() {
                parts.push(format!("{}. {}", i + 1, cmd));
            }
        }

        // 工作目录
        if let Some(cwd) = &self.cwd {
            parts.push(format!("## 当前工作目录\n{}", cwd));
        }

        // 主机信息
        if let Some(host) = &self.hostname {
            parts.push(format!("## 主机名\n{}", host));
        }

        // SSH 信息
        if let Some(ssh) = &self.ssh_info {
            parts.push(format!(
                "## SSH 连接\n{}@{}:{}",
                ssh.user, ssh.host, ssh.port
            ));
        }

        // 错误输出
        if let Some(err) = &self.error_output {
            let prep = prepare_terminal_context(err);
            if !prep.text.is_empty() {
                parts.push(format!("## 错误输出\n{}", prep.text));
            }
        }

        // 最近终端输出摘要
        if let Some(out) = &self.last_output {
            let prep = prepare_terminal_context(out);
            if !prep.text.is_empty() {
                parts.push(format!("## 最近终端输出\n{}", prep.text));
            }
        }

        parts.join("\n\n")
    }

    /// 终端输出是否像错误信息（启发式）。
    pub fn looks_like_error_output(text: &str) -> bool {
        let lower = text.to_lowercase();
        [
            "error",
            "failed",
            "failure",
            "permission denied",
            "not found",
            "fatal",
            "panic",
            "denied",
        ]
        .iter()
        .any(|k| lower.contains(k))
    }

    /// 为 QuickAction 构建提示词
    pub fn build_action_prompt(&self, action: &QuickAction, selection: &str) -> String {
        match action {
            QuickAction::ExplainSelection => {
                format!(
                    "请解释以下终端输出或命令的含义，用简洁中文：\n\n```\n{}\n```",
                    selection
                )
            }
            QuickAction::RetryFixed => {
                let cmd = self
                    .last_failed_command
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(selection);
                let mut prompt =
                    "用户执行了以下命令但失败了，请分析错误并给出修正后的命令：\n\n".to_string();
                if let Some(err) = &self.error_output {
                    prompt.push_str(&format!("错误输出：\n```\n{}\n```\n\n", err));
                }
                prompt.push_str(&format!("原始命令：\n```\n{}\n```\n\n", cmd));
                prompt.push_str("请给出修正后的命令，用 ```bash 代码块包裹。");
                prompt
            }
            QuickAction::GenerateSimilar => {
                let mut prompt =
                    "用户经常执行类似以下命令，请根据历史模式生成 3-5 个可能有用的命令：\n\n"
                        .to_string();
                prompt.push_str(&format!("```\n{}\n```\n\n", selection));
                if !self.recent_commands.is_empty() {
                    prompt.push_str("最近命令历史：\n");
                    for cmd in &self.recent_commands {
                        prompt.push_str(&format!("- {}\n", cmd));
                    }
                }
                prompt
            }
            QuickAction::SummarizeOutput => {
                format!(
                    "请总结以下终端输出的关键信息，用简洁要点列出：\n\n```\n{}\n```",
                    selection
                )
            }
            QuickAction::GenerateScript => {
                let mut prompt =
                    "根据以下描述或命令，生成一个完整的运维脚本：\n\n".to_string();
                prompt.push_str(&format!("```\n{}\n```\n\n", selection));
                prompt.push_str("请用 ```bash 代码块包裹完整脚本，包含错误处理和注释。");
                prompt
            }
            QuickAction::TranslateError => {
                format!(
                    "请翻译并解释以下错误信息，给出可能的原因和解决方案：\n\n```\n{}\n```",
                    selection
                )
            }
        }
    }
}

/// 基于历史命令的智能补全
pub struct CommandCompleter {
    history: Vec<String>,
    cache: HashMap<String, Vec<CompletionCandidate>>,
}

impl CommandCompleter {
    pub fn new(history: &CommandHistory) -> Self {
        let cmds: Vec<String> = history
            .entries_newest_first()
            .map(|e| e.command.clone())
            .collect();
        Self {
            history: cmds,
            cache: HashMap::new(),
        }
    }

    /// 模糊匹配补全
    pub fn complete(&self, prefix: &str, max_results: usize) -> Vec<CompletionCandidate> {
        if prefix.is_empty() {
            return Vec::new();
        }

        let prefix_lower = prefix.to_lowercase();
        let mut seen = std::collections::HashSet::new();
        let mut candidates: Vec<CompletionCandidate> = Vec::new();

        for cmd in &self.history {
            let cmd_lower = cmd.to_lowercase();
            if cmd_lower.contains(&prefix_lower) || cmd.starts_with(prefix) {
                let key = cmd.clone();
                if seen.insert(key.clone()) {
                    let score = self.fuzzy_score(prefix, cmd);
                    candidates.push(CompletionCandidate {
                        command: cmd.clone(),
                        score,
                        last_used: None,
                        use_count: 1,
                    });
                }
            }
        }

        candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        candidates.truncate(max_results);
        candidates
    }

    /// 简单模糊评分：前缀匹配 > 包含匹配 > 部分匹配
    fn fuzzy_score(&self, query: &str, candidate: &str) -> f32 {
        let query_lower = query.to_lowercase();
        let cand_lower = candidate.to_lowercase();

        if cand_lower == query_lower {
            return 100.0;
        }
        if cand_lower.starts_with(&query_lower) {
            return 90.0 - (candidate.len() as f32 * 0.1);
        }
        if cand_lower.contains(&query_lower) {
            return 70.0 - (candidate.len() as f32 * 0.05);
        }

        // 部分字符匹配
        let mut score = 0.0;
        let mut q_chars = query_lower.chars();
        for c in cand_lower.chars() {
            if q_chars.clone().next() == Some(c) {
                score += 10.0;
                q_chars.next();
            }
        }
        score
    }

    /// 从历史中提取常用命令前缀
    pub fn common_prefixes(&self, min_count: usize) -> Vec<(String, usize)> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for cmd in &self.history {
            if let Some(first_word) = cmd.split_whitespace().next() {
                *counts.entry(first_word.to_string()).or_insert(0) += 1;
            }
        }

        let mut result: Vec<(String, usize)> = counts
            .into_iter()
            .filter(|(_, count)| *count >= min_count)
            .collect();
        result.sort_by(|a, b| b.1.cmp(&a.1));
        result
    }
}

/// 增强的系统提示词构建器
pub struct EnhancedPromptBuilder {
    base_prompt: String,
    context: AiContext,
}

impl EnhancedPromptBuilder {
    pub fn new(base_prompt: &str, context: AiContext) -> Self {
        Self {
            base_prompt: base_prompt.to_string(),
            context,
        }
    }

    /// 构建完整的系统提示词
    pub fn build(&self) -> String {
        let mut prompt = self.base_prompt.clone();

        // 添加上下文信息
        let ctx_block = self.context.build_context_block();
        if !ctx_block.is_empty() {
            prompt.push_str("\n\n## 当前环境上下文\n");
            prompt.push_str(&ctx_block);
        }

        // 添加行为指导
        prompt.push_str("\n\n## 行为规范");
        prompt.push_str("\n- 基于用户的历史命令和当前环境给出针对性建议");
        prompt.push_str("\n- 如果检测到错误输出，优先分析错误原因");
        prompt.push_str("\n- 建议的命令应符合用户的技术栈和习惯");
        prompt.push_str("\n- 避免重复用户已经执行过的命令");
        prompt.push_str("\n- 对于危险操作，明确警告并建议备份");

        prompt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_score_exact() {
        let c = CommandCompleter::new(&CommandHistory::new());
        assert_eq!(c.fuzzy_score("ls", "ls"), 100.0);
    }

    #[test]
    fn fuzzy_score_prefix() {
        let c = CommandCompleter::new(&CommandHistory::new());
        let score = c.fuzzy_score("git", "git status");
        assert!(score > 80.0);
    }

    #[test]
    fn context_block_empty() {
        let ctx = AiContext::default();
        assert!(ctx.build_context_block().is_empty());
    }

    #[test]
    fn context_block_includes_last_output() {
        let ctx = AiContext {
            last_output: Some("nginx: error".into()),
            ..Default::default()
        };
        assert!(ctx.build_context_block().contains("nginx"));
    }

    #[test]
    fn looks_like_error_detects_common_patterns() {
        assert!(AiContext::looks_like_error_output("Permission denied"));
        assert!(!AiContext::looks_like_error_output("all ok"));
    }

    #[test]
    fn enhanced_prompt_includes_ssh() {
        let ctx = AiContext {
            ssh_info: Some(SshInfo {
                host: "prod-1".into(),
                port: 22,
                user: "deploy".into(),
            }),
            recent_commands: vec!["systemctl status nginx".into()],
            ..Default::default()
        };
        let prompt = EnhancedPromptBuilder::new("You are a shell assistant.", ctx).build();
        assert!(prompt.contains("deploy@prod-1"));
        assert!(prompt.contains("systemctl"));
    }

    #[test]
    fn action_prompt_explain() {
        let ctx = AiContext::default();
        let prompt = ctx.build_action_prompt(&QuickAction::ExplainSelection, "error: not found");
        assert!(prompt.contains("error: not found"));
        assert!(prompt.contains("解释"));
    }
}
