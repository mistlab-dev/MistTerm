//! 终端输出可点击文本（display-link）：路径 / 错误码 / IP / URL。
//!
//! 与 `cmd_audit` 结构性解耦：独立 JSON（assets/display-link-patterns.json）、
//! 独立加载与匹配入口；cmd_audit 不读取本模块规则，本模块不参与任何拦截。
//! 规则解析失败或文件缺失 = 功能整体关闭，输出渲染零影响。

use serde::{Deserialize, Serialize};

/// 从输出行中识别出的可点击片段
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DisplayLink {
    /// 在行内的字节起始偏移
    pub start: usize,
    /// 字节结束偏移（不含）
    pub end: usize,
    /// 命中的规则 id（调试/测试用）
    pub rule_id: String,
    /// 点击时分发的动作
    pub action: DisplayLinkAction,
    /// 命中的原文
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DisplayLinkAction {
    /// SFTP 面板跳转到该目录
    SftpOpen,
    /// 用错误码召回团队 Runbook（#66 链路）
    RunbookRecall,
    /// 复制（IP 等通用文本）
    Copy,
    /// 系统浏览器打开
    BrowserOpen,
}

impl DisplayLinkAction {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "sftp-open" => Some(Self::SftpOpen),
            "runbook-recall" => Some(Self::RunbookRecall),
            "copy-or-connect" => Some(Self::Copy),
            "browser-open" => Some(Self::BrowserOpen),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct PatternFile {
    #[serde(default)]
    #[allow(dead_code)]
    version: String,
    patterns: Vec<RawPattern>,
}

#[derive(Debug, Deserialize)]
struct RawPattern {
    id: String,
    pattern: String,
    /// path / errno / ip / url
    #[serde(default)]
    #[allow(dead_code)]
    category: String,
    action: String,
    /// 同一行内多个命中时的优先级：小者先占区间（URL > errno > ip > path）
    #[serde(default)]
    priority: u8,
}

struct CompiledPattern {
    id: String,
    action: DisplayLinkAction,
    priority: u8,
    regex: regex::Regex,
}

/// 规则引擎：加载一次，之后只做匹配。
/// 上限保护：单行最多返回 16 个链接，防止病态输出拖垮渲染。
pub struct DisplayLinkEngine {
    patterns: Vec<CompiledPattern>,
    max_hits_per_line: usize,
}

impl Default for DisplayLinkEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DisplayLinkEngine {
    pub fn new() -> Self {
        const JSON: &str = include_str!("../../assets/display-link-patterns.json");
        Self::from_json(JSON)
    }

    /// 解析失败返回空引擎（功能关闭），与 cmd_audit 的容错策略一致。
    pub fn from_json(json: &str) -> Self {
        let (patterns, max_hits_per_line) = Self::parse(json).unwrap_or_else(|| (Vec::new(), 16));
        Self {
            patterns,
            max_hits_per_line,
        }
    }

    fn parse(json: &str) -> Option<(Vec<CompiledPattern>, usize)> {
        let file: PatternFile = serde_json::from_str(json).ok()?;
        let mut patterns: Vec<CompiledPattern> = file
            .patterns
            .into_iter()
            .filter_map(|p| {
                let action = DisplayLinkAction::from_str(&p.action)?;
                let regex = regex::Regex::new(&p.pattern).ok()?;
                Some(CompiledPattern {
                    id: p.id,
                    action,
                    priority: p.priority,
                    regex,
                })
            })
            .collect();
        patterns.sort_by_key(|p| p.priority);
        Some((patterns, 16))
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// SFTP 列表目标目录：去掉末尾斜杠；若末段像文件名（含 `.`）则缩到父目录。
    /// 例：`/var/log/nginx/error.log` → `/var/log/nginx`；`/var/log` → `/var/log`。
    pub fn sftp_list_dir(path: &str) -> String {
        let trimmed = path
            .trim()
            .trim_end_matches(['/', '\\']);
        if trimmed.is_empty() || trimmed == "." {
            return "/".into();
        }
        let p = std::path::Path::new(trimmed);
        let file_like = p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.contains('.') && n != "." && n != "..");
        let dir = if file_like {
            match p.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => parent,
                _ => std::path::Path::new("/"),
            }
        } else {
            p
        };
        let mut out = dir.to_string_lossy().replace('\\', "/");
        if out.is_empty() {
            out = "/".into();
        }
        out
    }

    /// 对一行输出做匹配，返回互不重叠、按出现位置排序的链接。
    ///
    /// 区间裁决：同一区间被多条规则命中时，取 priority 最小（最优先）的；
    /// 已占用区间不再接受新链接（先到先得，按 priority 顺序扫描）。
    pub fn scan(&self, line: &str) -> Vec<DisplayLink> {
        if self.patterns.is_empty() || line.is_empty() || line.len() > 8192 {
            return Vec::new();
        }
        // 占用标记：按字节粒度记录已被更高优先级链接占用的区间。
        let mut occupied: Vec<(usize, usize)> = Vec::new();
        let mut hits: Vec<DisplayLink> = Vec::new();
        for p in &self.patterns {
            for m in p.regex.captures_iter(line) {
                // 取第一个非空捕获组（规则里的主捕获），无组则取整体匹配
                let (start, end, text) = match m.get(1).or_else(|| m.get(0)) {
                    Some(mm) => (mm.start(), mm.end(), mm.as_str().to_string()),
                    None => continue,
                };
                if end <= start {
                    continue;
                }
                if occupied.iter().any(|(s, e)| start < *e && end > *s) {
                    continue; // 与高优先级链接重叠，让位
                }
                occupied.push((start, end));
                hits.push(DisplayLink {
                    start,
                    end,
                    rule_id: p.id.clone(),
                    action: p.action,
                    text,
                });
                if hits.len() >= self.max_hits_per_line {
                    hits.sort_by_key(|h| h.start);
                    return hits;
                }
            }
        }
        hits.sort_by_key(|h| h.start);
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> DisplayLinkEngine {
        DisplayLinkEngine::new()
    }

    #[test]
    fn builtin_json_parses() {
        let e = engine();
        assert!(!e.is_empty(), "builtin rules must load");
        assert!(e.patterns.len() >= 4);
    }

    #[test]
    fn errno_hit() {
        let e = engine();
        let hits = e.scan("curl: (7) Failed to connect: ECONNREFUSED 127.0.0.1:6379");
        let errno = hits.iter().find(|h| h.rule_id == "DL-ERR-001").expect("errno hit");
        assert_eq!(errno.text, "ECONNREFUSED");
        assert_eq!(errno.action, DisplayLinkAction::RunbookRecall);
    }

    #[test]
    fn ipv4_hit_and_version_not_mismatched() {
        let e = engine();
        let hits = e.scan("connect to 192.168.1.10 ok; version 1.2.3 shipped");
        let ip = hits.iter().find(|h| h.rule_id == "DL-IPV4-001").expect("ip hit");
        assert_eq!(ip.text, "192.168.1.10");
        // 版本号 1.2.3 不是四段全匹配，不应命中
        assert!(hits.iter().all(|h| h.text != "1.2.3"));
    }

    #[test]
    fn url_hit() {
        let e = engine();
        let hits = e.scan("see https://docs.example.com/guide for details");
        assert!(hits.iter().any(|h| h.action == DisplayLinkAction::BrowserOpen
            && h.text.starts_with("https://docs.example.com/guide")));
    }

    #[test]
    fn path_hit() {
        let e = engine();
        let hits = e.scan("failed to open /var/log/nginx/error.log");
        let p = hits.iter().find(|h| h.action == DisplayLinkAction::SftpOpen);
        assert!(p.is_some(), "path hit expected, got {:?}", hits);
    }

    #[test]
    fn sftp_list_dir_shrinks_file_to_parent() {
        assert_eq!(
            DisplayLinkEngine::sftp_list_dir("/var/log/nginx/error.log"),
            "/var/log/nginx"
        );
        assert_eq!(
            DisplayLinkEngine::sftp_list_dir("/var/log/nginx/"),
            "/var/log/nginx"
        );
        assert_eq!(DisplayLinkEngine::sftp_list_dir("/var/log"), "/var/log");
        assert_eq!(DisplayLinkEngine::sftp_list_dir("/"), "/");
        assert_eq!(DisplayLinkEngine::sftp_list_dir(""), "/");
    }

    #[test]
    fn overlaps_priority_url_beats_ip() {
        let e = engine();
        // URL 里包含 IP 段：URL（priority 高，先扫）应胜出
        let hits = e.scan("at http://10.0.0.7:8080/health");
        let url = hits.iter().find(|h| h.action == DisplayLinkAction::BrowserOpen);
        assert!(url.is_some(), "url should win overlap");
        let ip_inside = hits.iter().find(|h| h.action == DisplayLinkAction::Copy
            && h.text == "10.0.0.7");
        assert!(ip_inside.is_none(), "overlapped IP must yield to URL");
    }

    #[test]
    fn max_hits_cap() {
        let e = engine();
        let line = "a EACCES b EACCES c EACCES d EACCES e EACCES f EACCES g EACCES h EACCES
        i EACCES j EACCES k EACCES l EACCES m EACCES n EACCES o EACCES p EACCES q EACCES r EACCES s EACCES t EACCES";
        let hits = e.scan(line);
        assert!(hits.len() <= 16, "cap at 16, got {}", hits.len());
    }

    #[test]
    fn long_line_short_circuit() {
        let e = engine();
        let line = "x".repeat(9000);
        assert!(e.scan(&line).is_empty());
    }

    #[test]
    fn empty_engine_no_hits() {
        let e = DisplayLinkEngine::from_json("{}");
        assert!(e.is_empty());
        assert!(e.scan("EACCES https://example.com").is_empty());
    }
}
