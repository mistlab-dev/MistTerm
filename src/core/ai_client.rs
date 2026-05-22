//! OpenAI 兼容 Chat Completions 客户端（阻塞 HTTP，供后台线程调用）。

use serde::{Deserialize, Serialize};

use crate::core::AiSettings;

const SYSTEM_PROMPT: &str = "你是 MistTerm 终端里的运维助手。用户会提问或附上终端输出。\
请用简洁中文回答。若给出 shell 命令，请用 markdown 代码块包裹（```bash ... ```），每行一条可执行命令。\
不要编造未提供的输出。";

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

/// 从回复中提取 ``` 代码块内的 shell 命令行。
pub fn extract_shell_commands(reply: &str) -> Vec<String> {
    let mut cmds = Vec::new();
    let mut in_fence = false;
    for line in reply.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence && !t.is_empty() && !t.starts_with('#') {
            cmds.push(t.to_string());
        }
    }
    if cmds.is_empty() {
        for line in reply.lines() {
            let t = line.trim();
            if (t.starts_with('$') || t.starts_with('#'))
                && t.len() > 2
            {
                let cmd = t.trim_start_matches(['$', '#', ' ']);
                if !cmd.is_empty() {
                    cmds.push(cmd.to_string());
                }
            }
        }
    }
    cmds.dedup();
    cmds
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
        return Err("API Key 为空".to_string());
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
