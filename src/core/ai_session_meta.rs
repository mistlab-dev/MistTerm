//! 组装发往 AI 的终端会话元信息。

#[derive(Clone, Debug, Default)]
pub struct TerminalSessionMeta {
    pub host: Option<String>,
    pub username: Option<String>,
    pub session_name: Option<String>,
}

impl TerminalSessionMeta {
    pub fn format_block(&self) -> Option<String> {
        if self.host.is_none() && self.username.is_none() && self.session_name.is_none() {
            return None;
        }
        let mut lines = vec!["--- Session ---".to_string()];
        if let Some(name) = &self.session_name {
            lines.push(format!("session: {name}"));
        }
        if let (Some(u), Some(h)) = (&self.username, &self.host) {
            lines.push(format!("target: {u}@{h}"));
        } else if let Some(h) = &self.host {
            lines.push(format!("host: {h}"));
        }
        Some(lines.join("\n"))
    }
}
