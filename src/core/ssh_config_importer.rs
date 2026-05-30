//! 解析 OpenSSH `~/.ssh/config`，检测可导入的 Host 块。

use std::path::{Path, PathBuf};

use super::session::SessionConfig;

/// 从 ssh config 解析出的单条候选
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConfigCandidate {
    /// `Host` 指令的第一个值（会话名 / 标记）
    pub host_alias: String,
    pub hostname: Option<String>,
    pub port: u16,
    pub username: String,
    pub identity_file: String,
    pub proxy_jump: String,
    pub proxy_command: String,
    /// 无法导入的原因（HostName 缺失等）
    pub skip_reason: Option<String>,
}

impl SshConfigCandidate {
    pub fn importable(&self) -> bool {
        self.skip_reason.is_none() && self.hostname.is_some()
    }

    pub fn marker_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.host_alias,
            self.hostname.as_deref().unwrap_or(""),
            self.port
        )
    }

    pub fn display_target(&self) -> String {
        if let Some(h) = &self.hostname {
            let mut s = format!("{}:{}", h, self.port);
            if !self.proxy_jump.is_empty() {
                s.push_str(&format!(" (Jump {})", self.proxy_jump));
            }
            s
        } else {
            "(HostName 缺失)".to_string()
        }
    }
}

/// 解析结果（含非致命警告）
#[derive(Debug, Clone, Default)]
pub struct SshConfigParseResult {
    pub candidates: Vec<SshConfigCandidate>,
    pub warnings: Vec<String>,
}

/// 默认 ssh config 路径
pub fn default_ssh_config_path() -> PathBuf {
    crate::platform::default_ssh_config_path()
}

/// 读取并解析 ssh config 文件
pub fn parse_ssh_config_file(path: &Path) -> std::io::Result<SshConfigParseResult> {
    let content = std::fs::read_to_string(path)?;
    Ok(parse_ssh_config_str(&content))
}

/// 解析文本（便于单测）
pub fn parse_ssh_config_str(content: &str) -> SshConfigParseResult {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let mut current: Option<SshConfigCandidate> = None;

    let flush = |current: &mut Option<SshConfigCandidate>, out: &mut Vec<SshConfigCandidate>| {
        if let Some(c) = current.take() {
            if should_import_host_alias(&c.host_alias) {
                let mut c = c;
                if c.hostname.is_none() {
                    c.skip_reason = Some("HostName 缺失".to_string());
                }
                out.push(c);
            }
        }
    };

    for (line_no, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let lower = trimmed.to_lowercase();
        if lower.starts_with("host ") {
            flush(&mut current, &mut out);
            let alias = trimmed[5..]
                .trim()
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            current = Some(SshConfigCandidate {
                host_alias: alias,
                hostname: None,
                port: 22,
                username: String::new(),
                identity_file: String::new(),
                proxy_jump: String::new(),
                proxy_command: String::new(),
                skip_reason: None,
            });
            continue;
        }
        let Some(ref mut block) = current else {
            if split_ssh_directive(trimmed).is_some() {
                warnings.push(format!(
                    "第 {} 行：指令在 Host 块之外，已忽略",
                    line_no + 1
                ));
            }
            continue;
        };
        if let Some((key, value)) = split_ssh_directive(trimmed) {
            match key.to_lowercase().as_str() {
                "hostname" => block.hostname = Some(value.to_string()),
                "port" => {
                    if let Ok(p) = value.parse::<u16>() {
                        block.port = p;
                    } else {
                        warnings.push(format!("第 {} 行：无效 Port «{}»", line_no + 1, value));
                    }
                }
                "user" => block.username = value.to_string(),
                "identityfile" => block.identity_file = expand_tilde(value),
                "proxyjump" => block.proxy_jump = value.to_string(),
                "proxycommand" => block.proxy_command = value.to_string(),
                _ => {}
            }
        } else {
            warnings.push(format!("第 {} 行：无法解析 «{}»", line_no + 1, trimmed));
        }
    }
    flush(&mut current, &mut out);
    SshConfigParseResult { candidates: out, warnings }
}

