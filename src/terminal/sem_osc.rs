//! OSC 语义协议杂项解析（#392）：进度 9;4 / 当前目录 7+9;9 / 标题 0+2。
//!
//! 与 [`crate::terminal::program_status`]（#391）同构：纯函数解析载荷，
//! 由 `OscSplitter` 在流里识别前缀后调用。坏载荷按各自语义处理：
//! 进度/目录/标题解析失败即忽略（转义序列本身不可见，画面不受影响）。

/// OSC 9;4 进度报告（Windows Terminal 兼容）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgressReport {
    pub state: ProgressState,
    /// 0-100；indeterminate（3）下 spec 允许省略或填 0，统一存 `None`。
    pub progress: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressState {
    /// 0：移除进度指示。
    Remove,
    /// 1：正常。
    Normal,
    /// 2：错误。
    Error,
    /// 3：不定进度（转圈）。
    Indeterminate,
    /// 4：警告。
    Warning,
}

impl ProgressReport {
    /// `payload` = `9;4` 之后的部分（如 `1;50` 或 `3`）。
    pub fn parse(payload: &str) -> Option<ProgressReport> {
        let mut parts = payload.split(';');
        let state = match parts.next()?.trim() {
            "0" => ProgressState::Remove,
            "1" => ProgressState::Normal,
            "2" => ProgressState::Error,
            "3" => ProgressState::Indeterminate,
            "4" => ProgressState::Warning,
            _ => return None,
        };
        let progress = match parts.next() {
            Some(raw) => {
                let n: u8 = raw.trim().parse().ok()?;
                if n > 100 {
                    return None;
                }
                Some(n)
            }
            None => None,
        };
        let progress = if state == ProgressState::Indeterminate {
            None
        } else if state == ProgressState::Remove {
            None
        } else {
            progress
        };
        Some(ProgressReport { state, progress })
    }
}

/// 当前目录汇报：OSC 7（`file://host/path`）、OSC 9;9（裸路径）、
/// OSC 1337 `CurrentDir=`（iTerm2，裸路径）统一产物。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CwdReport {
    pub path: String,
    /// #401: OSC 7 载荷里的主机段（`file://host/path`）。解析层面只做净化不做语义判断：
    /// hostname 与配置里的地址字段本来就不保证一致（域名 vs 短名 vs 大小写）。
    /// 继承判定实际用的是连接后探测到的身份（host + 登录名），这里只把字段留住，
    /// 免得解析层丢信息。OSC 9;9 / 1337 不带主机 → `None`。
    pub host: Option<String>,
}

/// OSC 7 载荷（如 `file://myhost/home/u` 或 `file:///home/u`）→ 路径 + 主机名。
pub fn parse_osc7(payload: &str) -> Option<CwdReport> {
    let rest = payload.strip_prefix("file://")?;
    // file://host/path → 主机段 + 路径；file:///path → 主机为空。
    let (host, path) = match rest.split_once('/') {
        Some((host, path)) => (host, format!("/{path}")),
        None => return None,
    };
    let path = sanitize(percent_decode(&path))?;
    // #401: 主机段仅作继承判定参考，坏值退化为 None 而不是整条拒绝——
    // 路径本身仍然可用，丢掉一条 cwd 汇报对用户毫无收益。
    let host = if host.is_empty() || host.chars().any(char::is_control) {
        None
    } else {
        Some(host.to_string())
    };
    Some(CwdReport { path, host })
}

/// OSC 9;9 载荷是裸路径（可能有引号包裹）。
pub fn parse_osc99(payload: &str) -> Option<CwdReport> {
    let payload = payload
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(payload);
    Some(CwdReport {
        path: sanitize(payload.to_string())?,
        host: None,
    })
}

fn sanitize(path: String) -> Option<String> {
    // 复审软注：与标题同口径，拒一切控制字符（不只 \0）。
    if !path.starts_with('/') || path.chars().any(char::is_control) {
        return None;
    }
    Some(path)
}

