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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_block_returns_none_when_all_empty() {
        let meta = TerminalSessionMeta::default();
        assert_eq!(meta.format_block(), None);
    }

    #[test]
    fn format_block_session_name_only() {
        let meta = TerminalSessionMeta {
            session_name: Some("prod-web-01".into()),
            ..Default::default()
        };
        let block = meta.format_block().expect("should have block");
        assert!(block.starts_with("--- Session ---"));
        assert!(block.contains("session: prod-web-01"));
        assert!(!block.contains("target:"));
        assert!(!block.contains("host:"));
    }

    #[test]
    fn format_block_host_only() {
        let meta = TerminalSessionMeta {
            host: Some("192.168.1.10".into()),
            ..Default::default()
        };
        let block = meta.format_block().unwrap();
        assert!(block.contains("host: 192.168.1.10"));
        assert!(!block.contains("target:"));
    }

    #[test]
    fn format_block_host_and_user_formats_target() {
        let meta = TerminalSessionMeta {
            host: Some("server.example".into()),
            username: Some("admin".into()),
            ..Default::default()
        };
        let block = meta.format_block().unwrap();
        assert!(block.contains("target: admin@server.example"));
        assert!(!block.contains("host:"));
    }

    #[test]
    fn format_block_username_without_host_shows_no_host_line() {
        // Username alone: there is no explicit username-only branch,
        // so format_block should still emit the header but no target/host line.
        let meta = TerminalSessionMeta {
            username: Some("alice".into()),
            ..Default::default()
        };
        let block = meta.format_block().unwrap();
        assert_eq!(block, "--- Session ---");
        assert!(!block.contains("target:"));
        assert!(!block.contains("host:"));
    }

    #[test]
    fn format_block_full_triplet() {
        let meta = TerminalSessionMeta {
            session_name: Some("DB Cluster".into()),
            host: Some("db01.internal".into()),
            username: Some("dba".into()),
        };
        let block = meta.format_block().unwrap();
        let lines: Vec<&str> = block.lines().collect();
        assert_eq!(lines[0], "--- Session ---");
        assert_eq!(lines[1], "session: DB Cluster");
        assert_eq!(lines[2], "target: dba@db01.internal");
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn format_block_session_name_and_host_only() {
        let meta = TerminalSessionMeta {
            session_name: Some("Bastion".into()),
            host: Some("bastion.corp".into()),
            ..Default::default()
        };
        let block = meta.format_block().unwrap();
        let lines: Vec<&str> = block.lines().collect();
        assert_eq!(lines[1], "session: Bastion");
        assert_eq!(lines[2], "host: bastion.corp");
    }
}
