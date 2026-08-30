//! OpenAI 兼容 Chat Completions 客户端（阻塞 HTTP，供后台线程调用）。

use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::core::AiSettings;

/// 中文模型下的默认 system prompt（`AiSettings.system_prompt` 为空时使用）。
///
/// 要求模型使用固定章节结构：结论、关键点、风险、下一步、建议命令，
/// 便于 [`extract_shell_commands`] 自动从「建议命令」章节提取可执行命令。
pub const DEFAULT_SYSTEM_PROMPT: &str = "你是 MistTerm 终端里的运维助手。用户会提问或附上终端输出。\
请用简洁中文回答，并固定使用这些小节：结论、关键点、风险、下一步、建议命令（没有命令可省略）。\
先给 1 句结论，再用短小要点列出关键原因、风险和下一步。避免长段落；每个要点尽量不超过 2 行；不要把普通字段都包成行内代码。\
需要用户立刻执行时，把命令放在最后的「建议命令」小节。若给出完整 shell 脚本，请用单个 ```bash 代码块包裹整段脚本；\
若给出若干条可直接执行的命令，用 ```bash 代码块列出，每行一条命令，不要与完整脚本混在同一提取逻辑里。不要编造未提供的输出。";

/// OpenAI Chat Completions API 中的单条消息角色 + 正文（未区分 system/user/assistant 外的角色）。
#[derive(Clone, Debug)]
pub struct ChatMessage {
    /// OpenAI 角色：`system` / `user` / `assistant`。
    pub role: String,
    /// 消息正文（多行纯文本或 Markdown，不做长度校验）。
    pub content: String,
}

/// 流式或非流式对话进度（后台线程 → UI）。
#[derive(Clone, Debug)]
pub enum ChatEvent {
    /// 流式传输中又到达一段增量 token 文本；可直接 append 到缓冲区。
    Delta(String),
    /// 对话结束；不再有任何事件。
    Finished,
    /// 后端明确返回错误或网络层失败。字段是可读的错误说明（包含 HTTP 状态码时已被格式化）。
    Failed(String),
    /// 用户在 UI 点击「停止」时触发（`cancel` 原子标志为 true）。
    Cancelled,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ApiMessage<'a>>,
    temperature: f32,
    max_tokens: u32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
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
struct StreamChoice {
    delta: Option<StreamDelta>,
}

#[derive(Deserialize)]
struct StreamDelta {
    content: Option<String>,
}

#[derive(Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Deserialize)]
struct ApiErrorBody {
    error: Option<ApiErrorDetail>,
}

#[derive(Deserialize)]
struct ApiErrorDetail {
    message: Option<String>,
}

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

/// 发往模型的终端上下文行数上限（超出截断）。
pub const AI_CONTEXT_MAX_LINES: usize = 400;
/// 发往模型的终端上下文字符上限（超出截断）。
pub const AI_CONTEXT_MAX_CHARS: usize = 24_000;

/// 终端选区经脱敏与体积限制后的结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedTerminalContext {
    /// 脱敏并截断后的最终文本（trim 过；空表示无需附带）。
    pub text: String,
    /// 截断后的行数（0..=[`AI_CONTEXT_MAX_LINES`]）。
    pub line_count: usize,
    /// 截断后的字符数（0..=[`AI_CONTEXT_MAX_CHARS`]）。
    pub char_count: usize,
    /// `true` 表示由于行数或字符数限制做了尾部截断；UI 会给用户提示。
    pub truncated: bool,
    /// 原始脱敏后的行数（用于展示「已附带 N 行中的 M 行」）。
    pub original_line_count: usize,
    /// 原始脱敏后的字符数。
    pub original_char_count: usize,
}

/// 对终端输出执行脱敏（IP/密码/Token/JWT/AWS Key/邮箱等模式化字段），然后按
/// [`AI_CONTEXT_MAX_LINES`] / [`AI_CONTEXT_MAX_CHARS`] 截断保留末尾。
///
/// 脱敏不保证 100%，仅降低常见误上传风险；AI 对话中用户仍能看到真实输出。
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

