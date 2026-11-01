//! 终端输出 Trigger：正则命中 → `ModuleAction::Notify`（后台标签注意点 / Toast）。
//!
//! 未自定义时规则来自 `assets/output-trigger-patterns.json`（编译进二进制）。
//! 用户在「工具 → 自动化规则」里改过之后，写到配置目录
//! `mistterm/output-trigger-rules.json`。保存后的下一行输出立刻用新规则。
//! 非法正则留在列表里供修改，但不进入匹配；`scan_line` 不 panic，输出路径不受影响。
//!
//! 模块开关（偏好 `disabled_modules` / `MISTTERM_DISABLE_MODULES`）只决定
//! registry 是否调用本模块。关掉之后仍可编辑规则，运行时不触发。
//!
//! 不做：插件市场、高亮整行改色（通知优先）、断线态提示（#184）。

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::capability::{LineOrigin, ModuleAction, ModuleId, OutputLineCtx, Severity};

/// 模块清单里的稳定 id。设置项与 `MISTTERM_DISABLE_MODULES` 都用它。
pub const MODULE_ID: ModuleId = "output-trigger";

pub const MAX_RULES: usize = 64;
pub const MAX_PATTERN_BYTES: usize = 1024;
const MAX_IMPORT_BYTES: usize = 256 * 1024;
const MAX_ID_CHARS: usize = 64;

const BUILTIN_JSON: &str = include_str!("../../assets/output-trigger-patterns.json");

/// 命中严重级别（决定 Toast 样式）。与 #112 的四档一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerSeverity {
    Error,
    Warn,
    Success,
    Info,
}

impl TriggerSeverity {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "error" => Some(Self::Error),
            "warn" | "warning" => Some(Self::Warn),
            "success" | "ok" => Some(Self::Success),
            "info" => Some(Self::Info),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Success => "success",
            Self::Info => "info",
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

/// 命中后的动作。目前只有通知；字段仍落盘，方便导入导出和以后加动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerAction {
    Notify,
}

impl TriggerAction {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "notify" | "toast" => Some(Self::Notify),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        "notify"
    }
}

/// 一条规则。`pattern_error` 不落盘，加载时按正则重算。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputTriggerRule {
    pub id: String,
    pub pattern: String,
    pub severity: TriggerSeverity,
    pub action: TriggerAction,
    pub enabled: bool,
    pub priority: u8,
    /// 正则编译失败时的说明。这种规则保留在列表里，但不会参与匹配。
    pub pattern_error: Option<String>,
}