/// #392 软注：OSC 7 载荷做百分号解码（`%20` 等）——常见空格目录原样失败。
/// 只解 `%XX`，非法序列原样保留。
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() + 1 && i + 2 < bytes.len() + 1 {
            let hex = bytes.get(i + 1..i + 3);
            if let Some(h) = hex {
                if let Ok(v) = u8::from_str_radix(std::str::from_utf8(h).unwrap_or(""), 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 标题汇报：OSC 0（icon+title）与 OSC 2（title）统一取整段为标题。
/// 终端标题惯例上允许任意文本（含空格），这里只裁掉控制字符并限长。
pub fn parse_title(payload: &str) -> Option<String> {
    let title = payload.trim();
    if title.is_empty() || title.chars().any(char::is_control) {
        return None;
    }
    if title.len() > 512 {
        return None;
    }
    Some(title.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_states_parse() {
        let got = ProgressReport::parse("1;50").unwrap();
        assert_eq!(got.state, ProgressState::Normal);
        assert_eq!(got.progress, Some(50));
        let got = ProgressReport::parse("2;100").unwrap();
        assert_eq!(got.state, ProgressState::Error);
        assert_eq!(got.progress, Some(100));
        let got = ProgressReport::parse("0").unwrap();
        assert_eq!(got.state, ProgressState::Remove);
        assert_eq!(got.progress, None);
    }

    #[test]
    fn progress_indeterminate_and_bounds() {
        let got = ProgressReport::parse("3;0").unwrap();
        assert_eq!(got.state, ProgressState::Indeterminate);
        assert_eq!(got.progress, None, "不定进度不存数值");
        assert!(ProgressReport::parse("1;101").is_none(), "越界整条忽略");
        assert!(ProgressReport::parse("9;50").is_none(), "未知 state 忽略");
        assert!(ProgressReport::parse("").is_none());
    }

    #[test]
    fn osc7_variants() {
        let got = parse_osc7("file://myhost/home/u/proj").unwrap();
        assert_eq!(got.path, "/home/u/proj");
        assert_eq!(got.host.as_deref(), Some("myhost"), "主机段要留下来给继承判定");
        let got = parse_osc7("file:///home/u").unwrap();
        assert_eq!(got.path, "/home/u");
        assert_eq!(got.host, None, "file:///path 形式没有主机名");
        assert!(parse_osc7("not-a-file-url").is_none());
        assert!(parse_osc7("file://hostonly").is_none(), "无路径段拒绝");
    }

    #[test]
    fn osc7_space_paths_stay_whole() {
        // F：带空格路径不得截断成 /tmp 或 /tmp/oob（#422 点查 Fail 项）。
        let got = parse_osc7("file://host/tmp/oob cwd 419").unwrap();
        assert_eq!(got.path, "/tmp/oob cwd 419");
        let got = parse_osc7("file://host/tmp/oob%20cwd%20419").unwrap();
        assert_eq!(got.path, "/tmp/oob cwd 419");
        let got = parse_osc99("/tmp/oob cwd 419").unwrap();
        assert_eq!(got.path, "/tmp/oob cwd 419");
        let got = parse_osc99("\"/tmp/oob cwd 419\"").unwrap();
        assert_eq!(got.path, "/tmp/oob cwd 419", "引号包裹的空格路径");
    }

    #[test]
    fn cwd_host_is_optional_not_fatal() {
        // 主机段含控制字符：路径照收，主机名丢弃（不因参考信息不合规就丢整条 cwd）。
        let got = parse_osc7("file://bad\u{7}host/home/u").unwrap();
        assert_eq!(got.path, "/home/u");
        assert_eq!(got.host, None);
        // OSC 9;9 裸路径永远没有主机名。
        assert_eq!(parse_osc99("/var/log").unwrap().host, None);
    }

    #[test]
    fn osc7_percent_decoding() {
        let got = parse_osc7("file://host/home/u/My%20Docs").unwrap();
        assert_eq!(got.path, "/home/u/My Docs");
        // 非法百分号序列原样保留。
        let got = parse_osc7("file://host/home/u/100%").unwrap();
        assert_eq!(got.path, "/home/u/100%");
    }

    #[test]
    fn path_with_control_char_rejected() {
        assert!(parse_osc99("/tmp\u{1b}x").is_none());
    }

    #[test]
    fn osc99_variants() {
        let got = parse_osc99("/var/log").unwrap();
        assert_eq!(got.path, "/var/log");
        let got = parse_osc99("\"/tmp/x\"").unwrap();
        assert_eq!(got.path, "/tmp/x", "容忍引号包裹");
        assert!(parse_osc99("relative/path").is_none());
    }

    #[test]
    fn title_rules() {
        assert_eq!(parse_title("user@host: ~/proj").unwrap(), "user@host: ~/proj");
        assert_eq!(parse_title("vim xxx.c").unwrap(), "vim xxx.c");
        assert!(parse_title("").is_none());
        assert!(parse_title("bad\u{7}bell").is_none(), "含控制字符忽略");
        assert!(parse_title(&"x".repeat(600)).is_none(), "超长忽略");
    }
}
