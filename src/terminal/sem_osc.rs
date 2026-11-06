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

/// 当前目录汇报：OSC 7（`file://host/path`）与 OSC 9;9（裸路径）统一产物。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CwdReport {
    pub path: String,
}

/// OSC 7 载荷（如 `file://myhost/home/u` 或 `file:///home/u`）→ 路径。
/// hostname 与本端/远端未必一致，一律以路径为准（spec 宽容处理）。
pub fn parse_osc7(payload: &str) -> Option<CwdReport> {
    let rest = payload.strip_prefix("file://")?;
    // file://host/path → 去掉 host 段；file:///path → host 为空。
    let path = match rest.split_once('/') {
        Some((_host, path)) => format!("/{path}"),
        None => return None,
    };
    sanitize(path)
}

/// OSC 9;9 载荷是裸路径（可能有引号包裹）。
pub fn parse_osc99(payload: &str) -> Option<CwdReport> {
    let payload = payload
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(payload);
    sanitize(payload.to_string())
}

fn sanitize(path: String) -> Option<CwdReport> {
    if !path.starts_with('/') || path.contains('\0') {
        return None;
    }
    Some(CwdReport { path })
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
        let got = parse_osc7("file:///home/u").unwrap();
        assert_eq!(got.path, "/home/u");
        assert!(parse_osc7("not-a-file-url").is_none());
        assert!(parse_osc7("file://hostonly").is_none(), "无路径段拒绝");
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