/// 编辑器提交的草稿。`id == None` 表示新建。
#[derive(Debug, Clone)]
pub struct RuleDraft {
    pub id: Option<String>,
    pub pattern: String,
    pub severity: TriggerSeverity,
    pub action: TriggerAction,
    pub enabled: bool,
    pub priority: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleError {
    EmptyPattern,
    PatternTooLong,
    InvalidRegex(String),
    NotFound,
    TooMany,
    EmptyId,
    IdTooLong,
    BadSeverity,
    BadAction,
    EmptyFile,
    InvalidFile(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub imported: usize,
    /// 已写入、但正则无效所以不会命中的规则 id。
    pub invalid_pattern_ids: Vec<String>,
    /// 没写进去的规则（级别/动作无法识别、重复 id、超出条数）。
    pub skipped: Vec<String>,
}

/// 单行匹配结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputTriggerHit {
    pub rule_id: String,
    pub severity: TriggerSeverity,
    /// 命中原文（截断，供 Toast 正文）
    pub text: String,
}

/// 编辑前检查正则。空模式、过长、编译失败都不会进入匹配表。
pub fn check_pattern(pattern: &str) -> Result<(), RuleError> {
    if pattern.trim().is_empty() {
        return Err(RuleError::EmptyPattern);
    }
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(RuleError::PatternTooLong);
    }
    regex::Regex::new(pattern)
        .map(|_| ())
        .map_err(|e| RuleError::InvalidRegex(e.to_string()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuleDocument {
    #[serde(default)]
    version: String,
    #[serde(default)]
    customized: bool,
    #[serde(default)]
    rules: Vec<RuleDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    patterns: Vec<RuleDto>,
}

impl Default for RuleDocument {
    fn default() -> Self {
        Self {
            version: String::new(),
            customized: false,
            rules: Vec::new(),
            patterns: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuleDto {
    id: String,
    pattern: String,
    severity: String,
    #[serde(default = "default_notify_action")]
    action: String,
    #[serde(default = "default_true")]
    enabled: bool,
    /// 缺省 0，与旧版 assets 解析一致（数字越小越先匹配）。
    #[serde(default)]
    priority: u8,
}

fn default_notify_action() -> String {
    "notify".into()
}

fn default_true() -> bool {
    true
}

fn rule_from_dto(dto: RuleDto) -> Result<OutputTriggerRule, (String, RuleError)> {
    let id = dto.id.trim().to_string();
    if id.is_empty() {
        return Err((id, RuleError::EmptyId));
    }
    if id.chars().count() > MAX_ID_CHARS {
        return Err((id, RuleError::IdTooLong));
    }
    let severity = TriggerSeverity::parse(&dto.severity)
        .ok_or_else(|| (id.clone(), RuleError::BadSeverity))?;
    let action =
        TriggerAction::parse(&dto.action).ok_or_else(|| (id.clone(), RuleError::BadAction))?;
    let pattern_error = match check_pattern(&dto.pattern) {
        Ok(()) => None,
        Err(RuleError::InvalidRegex(msg)) => Some(msg),
        Err(RuleError::PatternTooLong) => Some("pattern too long".to_string()),
        Err(RuleError::EmptyPattern) => Some("empty pattern".to_string()),
        Err(_) => Some("invalid pattern".to_string()),
    };
    Ok(OutputTriggerRule {
        id,
        pattern: dto.pattern,
        severity,
        action,
        enabled: dto.enabled,
        priority: dto.priority,
        pattern_error,
    })
}

fn rule_to_dto(rule: &OutputTriggerRule) -> RuleDto {
    RuleDto {
        id: rule.id.clone(),
        pattern: rule.pattern.clone(),
        severity: rule.severity.as_str().to_string(),
        action: rule.action.as_str().to_string(),
        enabled: rule.enabled,
        priority: rule.priority,
    }
}

fn parse_rules_lenient(json: &str) -> Vec<OutputTriggerRule> {
    let doc: RuleDocument = match serde_json::from_str(json) {
        Ok(doc) => doc,
        Err(_) => return Vec::new(),
    };
    document_rules(doc)
}

fn document_rules(doc: RuleDocument) -> Vec<OutputTriggerRule> {
    let dtos = if !doc.rules.is_empty() {
        doc.rules
    } else {
        doc.patterns
    };
    dtos.into_iter()
        .filter_map(|dto| rule_from_dto(dto).ok())
        .collect()
}

/// 编译进二进制的内置规则。用户还没自定义时，引擎用这一份。
pub fn builtin_rules() -> Vec<OutputTriggerRule> {
    parse_rules_lenient(BUILTIN_JSON)
}

struct CompiledPattern {
    id: String,
    severity: TriggerSeverity,
    priority: u8,
    regex: regex::Regex,
}

/// 规则引擎：只含已启用且正则合法的规则。
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
        Self::from_rules(&builtin_rules())
    }

    /// 解析失败返回空引擎（功能关闭，不拖垮调用方）。
    pub fn from_json(json: &str) -> Self {
        Self::from_rules(&parse_rules_lenient(json))
    }

    pub fn from_rules(rules: &[OutputTriggerRule]) -> Self {
        let mut patterns = Vec::new();
        for rule in rules {
            if !rule.enabled || rule.pattern_error.is_some() {
                continue;
            }
            if rule.action != TriggerAction::Notify {
                continue;
            }
            let Ok(regex) = regex::Regex::new(&rule.pattern) else {
                continue;
            };
            patterns.push(CompiledPattern {
                id: rule.id.clone(),
                severity: rule.severity,
                priority: rule.priority,
                regex,
            });
        }
        patterns.sort_by_key(|p| p.priority);
        Self { patterns }
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

struct Inner {
    path: PathBuf,
    /// `false` 时编辑不落盘（测试用的内置快照，避免写到开发者的配置目录）。
    persist: bool,
    customized: bool,
    rules: Vec<OutputTriggerRule>,
    engine: OutputTriggerEngine,
    save_error: Option<String>,
}

/// 用户规则与编译后的引擎。UI 和 `OutputTriggerModule` 各持一份 `Clone`（内部是 `Arc`）。
#[derive(Clone)]
pub struct OutputTriggerStore {
    inner: Arc<RwLock<Inner>>,
}

impl OutputTriggerStore {
    /// 配置目录下的用户规则。文件不存在 = 还没自定义 = 内置规则，且不创建文件。
    pub fn load() -> Self {
        Self::load_from(&Self::default_path())
    }

    pub fn load_from(path: &Path) -> Self {
        if !path.exists() {
            return Self::from_parts(path, builtin_rules(), false, true);
        }
        let doc: RuleDocument = crate::security::encrypted_file::load_encrypted_json(path);
        let customized_flag = doc.customized;
        let has_rules = !doc.rules.is_empty() || !doc.patterns.is_empty();
        if !customized_flag && !has_rules {
            log::warn!(
                "output-trigger rules file {} is empty or unreadable; using built-in rules",
                path.display()
            );
            return Self::from_parts(path, builtin_rules(), false, true);
        }
        let rules = document_rules(doc);
        Self::from_parts(path, rules, true, true)
    }

    /// 只含内置规则，不读也不写配置目录。`builtin_modules()` 与单测走这里。
    pub fn builtin_ephemeral() -> Self {
        Self::from_parts(&Self::default_path(), builtin_rules(), false, false)
    }

    pub fn default_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("output-trigger-rules.json");
        p
    }

    fn from_parts(
        path: &Path,
        rules: Vec<OutputTriggerRule>,
        customized: bool,
        persist: bool,
    ) -> Self {
        let engine = OutputTriggerEngine::from_rules(&rules);
        Self {
            inner: Arc::new(RwLock::new(Inner {
                path: path.to_path_buf(),
                persist,
                customized,
                rules,
                engine,
                save_error: None,
            })),
        }
    }

    fn write<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let mut guard = self.inner.write().unwrap_or_else(|err| err.into_inner());
        f(&mut guard)
    }

    fn read<R>(&self, f: impl FnOnce(&Inner) -> R) -> R {
        let guard = self.inner.read().unwrap_or_else(|err| err.into_inner());
        f(&guard)
    }

    pub fn customized(&self) -> bool {
        self.read(|inner| inner.customized)
    }

    pub fn rules(&self) -> Vec<OutputTriggerRule> {
        self.read(|inner| inner.rules.clone())
    }

    pub fn last_save_error(&self) -> Option<String> {
        self.read(|inner| inner.save_error.clone())
    }

    pub fn scan_line(&self, line: &str) -> Option<OutputTriggerHit> {
        self.read(|inner| inner.engine.scan_line(line))
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), RuleError> {
        self.write(|inner| {
            let Some(rule) = inner.rules.iter_mut().find(|rule| rule.id == id) else {
                return Err(RuleError::NotFound);
            };
            rule.enabled = enabled;
            inner.customized = true;
            recompile(inner);
            remember_save(inner);
            Ok(())
        })
    }

    pub fn upsert(&self, draft: RuleDraft) -> Result<OutputTriggerRule, RuleError> {
        check_pattern(&draft.pattern)?;
        self.write(|inner| {
            let id = if let Some(id) = draft.id.as_deref() {
                if !inner.rules.iter().any(|rule| rule.id == id) {
                    return Err(RuleError::NotFound);
                }
                id.to_string()
            } else {
                if inner.rules.len() >= MAX_RULES {
                    return Err(RuleError::TooMany);
                }
                fresh_id(&inner.rules)
            };
            let rule = OutputTriggerRule {
                id,
                pattern: draft.pattern,
                severity: draft.severity,
                action: draft.action,
                enabled: draft.enabled,
                priority: draft.priority,
                pattern_error: None,
            };
            if let Some(slot) = inner
                .rules
                .iter_mut()
                .find(|existing| existing.id == rule.id)
            {
                *slot = rule.clone();
            } else {
                inner.rules.push(rule.clone());
            }
            inner.customized = true;
            recompile(inner);
            remember_save(inner);
            Ok(rule)
        })
    }

    pub fn delete(&self, id: &str) -> bool {
        self.write(|inner| {
            let before = inner.rules.len();
            inner.rules.retain(|rule| rule.id != id);
            if inner.rules.len() == before {
                return false;
            }
            inner.customized = true;
            recompile(inner);
            remember_save(inner);
            true
        })
    }

    /// 删掉用户文件，内存回到当前二进制里的内置规则。
    pub fn reset_to_builtin(&self) -> Result<(), String> {
        self.write(|inner| {
            if inner.persist && inner.path.exists() {
                std::fs::remove_file(&inner.path).map_err(|err| err.to_string())?;
            }
            inner.rules = builtin_rules();
            inner.customized = false;
            inner.save_error = None;
            recompile(inner);
            Ok(())
        })
    }

    pub fn export_json(&self) -> String {
        self.read(|inner| {
            let doc = RuleDocument {
                version: "1".to_string(),
                customized: true,
                rules: inner.rules.iter().map(rule_to_dto).collect(),
                patterns: Vec::new(),
            };
            serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string())
        })
    }

    /// 用 JSON 整表替换当前规则（支持本模块导出的 `rules`，也支持内置 assets 的 `patterns`）。
    /// 正则无效的条目会留下但不会命中。整份 JSON 无法解析时不改现有规则。
    pub fn import_json(&self, json: &str) -> Result<ImportReport, RuleError> {
        if json.len() > MAX_IMPORT_BYTES {
            return Err(RuleError::InvalidFile("file is too large".into()));
        }
        let doc: RuleDocument =
            serde_json::from_str(json).map_err(|err| RuleError::InvalidFile(err.to_string()))?;
        let dtos = if !doc.rules.is_empty() {
            doc.rules
        } else {
            doc.patterns
        };
        if dtos.is_empty() {
            return Err(RuleError::EmptyFile);
        }
        let mut rules = Vec::new();
        let mut invalid_pattern_ids = Vec::new();
        let mut skipped = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for dto in dtos {
            let label = dto.id.trim().to_string();
            match rule_from_dto(dto) {
                Ok(rule) => {
                    if !seen.insert(rule.id.clone()) {
                        skipped.push(format!("{}: duplicate id", rule.id));
                        continue;
                    }
                    if rules.len() >= MAX_RULES {
                        skipped.push(format!("{}: too many rules", rule.id));
                        continue;
                    }
                    if rule.pattern_error.is_some() {
                        invalid_pattern_ids.push(rule.id.clone());
                    }
                    rules.push(rule);
                }
                Err((id, err)) => {
                    let id = if id.is_empty() { label } else { id };
                    skipped.push(format!("{id}: {err:?}"));
                }
            }
        }
        if rules.is_empty() {
            return Err(RuleError::InvalidFile("no rules could be imported".into()));
        }
        self.write(|inner| {
            inner.rules = rules;
            inner.customized = true;
            recompile(inner);
            remember_save(inner);
        });
        Ok(ImportReport {
            imported: self.rules().len(),
            invalid_pattern_ids,
            skipped,
        })
    }
}

fn recompile(inner: &mut Inner) {
    inner.engine = OutputTriggerEngine::from_rules(&inner.rules);
}

fn remember_save(inner: &mut Inner) {
    if !inner.persist {
        inner.save_error = None;
        return;
    }
    let doc = RuleDocument {
        version: "1".to_string(),
        customized: true,
        rules: inner.rules.iter().map(rule_to_dto).collect(),
        patterns: Vec::new(),
    };
    match crate::security::encrypted_file::save_encrypted_json(&inner.path, &doc) {
        Ok(()) => inner.save_error = None,
        Err(err) => inner.save_error = Some(err.to_string()),
    }
}

fn fresh_id(existing: &[OutputTriggerRule]) -> String {
    loop {
        let compact = uuid::Uuid::new_v4().simple().to_string();
        let id = format!("TR-USER-{}", &compact[..8]);
        if !existing.iter().any(|rule| rule.id == id) {
            return id;
        }
    }
}

/// output-trigger 内置模块。行缓冲在 Host；这里只对完整行做匹配。
pub struct OutputTriggerModule {
    store: OutputTriggerStore,
}

impl OutputTriggerModule {
    pub fn new() -> Self {
        Self::with_store(OutputTriggerStore::builtin_ephemeral())
    }

    pub fn with_store(store: OutputTriggerStore) -> Self {
        Self { store }
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
        "Output triggers"
    }

    fn on_output_line(&self, ctx: &OutputLineCtx<'_>) -> Vec<ModuleAction> {
        // 视口重扫会反复经过同一行。通知只认 PTY 完整行（#119 / #120）。
        if ctx.origin != LineOrigin::Pty {
            return Vec::new();
        }
        let Some(hit) = self.store.scan_line(ctx.line) else {
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
    use std::sync::atomic::{AtomicU64, Ordering};

    fn engine() -> OutputTriggerEngine {
        OutputTriggerEngine::new()
    }

    fn temp_rules_path() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "mistterm-output-trigger-{}-{}-{}.json",
            std::process::id(),
            n,
            uuid::Uuid::new_v4().simple()
        ))
    }

    struct TempRules(PathBuf);
    impl Drop for TempRules {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn sample_draft(pattern: &str) -> RuleDraft {
        RuleDraft {
            id: None,
            pattern: pattern.to_string(),
            severity: TriggerSeverity::Info,
            action: TriggerAction::Notify,
            enabled: true,
            priority: 100,
        }
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
    fn invalid_regex_is_skipped_and_does_not_panic() {
        let e = OutputTriggerEngine::from_json(
            r#"{"patterns":[
                {"id":"bad","pattern":"(","severity":"error","action":"notify","priority":1},
                {"id":"good","pattern":"HELLO_MIST","severity":"info","action":"notify","priority":2}
            ]}"#,
        );
        assert!(e.scan_line("nope (").is_none());
        let hit = e
            .scan_line("echo HELLO_MIST")
            .expect("good rule still matches");
        assert_eq!(hit.rule_id, "good");
        assert_eq!(hit.severity, TriggerSeverity::Info);
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
            let actions = module.on_output_line(&OutputLineCtx::pty(line));
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
        assert!(module
            .on_output_line(&OutputLineCtx::viewport("ECONNREFUSED"))
            .is_empty());
    }

    #[test]
    fn disabled_via_settings_emits_nothing_until_reenabled() {
        let reg = ModuleRegistry::with_modules(vec![Box::new(OutputTriggerModule::new())]);
        let ctx = OutputLineCtx::pty("ECONNREFUSED");
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

    #[test]
    fn default_path_is_mistterm_config_file() {
        let path = OutputTriggerStore::default_path();
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("output-trigger-rules.json")
        );
        assert_eq!(
            path.parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str()),
            Some("mistterm")
        );
    }

    #[test]
    fn missing_user_file_matches_builtin_engine_and_writes_nothing() {
        let file = TempRules(temp_rules_path());
        assert!(!file.0.exists());
        let store = OutputTriggerStore::load_from(&file.0);
        let builtin = OutputTriggerEngine::new();
        let samples = [
            "curl: Failed to connect: ECONNREFUSED 127.0.0.1",
            "Build succeeded in 12.3s",
            "WARNING: disk almost full",
            "hello world",
        ];
        for line in samples {
            assert_eq!(store.scan_line(line), builtin.scan_line(line));
        }
        assert!(!store.customized());
        assert!(!file.0.exists());
    }

    #[test]
    fn invalid_draft_does_not_change_matches_or_create_file() {
        let file = TempRules(temp_rules_path());
        let store = OutputTriggerStore::load_from(&file.0);
        let err = store.upsert(sample_draft("(")).expect_err("bad regex");
        assert!(matches!(err, RuleError::InvalidRegex(_)));
        assert!(store.scan_line("ECONNREFUSED").is_some());
        assert!(!file.0.exists());
        assert!(store.last_save_error().is_none());
    }

    #[test]
    fn custom_rule_applies_immediately_and_roundtrips() {
        let file = TempRules(temp_rules_path());
        let store = OutputTriggerStore::load_from(&file.0);
        let saved = store.upsert(sample_draft("HELLO_MIST")).expect("save");
        assert!(store.customized());
        assert!(file.0.exists());
        let hit = store.scan_line("echo HELLO_MIST").expect("custom hit");
        assert_eq!(hit.rule_id, saved.id);
        assert_eq!(hit.severity, TriggerSeverity::Info);

        let reloaded = OutputTriggerStore::load_from(&file.0);
        assert!(reloaded.customized());
        assert_eq!(
            reloaded.scan_line("echo HELLO_MIST").map(|hit| hit.rule_id),
            Some(saved.id)
        );
        // 内置规则在第一次自定义时被一起快照，未改过的仍在。
        assert!(reloaded.scan_line("ECONNREFUSED").is_some());
    }

    #[test]
    fn disabling_one_rule_stops_only_that_rule() {
        let file = TempRules(temp_rules_path());
        let store = OutputTriggerStore::load_from(&file.0);
        store.set_enabled("TR-ERR-001", false).expect("disable");
        assert!(store.scan_line("ECONNREFUSED").is_none());
        assert!(store.scan_line("WARNING: disk almost full").is_some());
        let reloaded = OutputTriggerStore::load_from(&file.0);
        assert!(reloaded.scan_line("ECONNREFUSED").is_none());
        assert!(reloaded.scan_line("WARNING: disk almost full").is_some());
    }

    #[test]
    fn deleting_every_rule_stays_empty_after_reload() {
        let file = TempRules(temp_rules_path());
        let store = OutputTriggerStore::load_from(&file.0);
        let ids: Vec<String> = store.rules().into_iter().map(|rule| rule.id).collect();
        assert!(!ids.is_empty());
        for id in ids {
            assert!(store.delete(&id));
        }
        assert!(store.customized());
        assert!(store.scan_line("ECONNREFUSED").is_none());
        let reloaded = OutputTriggerStore::load_from(&file.0);
        assert!(reloaded.rules().is_empty());
        assert!(reloaded.scan_line("ECONNREFUSED").is_none());
    }

    #[test]
    fn reset_restores_builtin_and_removes_user_file() {
        let file = TempRules(temp_rules_path());
        let store = OutputTriggerStore::load_from(&file.0);
        store.upsert(sample_draft("HELLO_MIST")).expect("save");
        store.reset_to_builtin().expect("reset");
        assert!(!file.0.exists());
        assert!(!store.customized());
        assert!(store.scan_line("HELLO_MIST").is_none());
        assert!(store.scan_line("ECONNREFUSED").is_some());
    }

    #[test]
    fn hand_edited_bad_regex_does_not_block_sibling_rule() {
        let file = TempRules(temp_rules_path());
        std::fs::write(
            &file.0,
            r#"{"customized":true,"rules":[
                {"id":"bad","pattern":"(","severity":"error","action":"notify","enabled":true,"priority":1},
                {"id":"good","pattern":"HELLO_MIST","severity":"info","action":"notify","enabled":true,"priority":5}
            ]}"#,
        )
        .expect("write");
        let store = OutputTriggerStore::load_from(&file.0);
        let bad = store
            .rules()
            .into_iter()
            .find(|rule| rule.id == "bad")
            .expect("bad rule kept");
        assert!(bad.pattern_error.is_some());
        assert!(store.scan_line("still (").is_none());
        let hit = store.scan_line("echo HELLO_MIST").expect("sibling");
        assert_eq!(hit.rule_id, "good");
    }

    #[test]
    fn import_rejects_garbage_without_wiping_current_rules() {
        let file = TempRules(temp_rules_path());
        let store = OutputTriggerStore::load_from(&file.0);
        let err = store.import_json("{").expect_err("garbage");
        assert!(matches!(err, RuleError::InvalidFile(_)));
        assert!(store.scan_line("ECONNREFUSED").is_some());
        assert!(!store.customized());
    }

    #[test]
    fn export_import_roundtrip_and_assets_shape() {
        let file = TempRules(temp_rules_path());
        let store = OutputTriggerStore::load_from(&file.0);
        store.upsert(sample_draft("HELLO_MIST")).expect("save");
        let json = store.export_json();
        let other_file = TempRules(temp_rules_path());
        let other = OutputTriggerStore::load_from(&other_file.0);
        let report = other.import_json(&json).expect("import");
        assert!(report.imported >= 1);
        assert!(other.scan_line("echo HELLO_MIST").is_some());

        let assets_file = TempRules(temp_rules_path());
        let assets = OutputTriggerStore::load_from(&assets_file.0);
        assets.import_json(BUILTIN_JSON).expect("assets shape");
        assert_eq!(
            assets.scan_line("ECONNREFUSED").map(|hit| hit.rule_id),
            Some("TR-ERR-001".into())
        );
    }

    #[test]
    fn shared_store_updates_module_without_restart_and_respects_module_switch() {
        let file = TempRules(temp_rules_path());
        let store = OutputTriggerStore::load_from(&file.0);
        store
            .import_json(
                r#"{"customized":true,"rules":[{"id":"custom","pattern":"HELLO_MIST","severity":"info","action":"notify","enabled":true,"priority":1}]}"#,
            )
            .expect("import");
        let module = OutputTriggerModule::with_store(store.clone());
        let reg = ModuleRegistry::with_modules(vec![Box::new(module)]);
        let ctx = OutputLineCtx::pty("echo HELLO_MIST");
        assert_eq!(reg.on_output_line(&ctx).len(), 1);

        store.set_enabled("custom", false).expect("disable");
        assert!(reg.on_output_line(&ctx).is_empty());

        store.set_enabled("custom", true).expect("enable");
        reg.sync_effective_disabled(&[MODULE_ID.to_string()]);
        assert!(reg.on_output_line(&ctx).is_empty());
        reg.sync_effective_disabled(&[]);
        assert_eq!(reg.on_output_line(&ctx).len(), 1);
    }
}
