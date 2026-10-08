//! 解析 `ssh://user@host:port` 链接（浏览器里点 ssh:// 链接时，系统会把它当参数传给 Mist）。
//!
//! 只取用户名、主机、端口。链接里带的密码（`ssh://user:pass@host`）一律丢弃，不使用也不保存。

/// 解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshUrl {
    pub user: Option<String>,
    pub host: String,
    pub port: u16,
    /// 链接里带了密码（已丢弃），界面上可以提醒一句。
    pub had_password: bool,
}

impl SshUrl {
    /// 显示用：`user@host` 或 `user@host:2222`。
    pub fn display(&self) -> String {
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        let user = self.user.as_deref().map(|u| format!("{u}@")).unwrap_or_default();
        if self.port == 22 {
            format!("{user}{host}")
        } else {
            format!("{user}{host}:{}", self.port)
        }
    }
}

/// 从启动参数里找第一个 `ssh://` 链接。
pub fn find_in_args<I, S>(args: I) -> Option<SshUrl>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter().find_map(|a| parse_ssh_url(a.as_ref()).ok())
}

/// 解析 `ssh://[user[;参数][:密码]@]host[:port][/]`。
pub fn parse_ssh_url(input: &str) -> Result<SshUrl, String> {
    let s = input.trim();
    let rest = s
        .get(..6)
        .filter(|p| p.eq_ignore_ascii_case("ssh://"))
        .map(|_| &s[6..])
        .ok_or_else(|| "不是 ssh:// 链接".to_string())?;
    // 去掉路径、查询、片段
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err("链接里没有主机".into());
    }
    let (userinfo, hostport) = match authority.rfind('@') {
        Some(i) => (Some(&authority[..i]), &authority[i + 1..]),
        None => (None, authority),
    };
    let mut had_password = false;
    let user = match userinfo {
        Some(ui) => {
            // RFC 草案允许 `user;fingerprint=...`；`user:pass` 里的密码丢弃
            let (u, pw) = match ui.split_once(':') {
                Some((u, _)) => (u, true),
                None => (ui, false),
            };
            had_password = pw;
            let u = u.split(';').next().unwrap_or("");
            let u = percent_decode(u)?;
            if u.is_empty() {
                None
            } else {
                if !valid_user(&u) {
                    return Err("用户名里有不允许的字符".into());
                }
                Some(u)
            }
        }
        None => None,
    };
    let (host, port) = if let Some(after) = hostport.strip_prefix('[') {
        let end = after.find(']').ok_or_else(|| "IPv6 地址缺少 ]".to_string())?;
        let host = &after[..end];
        let tail = &after[end + 1..];
        let port = match tail.strip_prefix(':') {
            Some(p) => parse_port(p)?,
            None if tail.is_empty() => 22,
            None => return Err("主机后面多了字符".into()),
        };
        (host.to_string(), port)
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), parse_port(p)?),
            None => (hostport.to_string(), 22),
        }
    };
    let host = percent_decode(&host)?;
    if host.is_empty() {
        return Err("链接里没有主机".into());
    }
    if !valid_host(&host) {
        return Err("主机名里有不允许的字符".into());
    }
    Ok(SshUrl { user, host, port, had_password })
}

fn parse_port(p: &str) -> Result<u16, String> {
    if p.is_empty() {
        return Ok(22);
    }
    match p.parse::<u16>() {
        Ok(0) | Err(_) => Err(format!("端口不对：{p}")),
        Ok(n) => Ok(n),
    }
}

fn percent_decode(s: &str) -> Result<String, String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3).ok_or_else(|| "链接里的 % 编码不完整".to_string())?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| "链接里的 % 编码不对".to_string())?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "链接不是有效的 UTF-8".to_string())
}

/// 主机：域名、IPv4、IPv6；不能以 `-` 开头，不能有空白和控制字符。
fn valid_host(h: &str) -> bool {
    !h.starts_with('-')
        && h.len() <= 253
        && h.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '%') || (!c.is_ascii() && c.is_alphanumeric()))
}

fn valid_user(u: &str) -> bool {
    !u.starts_with('-')
        && u.len() <= 64
        && u.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '\\' | '$' | '@') || (!c.is_ascii() && c.is_alphanumeric()))
}