/// 脱敏后再发往模型（多轮替换 + 常见密钥模式）。
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
        "secret=",
        "SECRET=",
    ];
    for _ in 0..8 {
        let mut changed = false;
        for n in NEEDLES {
            while let Some(i) = out.find(n) {
                let end = out[i..]
                    .find(|c: char| c.is_whitespace() || c == '\n' || c == '"' || c == '\'')
                    .map(|o| i + o)
                    .unwrap_or(out.len().min(i + 64));
                out.replace_range(i..end, "[REDACTED]");
                changed = true;
            }
        }
        if let Ok(re) = Regex::new(r"AKIA[0-9A-Z]{16}") {
            out = re.replace_all(&out, "[REDACTED_AWS_KEY]").into_owned();
            changed = true;
        }
        if let Ok(re) = Regex::new(r"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}") {
            out = re.replace_all(&out, "[REDACTED_JWT]").into_owned();
            changed = true;
        }
        if let Ok(re) = Regex::new(
            r"-----BEGIN (?:OPENSSH |RSA |EC |DSA )?PRIVATE KEY-----[\s\S]*?-----END (?:OPENSSH |RSA |EC |DSA )?PRIVATE KEY-----",
        ) {
            out = re.replace_all(&out, "[REDACTED_PRIVATE_KEY]").into_owned();
            changed = true;
        }
        if !changed {
            break;
        }
    }
    out
}

/// 使用 [`DEFAULT_SYSTEM_PROMPT`]，除非 [`AiSettings::system_prompt`] 已有非空白自定义值。
pub fn resolve_system_prompt(settings: &AiSettings) -> String {
    let custom = settings.system_prompt.trim();
    if custom.is_empty() {
        DEFAULT_SYSTEM_PROMPT.to_string()
    } else {
        custom.to_string()
    }
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
    cmds.retain(|c| is_runnable_shell_command(c));
    cmds.sort();
    cmds.dedup();
    cmds
}

/// 是否像可在终端单独执行的一条 shell 命令（过滤小节标题等误提取）。
pub fn is_runnable_shell_command(cmd: &str) -> bool {
    let line = cmd
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim())
        .unwrap_or("");
    if line.is_empty() {
        return false;
    }
    looks_like_runnable_line(line) || line.contains('|')
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
    let cmd = if let Some(rest) = t.strip_prefix('$') {
        rest.trim()
    } else if t.starts_with('#') && !t.starts_with("##") {
        t.trim_start_matches(['#', ' ']).trim()
    } else {
        return None;
    };
    if cmd.is_empty() || !is_runnable_shell_command(cmd) {
        return None;
    }
    Some(cmd.to_string())
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
    fn ignores_markdown_section_headings_as_commands() {
        let reply = r#"## 建议命令
```bash
ls -1A | awk '{print length"\t"$0}' | sort -nr | head -n 5
```
"#;
        let cmds = extract_shell_commands(reply);
        assert_eq!(cmds.len(), 1);
        assert!(cmds[0].contains("ls -1A"));
        assert!(!cmds.iter().any(|c| c.contains("建议命令")));
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

    #[test]
    fn redact_jwt_and_aws_key() {
        let raw = "key=AKIAIOSFODNN7EXAMPLE token=eyJhbGciOiJIUzI1NiJ9.abc.def";
        let out = redact_for_ai(raw);
        assert!(!out.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(!out.contains("eyJhbGci"));
    }

    #[test]
    fn retryable_transport_error_matches_network_prefix() {
        assert!(super::is_retryable_transport_error("网络错误：connection refused"));
        assert!(!super::is_retryable_transport_error("API 401：unauthorized"));
    }

    #[test]
    fn parse_blocking_chat_body_rejects_empty() {
        let err = super::parse_blocking_chat_body("").unwrap_err();
        assert!(err.contains("空响应"));
    }

    #[test]
    fn parse_blocking_chat_body_extracts_message() {
        let json = r#"{"choices":[{"message":{"role":"assistant","content":"你好"}}]}"#;
        assert_eq!(
            super::parse_blocking_chat_body(json).unwrap(),
            "你好".to_string()
        );
    }

    #[test]
    fn parse_blocking_chat_body_rejects_html() {
        let err = super::parse_blocking_chat_body("<html>403</html>").unwrap_err();
        assert!(err.contains("HTML"));
    }

    #[test]
    fn parse_models_response_sorts_and_dedups() {
        let json = r#"{"data":[{"id":"gpt-4o"},{"id":"gpt-4o-mini"},{"id":"gpt-4o"}]}"#;
        let ids = parse_models_response(json).expect("parse");
        assert_eq!(ids, vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()]);
    }

    #[test]
    fn parse_models_response_rejects_empty_list() {
        let json = r#"{"data":[]}"#;
        assert!(parse_models_response(json).is_err());
    }
}

