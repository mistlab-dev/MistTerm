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


// --- Capability Module Integration (#119) ---

pub struct OutputTriggerModule {
    engine: OutputTriggerEngine,
}

impl Default for OutputTriggerModule {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputTriggerModule {
    pub fn new() -> Self {
        Self {
            engine: OutputTriggerEngine::new(),
        }
    }

    pub fn from_json(json: &str) -> Self {
        Self {
            engine: OutputTriggerEngine::from_json(json),
        }
    }
}

impl crate::capability::MistModule for OutputTriggerModule {
    fn id(&self) -> crate::capability::ModuleId {
        "output-trigger"
    }

    fn name_zh(&self) -> &str {
        "输出触发器"
    }

    fn name_en(&self) -> &str {
        "Output Trigger"
    }

    fn enabled_by_default(&self) -> bool {
        true
    }

    fn on_output_line(&self, ctx: &crate::capability::OutputLineCtx<'_>) -> Vec<crate::capability::ModuleAction> {
        if let Some(hit) = self.engine.scan_line(ctx.line) {
            let sev = match hit.severity {
                TriggerSeverity::Error => crate::capability::Severity::Error,
                TriggerSeverity::Warn => crate::capability::Severity::Warn,
                TriggerSeverity::Success => crate::capability::Severity::Success,
                TriggerSeverity::Info => crate::capability::Severity::Info,
            };
            vec![crate::capability::ModuleAction::Notify {
                severity: sev,
                text: hit.text,
                attention: true,
            }]
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn module_on_output_line_matches_and_disables() {
        use crate::capability::MistModule;
        let module = OutputTriggerModule::new();
        assert_eq!(module.id(), "output-trigger");

        let ctx = crate::capability::OutputLineCtx {
            line: "curl: Failed to connect: ECONNREFUSED 127.0.0.1",
        };
        let actions = module.on_output_line(&ctx);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            crate::capability::ModuleAction::Notify { severity, text, attention } => {
                assert_eq!(*severity, crate::capability::Severity::Error);
                assert!(*attention);
                assert!(text.contains("ECONNREFUSED"));
            }
            _ => panic!("expected notify action"),
        }

        // 经 Registry 禁用后无动作
        let reg = crate::capability::ModuleRegistry::with_modules(vec![Box::new(module)]);
        assert_eq!(reg.on_output_line(&ctx).len(), 1);

        reg.set_enabled("output-trigger", false);
        assert!(reg.on_output_line(&ctx).is_empty());
    }

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
