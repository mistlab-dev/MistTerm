//! 终端输出 Trigger：正则命中 → 通知（后台标签注意点 / Toast）。
//!
//! 与 `cmd_audit`、`display_link` 结构性解耦：独立 JSON
//! （`assets/output-trigger-patterns.json`）、独立加载与匹配入口。
//! 解析失败或文件缺失 = 功能关闭，输出渲染零影响。
//!
//! 范围（#89 P2 / 需求 7）：异步等待场景「完成/出错主动告诉我」。
//! 不做：用户自定义 UI、插件市场、高亮整行改色（通知优先）。

use serde::Deserialize;

/// 命中严重级别（决定 Toast 样式）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerSeverity {
    Error,
    Warn,
    Success,
    Info,
}

impl TriggerSeverity {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            "error" => Some(Self::Error),
            "warn" | "warning" => Some(Self::Warn),
            "success" | "ok" => Some(Self::Success),
            "info" => Some(Self::Info),
            _ => None,
        }
    }
}

/// 单行匹配结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputTriggerHit {
    pub rule_id: String,
    pub severity: TriggerSeverity,
    /// 命中原文（截断，供 Toast 正文）
    pub text: String,
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
    severity: String,
    #[serde(default)]
    #[allow(dead_code)]
    action: String,
    #[serde(default)]
    priority: u8,
}

struct CompiledPattern {
    id: String,
    severity: TriggerSeverity,
    priority: u8,
    regex: regex::Regex,
}

/// 规则引擎：加载一次，之后只做匹配。
/// 单行最多 1 条（取 priority 最小），避免一行刷多条 Toast。
pub struct OutputTriggerEngine {
    patterns: Vec<CompiledPattern>,
}

impl Default for OutputTriggerEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputTriggerEngine {
    pub fn new() -> Self {
        const JSON: &str = include_str!("../../assets/output-trigger-patterns.json");
        Self::from_json(JSON)
    }

    /// 解析失败返回空引擎（功能关闭）。
    pub fn from_json(json: &str) -> Self {
        let patterns = Self::parse(json).unwrap_or_default();
        Self { patterns }
    }

    fn parse(json: &str) -> Option<Vec<CompiledPattern>> {
        let file: PatternFile = serde_json::from_str(json).ok()?;
        let mut patterns: Vec<CompiledPattern> = file
            .patterns
            .into_iter()
            .filter_map(|p| {
                let severity = TriggerSeverity::from_str(&p.severity)?;
                let regex = regex::Regex::new(&p.pattern).ok()?;
                Some(CompiledPattern {
                    id: p.id,
                    severity,
                    priority: p.priority,
                    regex,
                })
            })
            .collect();
        patterns.sort_by_key(|p| p.priority);
        Some(patterns)
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// 对一行输出做匹配；无命中返回 `None`。
    /// 行长超过 4KiB 直接跳过（高速垃圾输出保护）。
    pub fn scan_line(&self, line: &str) -> Option<OutputTriggerHit> {
        if self.patterns.is_empty() || line.is_empty() || line.len() > 4096 {
            return None;
        }
        for p in &self.patterns {
            if let Some(m) = p.regex.find(line) {
                let raw = m.as_str();
                let text = if raw.chars().count() > 80 {
                    let truncated: String = raw.chars().take(77).collect();
                    format!("{truncated}…")
                } else {
                    raw.to_string()
                };
                return Some(OutputTriggerHit {
                    rule_id: p.id.clone(),
                    severity: p.severity,
                    text,
                });
            }
        }
        None
    }
}

/// 把字节流拆成完整行再扫描；残余半行放回 `acc`。
pub fn feed_and_scan(
    engine: &OutputTriggerEngine,
    acc: &mut String,
    chunk: &[u8],
    out: &mut Vec<OutputTriggerHit>,
) {
    if engine.is_empty() || chunk.is_empty() {
        return;
    }
    let text = String::from_utf8_lossy(chunk);
    acc.push_str(&text);
    while let Some(idx) = acc.find('\n') {
        let mut line = acc[..idx].to_string();
        *acc = acc[idx + 1..].to_string();
        if line.ends_with('\r') {
            line.pop();
        }
        if let Some(hit) = engine.scan_line(&line) {
            out.push(hit);
        }
    }
    // 防止半行无限膨胀（无换行的二进制/进度条）
    const ACC_CAP: usize = 2048;
    if acc.len() > ACC_CAP {
        acc.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> OutputTriggerEngine {
        OutputTriggerEngine::new()
    }

    #[test]
    fn builtin_json_parses() {
        let e = engine();
        assert!(!e.is_empty());
    }

    #[test]
    fn errno_error_hit() {
        let e = engine();
        let hit = e
            .scan_line("curl: Failed to connect: ECONNREFUSED 127.0.0.1")
            .expect("error hit");
        assert_eq!(hit.rule_id, "TR-ERR-001");
        assert_eq!(hit.severity, TriggerSeverity::Error);
        assert!(hit.text.contains("ECONNREFUSED"));
    }

    #[test]
    fn success_hit() {
        let e = engine();
        let hit = e.scan_line("Build succeeded in 12.3s").expect("ok hit");
        assert_eq!(hit.severity, TriggerSeverity::Success);
    }

    #[test]
    fn warn_hit() {
        let e = engine();
        let hit = e.scan_line("WARNING: disk almost full").expect("warn");
        assert_eq!(hit.severity, TriggerSeverity::Warn);
    }

    #[test]
    fn no_hit_on_plain() {
        let e = engine();
        assert!(e.scan_line("hello world").is_none());
    }

    #[test]
    fn feed_splits_lines() {
        let e = engine();
        let mut acc = String::new();
        let mut out = Vec::new();
        feed_and_scan(&e, &mut acc, b"noise\nECONNREFUSED here\npartial", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(acc, "partial");
    }

    #[test]
    fn empty_engine_noop() {
        let e = OutputTriggerEngine::from_json("{}");
        assert!(e.is_empty());
        assert!(e.scan_line("ECONNREFUSED").is_none());
    }

    #[test]
    fn long_line_skipped() {
        let e = engine();
        let line = format!("{}ECONNREFUSED", "x".repeat(5000));
        assert!(e.scan_line(&line).is_none());
    }
}
