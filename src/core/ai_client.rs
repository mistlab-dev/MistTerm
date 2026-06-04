//! OpenAI 兼容 Chat Completions 客户端（阻塞 HTTP，供后台线程调用）。

use serde::{Deserialize, Serialize};

use crate::core::AiSettings;

const SYSTEM_PROMPT: &str = "你是 MistTerm 终端里的运维助手。用户会提问或附上终端输出。\
请用简洁中文回答。若给出完整 shell 脚本，请用单个 ```bash 代码块包裹整段脚本；若给出若干条可直接执行的命令，\
用 ```bash 代码块列出，每行一条命令，不要与完整脚本混在同一提取逻辑里。不要编造未提供的输出。";

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ApiMessage<'a>>,
    temperature: f32,
    max_tokens: u32,
}

#[derive(Serialize)]
struct ApiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ApiMessageOwned,
}

#[derive(Deserialize)]
struct ApiMessageOwned {
    content: String,
}

#[derive(Deserialize)]
struct ApiErrorBody {
    error: Option<ApiErrorDetail>,
}

#[derive(Deserialize)]
struct ApiErrorDetail {
    message: Option<String>,
}

/// 发往模型的终端上下文行数上限（超出截断）。
pub const AI_CONTEXT_MAX_LINES: usize = 400;
/// 发往模型的终端上下文字符上限（超出截断）。
pub const AI_CONTEXT_MAX_CHARS: usize = 24_000;

/// 终端选区经脱敏与体积限制后的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedTerminalContext {
    pub text: String,
    pub line_count: usize,
    pub char_count: usize,
    pub truncated: bool,
    pub original_line_count: usize,
    pub original_char_count: usize,
}

/// 脱敏并按 [`AI_CONTEXT_MAX_LINES`] / [`AI_CONTEXT_MAX_CHARS`] 截断。
pub fn prepare_terminal_context(text: &str) -> PreparedTerminalContext {
    let redacted = redact_for_ai(text);
    let trimmed = redacted.trim();
    let original_line_count = if trimmed.is_empty() {
        0
    } else {
        trimmed.lines().count()
    };
    let original_char_count = trimmed.chars().count();
    if original_line_count == 0 {
        return PreparedTerminalContext {
            text: String::new(),
            line_count: 0,
            char_count: 0,
            truncated: false,
            original_line_count: 0,
            original_char_count: 0,
        };
    }
    let mut lines: Vec<&str> = trimmed.lines().collect();
    let mut truncated = false;
    if lines.len() > AI_CONTEXT_MAX_LINES {
        lines.truncate(AI_CONTEXT_MAX_LINES);
        truncated = true;
    }
    let mut out = lines.join("\n");
    if out.chars().count() > AI_CONTEXT_MAX_CHARS {
        out = out.chars().take(AI_CONTEXT_MAX_CHARS).collect();
        truncated = true;
    }
    let line_count = if out.is_empty() {
        0
    } else {
        out.lines().count()
    };
    let char_count = out.chars().count();
    PreparedTerminalContext {
        text: out,
        line_count,
        char_count,
        truncated,
        original_line_count,
        original_char_count,
    }
}

/// 脱敏后再发往模型。
pub fn redact_for_ai(text: &str) -> String {
    let mut out = text.to_string();
    const NEEDLES: &[&str] = &[
        "Bearer ",
        "-----BEGIN",
        "PRIVATE KEY",
        "password=",
        "PASSWORD=",
        "api_key=",
        "API_KEY=",
        "token=",
        "TOKEN=",
    ];
    for n in NEEDLES {
        if let Some(i) = out.find(n) {
            let end = out[i..]
                .find(|c: char| c.is_whitespace() || c == '\n' || c == '"' || c == '\'')
                .map(|o| i + o)
                .unwrap_or(out.len().min(i + 48));
            out.replace_range(i..end, "[REDACTED]");
        }
    }
    out
}

/// 从回复中提取可在终端单独执行的 shell 命令（跳过整段脚本类代码块）。
pub fn extract_shell_commands(reply: &str) -> Vec<String> {
    let mut cmds = Vec::new();
    let mut in_fence = false;
    let mut block: Vec<String> = Vec::new();

    for line in reply.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            if in_fence {
                cmds.extend(commands_from_fence_block(&block));
                block.clear();
            }
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            block.push(t.to_string());
            continue;
        }
        if let Some(c) = prompt_line_to_command(t) {
            cmds.push(c);
        }
    }
    if in_fence && !block.is_empty() {
        cmds.extend(commands_from_fence_block(&block));
    }
    cmds.sort();
    cmds.dedup();
    cmds
}

fn commands_from_fence_block(lines: &[String]) -> Vec<String> {
    if lines.is_empty() || is_whole_script_block(lines) {
        return Vec::new();
    }
    lines
        .iter()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#') && looks_like_runnable_line(t)
        })
        .cloned()
        .collect()
}

fn is_whole_script_block(lines: &[String]) -> bool {
    if lines.iter().any(|l| l.starts_with("#!")) {
        return true;
    }
    if lines.iter().any(|l| l.contains("<<") && l.contains("EOF")) {
        return true;
    }
    if lines.len() >= 6 {
        return true;
    }
    if lines.len() >= 3 {
        let has_control = lines.iter().any(|l| {
            let t = l.trim();
            t.starts_with("if ") || t.starts_with("elif ") || t == "fi"
                || t.starts_with("for ") || t.starts_with("while ")
                || t.starts_with("case ") || t.starts_with("function ")
                || t.ends_with(" do") || t == "done"
        });
        if has_control {
            return true;
        }
    }
    false
}

