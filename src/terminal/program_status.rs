//! OSC 7501 Program Status Protocol（#390，rev 0.4 / 2026-10-10）。
//!
//! 终端内程序（AI Agent 等）通过 `OSC 7501 ; key=value : key=value … ST`
//! 主动汇报状态。本模块只做**接收侧**：解析一条报告为 [`ProgramStatus`]，
//! 由 `OscSplitter`（command_block.rs）在流里识别 `7501` 前缀后调用。
//!
//! spec 要点（control-codes.page/proposals/osc-7501）：
//! - key=value 用 `:` 分隔；值只含字母数字和 `_.,+/=-`，无需转义
//! - `msg` / `title` 是 base64(UTF-8)，解码后不得含控制字符
//! - 未知 key 忽略（协议扩展方式）；坏 pair 跳过其余照用；
//!   报告超尺寸 / base64 解码失败 / 解出控制字符 → 整条丢弃
//! - `state` 必填；未知 state 整条忽略（新状态不能被旧终端当 idle）

/// 单条状态报告（spec: 一条替换同 id 记录，键缺失即从记录中消失）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramStatus {
    /// 必填：idle / working / blocked / done / error / clear。
    pub state: ProgramState,
    /// 记录 id；`None` = 根记录。路径段以 `/` 分隔（≤8 段 ×32 字符）。
    pub id: Option<String>,
    /// 仅 blocked：permission / question / auth；其他值视为 none。
    pub kind: Option<BlockedKind>,
    /// 仅 working / blocked：0..=100；缺失 = 不定进度。
    pub progress: Option<u8>,
    /// 程序稳定名（cargo / claude …），≤32 字符，用于分组/图标。
    pub app: Option<String>,
    /// 记录短标签（base64 已解码）。
    pub title: Option<String>,
    /// 一行说明（base64 已解码）。
    pub msg: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramState {
    Idle,
    Working,
    Blocked,
    Done,
    Error,
    /// 非状态：删除该 id 记录（无 id 则全部）。
    Clear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockedKind {
    Permission,
    Question,
    Auth,
}

/// 一条报告的解析结果：成功为 [`ProgramStatus`]；格式级损坏（整条丢弃）为 `None`。
/// 未知 key / 坏 pair 的「跳过继续」语义在函数内处理，不体现在返回值上。
pub fn parse_7501_report(payload: &str) -> Option<ProgramStatus> {
    const MAX_REPORT_BYTES: usize = 8_192;
    if payload.len() > MAX_REPORT_BYTES {
        return None;
    }
    let mut state = None;
    let mut id = None;
    let mut kind = None;
    let mut progress = None;
    let mut app = None;
    let mut title = None;
    let mut msg = None;

    for pair in payload.split(':') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let Some((key, value)) = pair.split_once('=') else {
            // 坏 pair：跳过，其余照用。
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "state" => match value {
                "idle" => state = Some(ProgramState::Idle),
                "working" => state = Some(ProgramState::Working),
                "blocked" => state = Some(ProgramState::Blocked),
                "done" => state = Some(ProgramState::Done),
                "error" => state = Some(ProgramState::Error),
                "clear" => state = Some(ProgramState::Clear),
                // 未知 state：整条忽略（spec 明文）。
                _ => return None,
            },
            "id" => {
                // spec：路径超限（>8 段 / 段超 32 字符 / 非法字符）→ 整条丢弃。
                if !valid_id(value) {
                    return None;
                }
                id = Some(value.to_string());
            }
            "kind" => {
                kind = match value {
                    "permission" => Some(BlockedKind::Permission),
                    "question" => Some(BlockedKind::Question),
                    "auth" => Some(BlockedKind::Auth),
                    // 任何其他值视为 none（spec 明文）。
                    _ => None,
                };
            }
            "progress" => {
                // 越界视为 none。
                progress = value.parse::<u8>().ok().filter(|p| *p <= 100);
            }
            "app" => {
                if !value.is_empty()
                    && value.len() <= 32
                    && value
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '+' | '-'))
                {
                    app = Some(value.to_string());
                }
            }
            "title" => {
                // spec：base64 解码失败 / 解出控制字符 → 整条丢弃。
                title = Some(decode_b64_line(value)?);
            }
            "msg" => {
                msg = Some(decode_b64_line(value)?);
            }
            // 未知 key：忽略（协议扩展方式）。
            _ => {}
        }
    }

    let state = state?;
    // spec：kind 「With blocked only」——非 blocked 状态一律视为 none。
    let kind = if state == ProgramState::Blocked {
        kind
    } else {
        None
    };
    // spec：progress 「With working or blocked」——其余状态视为 none。
    let progress = if matches!(state, ProgramState::Working | ProgramState::Blocked) {
        progress
    } else {
        None
    };
    Some(ProgramStatus {
        state,
        id,
        kind,
        progress,
        app,
        title,
        msg,
    })
}

/// id：`/` 分隔路径，≤8 段，每段 ≤32 字符，段内字符受限。
fn valid_id(value: &str) -> bool {
    let segments: Vec<&str> = value.split('/').collect();
    segments.len() <= 8
        && segments
            .iter()
            .all(|seg| {
                !seg.is_empty()
                    && seg.len() <= 32
                    && seg
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '+' | '-'))
            })
}

