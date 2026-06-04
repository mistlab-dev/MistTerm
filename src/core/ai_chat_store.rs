//! AI 对话本地持久化（按会话 session_id 分文件）。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredContextRef {
    pub text: String,
    pub line_count: usize,
    pub char_count: usize,
    pub truncated: bool,
    pub original_line_count: usize,
    pub original_char_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAiMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_refs: Vec<StoredContextRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,
}

pub fn chat_store_dir() -> PathBuf {
    let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("mistterm");
    p.push("ai_chats");
    p
}

fn chat_file_path(session_key: &str) -> PathBuf {
    let safe: String = session_key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    chat_store_dir().join(format!("{safe}.json"))
}

pub fn load_chat(session_key: &str) -> Vec<StoredAiMessage> {
    let path = chat_file_path(session_key);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_chat(session_key: &str, messages: &[StoredAiMessage]) -> std::io::Result<()> {
    let dir = chat_store_dir();
    std::fs::create_dir_all(&dir)?;
    let path = chat_file_path(session_key);
    if messages.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    let json = serde_json::to_string_pretty(messages).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

pub fn delete_chat(session_key: &str) {
    let _ = std::fs::remove_file(chat_file_path(session_key));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_stored_message_json() {
        let msg = StoredAiMessage {
            role: "user".into(),
            content: "hi".into(),
            api_content: Some("hi\n\nctx".into()),
            context_refs: vec![StoredContextRef {
                text: "err".into(),
                line_count: 1,
                char_count: 3,
                truncated: false,
                original_line_count: 1,
                original_char_count: 3,
                source_key: None,
            }],
            commands: vec![],
        };
        let json = serde_json::to_string(&msg).unwrap();
        let back: StoredAiMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.content, "hi");
        assert_eq!(back.context_refs.len(), 1);
    }
}