fn looks_like_runnable_line(line: &str) -> bool {
    if matches!(line, "fi" | "done" | "esac" | "then" | "else" | "do") {
        return false;
    }
    if line.starts_with("if ") || line.starts_with("elif ") || line.starts_with("for ")
        || line.starts_with("while ") || line.starts_with("case ") || line.starts_with("function ")
    {
        return false;
    }
    if line.starts_with("cat ") && line.contains("<<") {
        return false;
    }
    let first = line.split_whitespace().next().unwrap_or("");
    if first.is_empty() {
        return false;
    }
    if first.contains('=') && !first.starts_with("export") && !first.starts_with("./") {
        return false;
    }
    const RUNNABLE: &[&str] = &[
        "echo", "chmod", "chown", "cp", "mv", "rm", "mkdir", "touch", "cd", "pwd", "ls", "cat",
        "dig", "curl", "wget", "ping", "whois", "nslookup", "host", "bash", "sh", "zsh", "python",
        "python3", "node", "npm", "yarn", "pip", "pip3", "apt", "apt-get", "yum", "dnf", "brew",
        "systemctl", "docker", "podman", "kubectl", "ssh", "scp", "rsync", "tar", "grep", "awk",
        "sed", "tee", "sudo", "export",
    ];
    if first.starts_with("./") {
        return true;
    }
    RUNNABLE.contains(&first)
}

fn prompt_line_to_command(t: &str) -> Option<String> {
    if (t.starts_with('$') || t.starts_with('#')) && t.len() > 2 {
        let cmd = t.trim_start_matches(['$', '#', ' ']);
        if !cmd.is_empty() {
            return Some(cmd.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_long_context() {
        let body = (0..500)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let prep = prepare_terminal_context(&body);
        assert!(prep.truncated);
        assert_eq!(prep.line_count, AI_CONTEXT_MAX_LINES);
        assert!(prep.original_line_count > AI_CONTEXT_MAX_LINES);
    }

    #[test]
    fn skips_whole_script_in_fence() {
        let reply = r#"说明
```bash
#!/bin/bash
DOMAIN=$1
if [ -z "$DOMAIN" ]; then
  echo usage
  exit 1
fi
dig +short A $DOMAIN
```
"#;
        assert!(extract_shell_commands(reply).is_empty());
    }

    #[test]
    fn extracts_short_runnable_block() {
        let reply = r#"运行：
```bash
chmod +x check_domain.sh
./check_domain.sh example.com
```
"#;
        let cmds = extract_shell_commands(reply);
        assert_eq!(cmds.len(), 2);
        assert!(cmds.iter().any(|c| c.starts_with("chmod")));
        assert!(cmds.iter().any(|c| c.starts_with("./")));
    }
}

pub fn chat_completions(
    settings: &AiSettings,
    messages: &[ChatMessage],
) -> Result<String, String> {
    let api_key = settings
        .load_api_key()
        .ok_or_else(|| "未配置 API Key（请在 AI 面板填写并保存）".to_string())?;
    chat_completions_with_key(settings, &api_key, messages)
}

pub fn chat_completions_with_key(
    settings: &AiSettings,
    api_key: &str,
    messages: &[ChatMessage],
) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Err("API Key is empty".to_string());
    }
    let url = settings.chat_completions_url();
    let api_messages: Vec<ApiMessage> = std::iter::once(ApiMessage {
        role: "system",
        content: SYSTEM_PROMPT,
    })
    .chain(messages.iter().map(|m| ApiMessage {
        role: m.role.as_str(),
        content: m.content.as_str(),
    }))
    .collect();
    let body = ChatRequest {
        model: settings.model.trim(),
        messages: api_messages,
        temperature: 0.2,
        max_tokens: settings.max_tokens,
    };
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(settings.timeout_secs.max(5)))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&body)
        .send()
        .map_err(|e| format!("网络错误：{e}"))?;
    let status = resp.status();
    let text = resp.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        if let Ok(err) = serde_json::from_str::<ApiErrorBody>(&text) {
            if let Some(msg) = err.error.and_then(|e| e.message) {
                return Err(format!("API {}：{msg}", status.as_u16()));
            }
        }
        return Err(format!("API {}：{text}", status.as_u16()));
    }
    let parsed: ChatResponse = serde_json::from_str(&text).map_err(|e| format!("解析响应失败：{e}"))?;
    parsed
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .ok_or_else(|| "模型返回为空".to_string())
}

pub fn test_connection(settings: &AiSettings) -> Result<(), String> {
    let api_key = settings
        .load_api_key()
        .ok_or_else(|| "请先填写 API Key".to_string())?;
    test_connection_with_key(settings, &api_key)
}

pub fn test_connection_with_key(settings: &AiSettings, api_key: &str) -> Result<(), String> {
    chat_completions_with_key(
        settings,
        api_key,
        &[ChatMessage {
            role: "user".to_string(),
            content: "reply with ok".to_string(),
        }],
    )
    .map(|_| ())
}