/// 同步阻塞调用 `/chat/completions`（非流式）；API Key 从 settings 的密钥链加载。
///
/// 本质是构造 `run_chat_with_key(..., force_blocking=true)` 并收集所有 Delta 后返回整段正文。
/// 流式 UI 请不要调用此函数，直接在后台线程使用 [`run_chat_with_key`]。
pub fn chat_completions(
    settings: &AiSettings,
    messages: &[ChatMessage],
) -> Result<String, String> {
    let api_key = settings
        .load_api_key()
        .ok_or_else(|| "未配置 API Key（请在 AI 面板填写并保存）".to_string())?;
    chat_completions_with_key(settings, &api_key, messages)
}

/// 同 [`chat_completions`]，但允许调用方显式传入 API Key（用于「测试连接」流程中用户尚未保存的场景）。
pub fn chat_completions_with_key(
    settings: &AiSettings,
    api_key: &str,
    messages: &[ChatMessage],
) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Err("API Key is empty".to_string());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let cancel = AtomicBool::new(false);
    run_chat_with_key(settings, api_key, messages, &cancel, &tx, true, None);
    let mut full = String::new();
    loop {
        match rx.recv() {
            Ok(ChatEvent::Delta(d)) => full.push_str(&d),
            Ok(ChatEvent::Finished) => return Ok(full),
            Ok(ChatEvent::Failed(e)) => return Err(e),
            Ok(ChatEvent::Cancelled) => return Err("Request cancelled".to_string()),
            Err(_) => return Err("Request interrupted".to_string()),
        }
    }
}

/// 后台线程入口：按 [`AiSettings::stream_responses`] 选择流式或阻塞调用。
///
/// - `cancel` 设为 `true` 时流式读取会在下次 chunk 后发送 [`ChatEvent::Cancelled`] 并返回。
/// - `force_blocking=true` 时忽略配置，直接走阻塞模式（被 [`chat_completions_with_key`] 使用）。
/// - `system_prompt_override` 可覆盖默认 system prompt（`None` 时使用 [`resolve_system_prompt`]）。
///
/// 该函数不返回结果，所有事件都通过 `tx` 通道发送；UI 侧在循环中接收。
pub fn run_chat_with_key(
    settings: &AiSettings,
    api_key: &str,
    messages: &[ChatMessage],
    cancel: &AtomicBool,
    tx: &Sender<ChatEvent>,
    force_blocking: bool,
    system_prompt_override: Option<String>,
) {
    let result = if settings.stream_responses && !force_blocking {
        chat_streaming_with_key(settings, api_key, messages, cancel, tx, system_prompt_override.as_deref())
    } else {
        chat_blocking_with_key(settings, api_key, messages, cancel, tx, system_prompt_override.as_deref())
    };
    if let Err(e) = result {
        let _ = tx.send(ChatEvent::Failed(e));
    }
}

fn http_client(settings: &AiSettings) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(settings.timeout_secs.max(5)))
        .build()
        .map_err(|e| e.to_string())
}

fn is_retryable_transport_error(err: &str) -> bool {
    err.starts_with("网络错误：")
}

fn send_with_retries<F>(settings: &AiSettings, mut send_once: F) -> Result<reqwest::blocking::Response, String>
where
    F: FnMut() -> Result<reqwest::blocking::Response, reqwest::Error>,
{
    let max = settings.request_retries;
    let mut attempt = 0u32;
    loop {
        match send_once() {
            Ok(resp) => return Ok(resp),
            Err(e) => {
                let msg = format!("网络错误：{e}");
                if attempt >= max || !is_retryable_transport_error(&msg) {
                    return Err(msg);
                }
                attempt += 1;
                thread::sleep(Duration::from_millis(400 * u64::from(attempt)));
            }
        }
    }
}