/// 解码 base64(UTF-8) 单行文本；解码失败 / 含控制字符 → `None`（调用方整条丢弃）。
fn decode_b64_line(value: &str) -> Option<String> {
    use base64::Engine as _;
    // spec：padding optional。STANDARD 引擎要求 `=`；缺 pad 时按 4 对齐补齐。
    let padded = match value.len() % 4 {
        2 => format!("{value}=="),
        3 => format!("{value}="),
        _ => value.to_string(),
    };
    let engine = base64::engine::general_purpose::STANDARD;
    let decoded = engine.decode(padded).ok()?;
    let text = String::from_utf8(decoded).ok()?;
    if text.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_examples_parse() {
        // spec 原文示例：working / blocked / done。
        let working = parse_7501_report("state=working:app=make:msg=QnVpbGRpbmcgZG9jcw==").unwrap();
        assert_eq!(working.state, ProgramState::Working);
        assert_eq!(working.app.as_deref(), Some("make"));
        assert_eq!(working.msg.as_deref(), Some("Building docs"));

        let blocked = parse_7501_report(
            "state=blocked:kind=question:app=make:msg=T3ZlcndyaXRlIGNvbmZpZy50b21sPw==",
        )
        .unwrap();
        assert_eq!(blocked.state, ProgramState::Blocked);
        assert_eq!(blocked.kind, Some(BlockedKind::Question));
        assert_eq!(blocked.msg.as_deref(), Some("Overwrite config.toml?"));

        let done = parse_7501_report("state=done:app=make:msg=RG9jcyBidWlsdA==").unwrap();
        assert_eq!(done.state, ProgramState::Done);
    }

    #[test]
    fn clear_without_id_clears_everything() {
        let got = parse_7501_report("state=clear").unwrap();
        assert_eq!(got.state, ProgramState::Clear);
        assert!(got.id.is_none());
    }

    #[test]
    fn unknown_state_ignores_whole_report() {
        assert!(parse_7501_report("state=napping").is_none());
        assert!(parse_7501_report("").is_none());
        // 只有未知 state 的报告整条丢；混入合法 pair 也一样。
        assert!(parse_7501_report("state=napping:app=x").is_none());
    }

    #[test]
    fn unknown_key_and_bad_pair_are_skipped() {
        let got = parse_7501_report("state=idle:vendor=acme:brokenpair").unwrap();
        assert_eq!(got.state, ProgramState::Idle);
        // 未知 key 不产字段。
        assert!(got.app.is_none());
    }

    #[test]
    fn progress_out_of_range_is_none() {
        let got = parse_7501_report("state=working:progress=150").unwrap();
        assert_eq!(got.progress, None);
        let got = parse_7501_report("state=working:progress=42").unwrap();
        assert_eq!(got.progress, Some(42));
    }

    #[test]
    fn id_path_rules() {
        // 合法：层级路径。
        let got = parse_7501_report("state=done:id=build/test").unwrap();
        assert_eq!(got.id.as_deref(), Some("build/test"));
        // 非法：9 段 → 整条丢弃（spec「超限整条丢弃」语义）。
        assert!(parse_7501_report("state=done:id=a/b/c/d/e/f/g/h/i").is_none());
        // 非法：空段 → 整条丢弃。
        assert!(parse_7501_report("state=done:id=build//test").is_none());
    }

    #[test]
    fn bad_base64_drops_whole_report() {
        // spec：base64 解码失败或解出控制字符 → 整条丢弃（不是只丢字段）。
        assert!(parse_7501_report("state=idle:msg=Cg==:app=x").is_none());
        assert!(parse_7501_report("state=idle:msg=!!!not-b64!!!").is_none());
        assert!(parse_7501_report("state=idle:title=%%%").is_none());
        // 正常 base64 不受影响。
        let got = parse_7501_report("state=idle:msg=SGk=").unwrap();
        assert_eq!(got.msg.as_deref(), Some("Hi"));
    }

    #[test]
    fn kind_only_valid_with_blocked_values() {
        let got = parse_7501_report("state=working:kind=permission").unwrap();
        assert_eq!(got.kind, None, "kind 只在 blocked 有意义，working 下按 spec 为 none");
        let got = parse_7501_report("state=blocked:kind=weird").unwrap();
        assert_eq!(got.kind, None, "未知 kind 视为 none");
    }

    #[test]
    fn base64_without_padding_decodes() {
        // spec：padding optional。"Hi" 无 pad = "SGi"→实际 "SGVsbG8=" 去 pad 是 "SGVsbG8"。
        let got = parse_7501_report("state=idle:msg=SGk").unwrap();
        assert_eq!(got.msg.as_deref(), Some("Hi"));
        let got = parse_7501_report("state=idle:title=SGk").unwrap();
        assert_eq!(got.title.as_deref(), Some("Hi"));
    }

    #[test]
    fn invalid_id_drops_whole_report() {
        // spec：路径超限整条丢弃（不再是丢字段当根记录）。
        assert!(parse_7501_report("state=idle:id=a/b/c/d/e/f/g/h/i").is_none());
        assert!(parse_7501_report("state=idle:id=build//x").is_none());
        // 合法 id 不受影响。
        let got = parse_7501_report("state=idle:id=build/test-1").unwrap();
        assert_eq!(got.id.as_deref(), Some("build/test-1"));
    }

    #[test]
    fn oversized_report_is_dropped_whole() {
        let junk = "a".repeat(10_000);
        assert!(parse_7501_report(&format!("state=idle:app={junk}")).is_none());
    }
}