/// 在已保存会话里找对应的那个：主机（不分大小写）和端口一致；链接里有用户名时用户名也要一致。
/// 有多个时取最近连接过的。
pub fn match_saved_session<'a>(
    url: &SshUrl,
    sessions: &'a [crate::core::session::SessionConfig],
) -> Option<&'a crate::core::session::SessionConfig> {
    sessions
        .iter()
        .filter(|s| s.host.trim().eq_ignore_ascii_case(&url.host) && s.port == url.port)
        .filter(|s| url.user.as_deref().map_or(true, |u| s.username == u))
        .max_by_key(|s| s.last_connected_at.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::session::SessionConfig;

    fn sess(name: &str, user: &str, host: &str, port: u16, last: Option<i64>) -> SessionConfig {
        let mut s = SessionConfig::default();
        s.name = name.into();
        s.username = user.into();
        s.host = host.into();
        s.port = port;
        s.last_connected_at = last;
        s
    }

    #[test]
    fn matches_saved_sessions() {
        let list = vec![
            sess("a", "root", "Web-01.example.com", 22, Some(5)),
            sess("b", "deploy", "web-01.example.com", 22, Some(9)),
            sess("c", "root", "web-01.example.com", 2222, None),
        ];
        let m = |u: &str| match_saved_session(&parse_ssh_url(u).unwrap(), &list).map(|s| s.name.clone());
        assert_eq!(m("ssh://root@web-01.example.com"), Some("a".into()));
        assert_eq!(m("ssh://web-01.example.com"), Some("b".into()));
        assert_eq!(m("ssh://root@web-01.example.com:2222"), Some("c".into()));
        assert_eq!(m("ssh://nobody@web-01.example.com"), None);
        assert_eq!(m("ssh://root@web-02.example.com"), None);
    }

    fn ok(s: &str) -> SshUrl {
        parse_ssh_url(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    #[test]
    fn parses_common_forms() {
        assert_eq!(ok("ssh://root@10.0.0.1:22"), SshUrl { user: Some("root".into()), host: "10.0.0.1".into(), port: 22, had_password: false });
        assert_eq!(ok("ssh://example.com"), SshUrl { user: None, host: "example.com".into(), port: 22, had_password: false });
        assert_eq!(ok("SSH://deploy@web-01.prod.internal:2222/"), SshUrl { user: Some("deploy".into()), host: "web-01.prod.internal".into(), port: 2222, had_password: false });
        assert_eq!(ok("ssh://admin@[2001:db8::1]:2200").host, "2001:db8::1");
        assert_eq!(ok("ssh://admin@[2001:db8::1]").port, 22);
        assert_eq!(ok("ssh://me;fingerprint=ssh-ed25519-abc@host").user.as_deref(), Some("me"));
        assert_eq!(ok("ssh://first.last%40corp@host").user.as_deref(), Some("first.last@corp"));
        assert_eq!(ok("ssh://host:").port, 22);
        assert_eq!(ok("  ssh://u@h:2022?x=1#y  ").port, 2022);
    }

    #[test]
    fn password_in_link_is_dropped() {
        let u = ok("ssh://root:hunter2@10.0.0.1");
        assert_eq!(u.user.as_deref(), Some("root"));
        assert!(u.had_password);
        assert!(!u.display().contains("hunter2"));
    }

    #[test]
    fn rejects_bad_links() {
        for s in [
            "http://host",
            "ssh://",
            "ssh:///path",
            "ssh://host:99999",
            "ssh://host:0",
            "ssh://host:abc",
            "ssh://-oProxyCommand=evil",
            "ssh://-l@host",
            "ssh://user@host name",
            "ssh://user@ho%0Ast",
            "ssh://[::1",
            "ssh://a@b%zz",
        ] {
            assert!(parse_ssh_url(s).is_err(), "{s} should be rejected");
        }
    }

    #[test]
    fn finds_link_in_args() {
        let args = ["Mist.exe", "--flag", "ssh://ops@db:2201"];
        assert_eq!(find_in_args(args).unwrap().display(), "ops@db:2201");
        assert!(find_in_args(["Mist", "--version"]).is_none());
    }
}