fn chat_blocking_with_key(
    settings: &AiSettings,
    api_key: &str,
    messages: &[ChatMessage],
    cancel: &AtomicBool,
    tx: &Sender<ChatEvent>,
    system_prompt_override: Option<&str>,
) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        let _ = tx.send(ChatEvent::Cancelled);
        return Ok(());
    }
    let url = settings.chat_completions_url();
    let system = system_prompt_override
        .map(str::to_string)
        .unwrap_or_else(|| resolve_system_prompt(settings));
    let api_messages: Vec<ApiMessage> = std::iter::once(ApiMessage {
        role: "system",
        content: system.as_str(),
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
        stream: false,
    };
    let client = http_client(settings)?;
    let resp = send_with_retries(settings, || {
        client
            .post(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .json(&body)
            .send()
    })?;
    if cancel.load(Ordering::Relaxed) {
        let _ = tx.send(ChatEvent::Cancelled);
        return Ok(());
    }
    let status = resp.status();
    let text = resp.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(parse_api_error(status.as_u16(), &text));
    }
    let reply = parse_blocking_chat_body(&text)?;
    emit_blocking_reply(tx, reply);
    Ok(())
}

fn chat_streaming_with_key(
    settings: &AiSettings,
    api_key: &str,
    messages: &[ChatMessage],
    cancel: &AtomicBool,
    tx: &Sender<ChatEvent>,
    system_prompt_override: Option<&str>,
) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        let _ = tx.send(ChatEvent::Cancelled);
        return Ok(());
    }
    let url = settings.chat_completions_url();
    let system = system_prompt_override
        .map(str::to_string)
        .unwrap_or_else(|| resolve_system_prompt(settings));
    let api_messages: Vec<ApiMessage> = std::iter::once(ApiMessage {
        role: "system",
        content: system.as_str(),
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
        stream: true,
    };
    let client = http_client(settings)?;
    let mut resp = send_with_retries(settings, || {
        client
            .post(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Accept", "text/event-stream")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
    })?;
    if cancel.load(Ordering::Relaxed) {
        let _ = tx.send(ChatEvent::Cancelled);
        return Ok(());
    }
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().map_err(|e| e.to_string())?;
        return Err(parse_api_error(status.as_u16(), &text));
    }
    let mut reader = BufReader::new(resp.by_ref());
    let mut line = String::new();
    let mut got_delta = false;
    let mut raw_body = String::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = tx.send(ChatEvent::Cancelled);
            return Ok(());
        }
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        raw_body.push_str(&line);
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(':') {
            continue;
        }
        let payload = trimmed.strip_prefix("data:").map(str::trim).unwrap_or(trimmed);
        if payload == "[DONE]" {
            break;
        }
        if let Ok(chunk) = serde_json::from_str::<StreamChunk>(payload) {
            for choice in chunk.choices {
                if let Some(delta) = choice.delta.and_then(|d| d.content).filter(|s| !s.is_empty()) {
                    got_delta = true;
                    let _ = tx.send(ChatEvent::Delta(delta));
                }
            }
        }
    }
    if !got_delta {
        // 部分网关忽略 stream=true，直接返回整段 JSON；先尝试解析已读 body，避免重复请求。
        match parse_blocking_chat_body(&raw_body) {
            Ok(reply) => {
                emit_blocking_reply(tx, reply);
                return Ok(());
            }
            Err(_e) if raw_body.trim().is_empty() => {
                // 空 SSE 体时再回退非流式请求。
                return chat_blocking_with_key(
                    settings,
                    api_key,
                    messages,
                    cancel,
                    tx,
                    system_prompt_override,
                );
            }
            Err(e) => return Err(e),
        }
    }
    let _ = tx.send(ChatEvent::Finished);
    Ok(())
}