fn split_ssh_directive(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.splitn(2, |c: char| c == ' ' || c == '\t');
    let key = parts.next()?.trim();
    let value = parts.next()?.trim().trim_matches('"');
    if key.is_empty() {
        return None;
    }
    Some((key, value))
}

fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = crate::platform::home_dir() {
            return home.join(rest).to_string_lossy().into_owned();
        }
    }
    path.to_string()
}

fn should_import_host_alias(alias: &str) -> bool {
    if alias.is_empty() || alias == "*" {
        return false;
    }
    if alias.contains('*') || alias.contains('?') {
        return false;
    }
    true
}

/// 是否已从已有会话导入
pub fn is_already_imported(c: &SshConfigCandidate, existing: &[SessionConfig]) -> bool {
    let key = c.marker_key();
    existing
        .iter()
        .any(|s| s.ssh_config_marker.as_deref() == Some(key.as_str()))
}

/// 相对已有会话，筛出尚未导入的可导入条目
pub fn pending_imports<'a>(
    candidates: &'a [SshConfigCandidate],
    existing: &[SessionConfig],
) -> Vec<&'a SshConfigCandidate> {
    candidates
        .iter()
        .filter(|c| c.importable() && !is_already_imported(c, existing))
        .collect()
}

/// 将候选转为新 `SessionConfig`（名称去重）
pub fn candidate_to_session(
    c: &SshConfigCandidate,
    existing_names: &[String],
) -> SessionConfig {
    let host = c.hostname.clone().unwrap_or_default();
    let mut name = c.host_alias.clone();
    let mut n = 2;
    while existing_names.iter().any(|x| x == &name) {
        name = format!("{} ({})", c.host_alias, n);
        n += 1;
    }
    let mut cfg = SessionConfig::default();
    cfg.name = name;
    cfg.host = host;
    cfg.port = c.port;
    cfg.username = c.username.clone();
    cfg.private_key_path = c.identity_file.clone();
    cfg.proxy_jump = c.proxy_jump.clone();
    cfg.proxy_command = c.proxy_command.clone();
    cfg.ssh_config_marker = Some(c.marker_key());
    cfg.created_at = Some(chrono::Utc::now().timestamp());
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_host() {
        let text = r#"
Host prod
    HostName 10.0.0.1
    User admin
    Port 2222
    IdentityFile ~/.ssh/id_rsa
    ProxyJump bastion
"#;
        let r = parse_ssh_config_str(text);
        assert_eq!(r.candidates.len(), 1);
        assert_eq!(r.candidates[0].host_alias, "prod");
        assert_eq!(r.candidates[0].hostname.as_deref(), Some("10.0.0.1"));
        assert_eq!(r.candidates[0].port, 2222);
        assert_eq!(r.candidates[0].username, "admin");
        assert_eq!(r.candidates[0].proxy_jump, "bastion");
        assert!(r.candidates[0].importable());
    }

    #[test]
    fn skip_wildcard_and_missing_hostname() {
        let text = r#"
Host *
    HostName x
Host web-*
    HostName y
Host bad
    User u
"#;
        let r = parse_ssh_config_str(text);
        assert_eq!(r.candidates.len(), 1);
        assert_eq!(r.candidates[0].host_alias, "bad");
        assert!(!r.candidates[0].importable());
    }

    #[test]
    fn duplicate_name_suffix() {
        let c = SshConfigCandidate {
            host_alias: "prod".into(),
            hostname: Some("10.0.0.1".into()),
            port: 22,
            username: String::new(),
            identity_file: String::new(),
            proxy_jump: String::new(),
            proxy_command: String::new(),
            skip_reason: None,
        };
        let s = candidate_to_session(&c, &["prod".into()]);
        assert_eq!(s.name, "prod (2)");
    }
}
