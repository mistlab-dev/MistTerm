//! 终端输出 Trigger：正则命中 → `ModuleAction::Notify`（后台标签注意点 / Toast）。
//!
//! 规则仍用 `assets/output-trigger-patterns.json`。模块 id：`output-trigger`。
//! 解析失败或文件缺失 = 引擎为空，`on_output_line` 不产出动作，输出渲染零影响。
//!
//! 范围（#89 P2 / #112，经 #119 迁入 MistModule）：异步等待场景「完成/出错主动告诉我」。
//! 不做：用户自定义 UI、插件市场、高亮整行改色（通知优先）。
//! 关闭模块后不再收到行事件（registry 不调用本模块）；不擦 scrollback / 历史 Toast。

use crate::capability::{ModuleAction, ModuleId, OutputLineCtx, Severity};

use serde::Deserialize;

/// 模块清单里的稳定 id。设置项与 `MISTTERM_DISABLE_MODULES` 都用它。
pub const MODULE_ID: ModuleId = "output-trigger";

/// 命中严重级别（决定 Toast 样式）。与 #112 的四档一致。
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

    fn to_module_severity(self) -> Severity {
        match self {
            Self::Error => Severity::Error,
            Self::Warn => Severity::Warn,
            Self::Success => Severity::Success,
            Self::Info => Severity::Info,
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

/// output-trigger 内置模块。行缓冲在 Host；这里只对完整行做匹配。
pub struct OutputTriggerModule {
    engine: OutputTriggerEngine,
}

impl OutputTriggerModule {
    pub fn new() -> Self {
        Self {
            engine: OutputTriggerEngine::new(),
        }
    }
}

impl Default for OutputTriggerModule {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::capability::MistModule for OutputTriggerModule {
    fn id(&self) -> ModuleId {
        MODULE_ID
    }

    fn name_zh(&self) -> &str {
        "输出触发"
    }

    fn name_en(&self) -> &str {
        "Output Trigger"
    }

    fn on_output_line(&self, ctx: &OutputLineCtx<'_>) -> Vec<ModuleAction> {
        let Some(hit) = self.engine.scan_line(ctx.line) else {
            return Vec::new();
        };
        vec![ModuleAction::Notify {
            severity: hit.severity.to_module_severity(),
            text: hit.text,
            attention: true,
            dedupe_key: hit.rule_id,
        }]
    }
}

/// 把字节流拆成完整行再扫描；残余半行放回 `acc`。
/// 引擎为空时不消费字节（与 #112 一致）。Host 生产路径走 registry，不调用本函数。
#[cfg(test)]
fn feed_and_scan(
    engine: &OutputTriggerEngine,
    acc: &mut String,
    chunk: &[u8],
    out: &mut Vec<OutputTriggerHit>,
) {
    if engine.is_empty() || chunk.is_empty() {
        return;
    }
    for line in crate::capability::split_pty_output_lines(acc, chunk) {
        if let Some(hit) = engine.scan_line(&line) {
            out.push(hit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::{MistModule, ModuleRegistry};

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

    #[test]
    fn lower_priority_number_wins_when_two_rules_match() {
        let e = engine();
        let hit = e
            .scan_line("ECONNREFUSED build succeeded")
            .expect("error outranks success");
        assert_eq!(hit.rule_id, "TR-ERR-001");
        assert_eq!(hit.severity, TriggerSeverity::Error);
    }

    #[test]
    fn module_notify_matches_engine_hit() {
        let engine = engine();
        let module = OutputTriggerModule::new();
        let samples = [
            "curl: Failed to connect: ECONNREFUSED 127.0.0.1",
            "Build succeeded in 12.3s",
            "WARNING: disk almost full",
            "hello world",
        ];
        for line in samples {
            let actions = module.on_output_line(&OutputLineCtx { line });
            match engine.scan_line(line) {
                None => assert!(actions.is_empty(), "plain line must not notify"),
                Some(hit) => {
                    assert_eq!(actions.len(), 1);
                    match &actions[0] {
                        ModuleAction::Notify {
                            severity,
                            text,
                            attention,
                            dedupe_key,
                        } => {
                            assert_eq!(dedupe_key, &hit.rule_id);
                            assert_eq!(text, &hit.text);
                            assert!(*attention);
                            assert_eq!(*severity, hit.severity.to_module_severity());
                        }
                        other => panic!("expected notify, got {other:?}"),
                    }
                }
            }
        }
    }

    #[test]
    fn disabled_via_settings_emits_nothing_until_reenabled() {
        let reg = ModuleRegistry::with_modules(vec![Box::new(OutputTriggerModule::new())]);
        let ctx = OutputLineCtx {
            line: "ECONNREFUSED",
        };
        assert!(reg.is_enabled(MODULE_ID));
        assert_eq!(reg.on_output_line(&ctx).len(), 1);

        // UI 关闭：disabled_modules 含本 id。关 = 之后不再收行事件。
        reg.sync_effective_disabled(&[MODULE_ID.to_string()]);
        assert!(!reg.is_enabled(MODULE_ID));
        assert!(reg.on_output_line(&ctx).is_empty());

        // 从设置里拿掉后恢复（默认开启）。
        reg.sync_effective_disabled(&[]);
        assert!(reg.is_enabled(MODULE_ID));
        assert_eq!(reg.on_output_line(&ctx).len(), 1);
    }
}