fn format_parse_error(text: &str, err: serde_json::Error) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "API 返回空响应，请检查 Base URL（需含 /v1）、API Key 与模型名称".to_string();
    }
    if trimmed.starts_with('<') {
        return "API 返回了 HTML 页面而非 JSON，请检查 Base URL 是否正确".to_string();
    }
    let preview: String = trimmed.chars().take(160).collect();
    format!("解析响应失败：{err}；响应开头：{preview}")
}

fn parse_blocking_chat_body(text: &str) -> Result<String, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(
            "API 返回空响应，请检查 Base URL（需含 /v1）、API Key 与模型名称".to_string(),
        );
    }
    let parsed: ChatResponse =
        serde_json::from_str(trimmed).map_err(|e| format_parse_error(trimmed, e))?;
    parsed
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .ok_or_else(|| "模型返回为空".to_string())
}

fn emit_blocking_reply(tx: &Sender<ChatEvent>, reply: String) {
    if !reply.is_empty() {
        let _ = tx.send(ChatEvent::Delta(reply));
    }
    let _ = tx.send(ChatEvent::Finished);
}

fn parse_api_error(status: u16, text: &str) -> String {
    if let Ok(err) = serde_json::from_str::<ApiErrorBody>(text) {
        if let Some(msg) = err.error.and_then(|e| e.message) {
            return format!("API {status}：{msg}");
        }
    }
    format!("API {status}：{text}")
}

/// 从 `/models` 接口拉取当前 API Key 可访问的模型 ID 列表（已按字典序去重）。
///
/// API Key 从 settings 的密钥链加载；若未配置则返回错误。
pub fn fetch_models(settings: &AiSettings) -> Result<Vec<String>, String> {
    let api_key = settings
        .load_api_key()
        .ok_or_else(|| "请先填写 API Key".to_string())?;
    fetch_models_with_key(settings, &api_key)
}

/// 同 [`fetch_models`]，但允许显式传入 API Key（用于「保存前预览模型」）。
pub fn fetch_models_with_key(settings: &AiSettings, api_key: &str) -> Result<Vec<String>, String> {
    if api_key.trim().is_empty() {
        return Err("API Key is empty".to_string());
    }
    let client = http_client(settings)?;
    let resp = send_with_retries(settings, || {
        client
            .get(settings.models_url())
            .header("Authorization", format!("Bearer {api_key}"))
            .send()
    })?;
    let status = resp.status();
    let text = resp.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(parse_api_error(status.as_u16(), &text));
    }
    parse_models_response(&text)
}

fn parse_models_response(text: &str) -> Result<Vec<String>, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(
            "API 返回空响应，请检查 Base URL（需含 /v1）与 API Key".to_string(),
        );
    }
    let parsed: ModelsResponse =
        serde_json::from_str(trimmed).map_err(|e| format_parse_error(trimmed, e))?;
    let mut ids: Vec<String> = parsed
        .data
        .into_iter()
        .map(|m| m.id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() {
        return Err("模型列表为空".to_string());
    }
    Ok(ids)
}

/// 快速连通性测试：调用一次 `/models` 接口，成功返回 `Ok(())`，失败返回带状态码的中文说明。
///
/// API Key 从 settings 的密钥链加载。
pub fn test_connection(settings: &AiSettings) -> Result<(), String> {
    let api_key = settings
        .load_api_key()
        .ok_or_else(|| "请先填写 API Key".to_string())?;
    test_connection_with_key(settings, &api_key)
}

/// 同 [`test_connection`]，允许显式传入 API Key（用于 AI 面板「测试连接」按钮，用户尚未保存）。
pub fn test_connection_with_key(settings: &AiSettings, api_key: &str) -> Result<(), String> {
    if api_key.trim().is_empty() {
        return Err("API Key is empty".to_string());
    }
    let client = http_client(settings)?;
    let resp = send_with_retries(settings, || {
        client
            .get(settings.models_url())
            .header("Authorization", format!("Bearer {api_key}"))
            .send()
    })?;
    if resp.status().is_success() {
        return Ok(());
    }
    chat_completions_with_key(
        settings,
        api_key,
        &[ChatMessage {
            role: "user".to_string(),
            content: "ping".to_string(),
        }],
    )
    .map(|_| ())
}
