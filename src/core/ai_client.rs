//! OpenAI 兼容 Chat Completions 客户端（阻塞 HTTP，供后台线程调用）。

use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::core::AiSettings;

pub const DEFAULT_SYSTEM_PROMPT: &str = "你是 MistTerm 终端里的运维助手。用户会提问或附上终端输出。\
请用简洁中文回答，并固定使用这些小节：结论、关键点、风险、下一步、建议命令（没有命令可省略）。\
先给 1 句结论，再用短小要点列出关键原因、风险和下一步。避免长段落；每个要点尽量不超过 2 行；不要把普通字段都包成行内代码。\
需要用户立刻执行时，把命令放在最后的「建议命令」小节。若给出完整 shell 脚本，请用单个 ```bash 代码块包裹整段脚本；\
若给出若干条可直接执行的命令，用 ```bash 代码块列出，每行一条命令，不要与完整脚本混在同一提取逻辑里。不要编造未提供的输出。";

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// 流式或非流式对话进度（后台线程 → UI）。
#[derive(Clone, Debug)]
pub enum ChatEvent {
    Delta(String),
    Finished,
    Failed(String),
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
    let (tx, rx) = std::sync::mpsc::channel();
    let cancel = AtomicBool::new(false);
    run_chat_with_key(settings, api_key, messages, &cancel, &tx, true);
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

/// 后台线程入口：按设置走流式或整段响应。
pub fn run_chat_with_key(
    settings: &AiSettings,
    api_key: &str,
    messages: &[ChatMessage],
    cancel: &AtomicBool,
    tx: &Sender<ChatEvent>,
    force_blocking: bool,
) {
    let result = if settings.stream_responses && !force_blocking {
        chat_streaming_with_key(settings, api_key, messages, cancel, tx)
    } else {
        chat_blocking_with_key(settings, api_key, messages, cancel, tx)
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
) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        let _ = tx.send(ChatEvent::Cancelled);
        return Ok(());
    }
    let url = settings.chat_completions_url();
    let system = resolve_system_prompt(settings);
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
    let parsed: ChatResponse = serde_json::from_str(&text).map_err(|e| format!("解析响应失败：{e}"))?;
    let reply = parsed
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .ok_or_else(|| "模型返回为空".to_string())?;
    if !reply.is_empty() {
        let _ = tx.send(ChatEvent::Delta(reply));
    }
    let _ = tx.send(ChatEvent::Finished);
    Ok(())
}

fn chat_streaming_with_key(
    settings: &AiSettings,
    api_key: &str,
    messages: &[ChatMessage],
    cancel: &AtomicBool,
    tx: &Sender<ChatEvent>,
) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        let _ = tx.send(ChatEvent::Cancelled);
        return Ok(());
    }
    let url = settings.chat_completions_url();
    let system = resolve_system_prompt(settings);
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
        // 部分网关忽略 stream，回退整段请求
        return chat_blocking_with_key(settings, api_key, messages, cancel, tx);
    }
    let _ = tx.send(ChatEvent::Finished);
    Ok(())
}

fn parse_api_error(status: u16, text: &str) -> String {
    if let Ok(err) = serde_json::from_str::<ApiErrorBody>(text) {
        if let Some(msg) = err.error.and_then(|e| e.message) {
            return format!("API {status}：{msg}");
        }
    }
    format!("API {status}：{text}")
}

pub fn fetch_models(settings: &AiSettings) -> Result<Vec<String>, String> {
    let api_key = settings
        .load_api_key()
        .ok_or_else(|| "请先填写 API Key".to_string())?;
    fetch_models_with_key(settings, &api_key)
}

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
    let parsed: ModelsResponse =
        serde_json::from_str(text).map_err(|e| format!("解析响应失败：{e}"))?;
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

pub fn test_connection(settings: &AiSettings) -> Result<(), String> {
    let api_key = settings
        .load_api_key()
        .ok_or_else(|| "请先填写 API Key".to_string())?;
    test_connection_with_key(settings, &api_key)
}

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
