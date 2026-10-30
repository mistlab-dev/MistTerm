//! 终端输出可点击文本（display-link）：路径 / 错误码 / IP / URL。
//!
//! 规则仍是 `assets/display-link-patterns.json`。扫描只经 [`MistModule::on_output_line`]，
//! 且只认 [`LineOrigin::Viewport`]：视口文本已经过 VTE，字节区间才能对上格子。
//! 原始 PTY 行（[`LineOrigin::Pty`]）不产出装饰，避免和 output-trigger 的「一行一次」副作用搅在一起。
//! 右键「新开连接」经 [`MistModule::contribute_context_menu`] 贡献，Host 持有菜单生命周期。
//!
//! 规则解析失败或文件缺失 = 引擎为空，输出渲染零影响。

use serde::{Deserialize, Serialize};

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::capability::{
    LineOrigin, MenuCtx, MenuItem, MistModule, ModuleAction, ModuleId, OutputLineCtx,
};

/// 稳定模块 id（设置项 / `MISTTERM_DISABLE_MODULES`）。
pub const MODULE_ID: ModuleId = "display-link";

/// 与 JSON `action` 字段一致，Host 按这些字符串分发点击。
pub const ACTION_SFTP_OPEN: &str = "sftp-open";
pub const ACTION_RUNBOOK_RECALL: &str = "runbook-recall";
pub const ACTION_COPY_OR_CONNECT: &str = "copy-or-connect";
pub const ACTION_BROWSER_OPEN: &str = "browser-open";

/// 右键「新开连接到此 IP」。只在命中 IP（`copy-or-connect`）时贡献。
pub const ACTION_NEW_CONNECTION: &str = "display-link.new-connection";

/// 与 #185（output-trigger）对齐的上限。
pub const MAX_RULES: usize = 64;
pub const MAX_PATTERN_BYTES: usize = 1024;
const MAX_IMPORT_BYTES: usize = 256 * 1024;
const MAX_ID_CHARS: usize = 64;

const BUILTIN_JSON: &str = include_str!("../../assets/display-link-patterns.json");

/// 点击动作白名单（方案 §5：保存时校验 action ∈ 白名单）。
pub const ACTION_WHITELIST: [&str; 4] = [
    ACTION_SFTP_OPEN,
    ACTION_RUNBOOK_RECALL,
    ACTION_COPY_OR_CONNECT,
    ACTION_BROWSER_OPEN,
];

/// 一条链接识别规则。`pattern_error` 不落盘，加载时按正则重算。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayLinkRule {
    pub id: String,
    pub pattern: String,
    /// path / errno / ip / url（path 类命中保留 #140 系统路径过滤）
    pub category: String,
    /// 四类白名单 action 字符串，Host 按它分发点击
    pub action: String,
    pub enabled: bool,
    /// 小者先占区间（URL > errno > ip > path）
    pub priority: u8,
    /// 正则编译失败时的说明。这种规则保留在列表里，但不会参与匹配。
    pub pattern_error: Option<String>,
}

/// 编辑器提交的草稿。`id == None` 表示新建。
#[derive(Debug, Clone)]
pub struct LinkRuleDraft {
    pub id: Option<String>,
    pub pattern: String,
    pub category: String,
    pub action: String,
    pub enabled: bool,
    pub priority: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkRuleError {
    EmptyPattern,
    PatternTooLong,
    InvalidRegex(String),
    NotFound,
    TooMany,
    EmptyId,
    IdTooLong,
    BadAction,
    EmptyFile,
    InvalidFile(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkImportReport {
    pub imported: usize,
    /// 已写入、但正则无效所以不会命中的规则 id。
    pub invalid_pattern_ids: Vec<String>,
    /// 没写进去的规则（动作不在白名单、重复 id、超出条数）。
    pub skipped: Vec<String>,
}

/// 编辑前检查正则。
pub fn check_link_pattern(pattern: &str) -> Result<(), LinkRuleError> {
    if pattern.trim().is_empty() {
        return Err(LinkRuleError::EmptyPattern);
    }
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(LinkRuleError::PatternTooLong);
    }
    regex::Regex::new(pattern)
        .map(|_| ())
        .map_err(|e| LinkRuleError::InvalidRegex(e.to_string()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RuleDocument {
    #[serde(default)]
    version: String,
    #[serde(default)]
    customized: bool,
    #[serde(default)]
    rules: Vec<RuleDto>,
    /// 兼容内置 assets 的 `patterns` 别名（方案 §4）。
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
    #[serde(default)]
    category: String,
    action: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    priority: u8,
}

fn default_true() -> bool {
    true
}

fn rule_from_dto(dto: RuleDto) -> Result<DisplayLinkRule, (String, LinkRuleError)> {
    let id = dto.id.trim().to_string();
    if id.is_empty() {
        return Err((id, LinkRuleError::EmptyId));
    }
    if id.chars().count() > MAX_ID_CHARS {
        return Err((id, LinkRuleError::IdTooLong));
    }
    // action 必须在四类白名单里；非法 action 拒绝（方案 §5 默认口径）。
    if !ACTION_WHITELIST.contains(&dto.action.as_str()) {
        return Err((id, LinkRuleError::BadAction));
    }
    let pattern_error = match check_link_pattern(&dto.pattern) {
        Ok(()) => None,
        Err(LinkRuleError::InvalidRegex(msg)) => Some(msg),
        Err(LinkRuleError::PatternTooLong) => Some("pattern too long".to_string()),
        Err(LinkRuleError::EmptyPattern) => Some("empty pattern".to_string()),
        Err(_) => Some("invalid pattern".to_string()),
    };
    Ok(DisplayLinkRule {
        id,
        pattern: dto.pattern,
        category: dto.category,
        action: dto.action,
        enabled: dto.enabled,
        priority: dto.priority,
        pattern_error,
    })
}

fn rule_to_dto(rule: &DisplayLinkRule) -> RuleDto {
    RuleDto {
        id: rule.id.clone(),
        pattern: rule.pattern.clone(),
        category: rule.category.clone(),
        action: rule.action.clone(),
        enabled: rule.enabled,
        priority: rule.priority,
    }
}

fn document_rules(doc: RuleDocument) -> Vec<DisplayLinkRule> {
    let dtos = if !doc.rules.is_empty() {
        doc.rules
    } else {
        doc.patterns
    };
    dtos.into_iter()
        .filter_map(|dto| rule_from_dto(dto).ok())
        .collect()
}

fn parse_rules_lenient(json: &str) -> Vec<DisplayLinkRule> {
    match serde_json::from_str::<RuleDocument>(json) {
        Ok(doc) => document_rules(doc),
        Err(_) => Vec::new(),
    }
}

/// 编译进二进制的内置规则。用户还没自定义时，引擎用这一份。
pub fn builtin_link_rules() -> Vec<DisplayLinkRule> {
    parse_rules_lenient(BUILTIN_JSON)
}


/// 系统二进制目录前缀（#140）：`ps`/CMD 列里的这些路径是「程序本身」，
/// 不是用户可操作的文件/目录，画链接是噪声。
/// 只拦「整个路径落在系统目录下」的绝对路径；`/etc` `/var` `/opt` 下的配置/数据不拦。
const SYSTEM_BIN_PREFIXES: [&str; 8] = [
    "/usr/bin/", "/usr/sbin/", "/usr/local/bin/", "/usr/local/sbin/",
    "/bin/", "/sbin/", "/lib/", "/lib64/",
];

fn is_system_binary_path(text: &str) -> bool {
    if !text.starts_with('/') {
        return false;
    }
    if SYSTEM_BIN_PREFIXES.iter().any(|p| text.starts_with(p)) {
        return true;
    }
    // 目录名本身（无尾随段，如 `/usr/sbin`、`/bin`）也不链。
    matches!(
        text,
        "/usr/bin" | "/usr/sbin" | "/usr/local/bin" | "/usr/local/sbin" | "/bin" | "/sbin"
            | "/lib" | "/lib64"
    )
}

/// 从输出行中识别出的可点击片段。
#[derive(Debug, Clone, PartialEq, Eq)]
struct DisplayLink {
    start: usize,
    end: usize,
    rule_id: String,
    action: DisplayLinkAction,
    text: String,
}

impl DisplayLink {
    pub fn rule_id(&self) -> &str {
        &self.rule_id
    }

    pub fn action_str(&self) -> &'static str {
        self.action.as_str()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn range(&self) -> (usize, usize) {
        (self.start, self.end)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisplayLinkAction {
    SftpOpen,
    RunbookRecall,
    Copy,
    BrowserOpen,
}

impl DisplayLinkAction {
    fn from_str(s: &str) -> Option<Self> {
        match s {
            ACTION_SFTP_OPEN => Some(Self::SftpOpen),
            ACTION_RUNBOOK_RECALL => Some(Self::RunbookRecall),
            ACTION_COPY_OR_CONNECT => Some(Self::Copy),
            ACTION_BROWSER_OPEN => Some(Self::BrowserOpen),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::SftpOpen => ACTION_SFTP_OPEN,
            Self::RunbookRecall => ACTION_RUNBOOK_RECALL,
            Self::Copy => ACTION_COPY_OR_CONNECT,
            Self::BrowserOpen => ACTION_BROWSER_OPEN,
        }
    }
}

struct CompiledPattern {
    id: String,
    action: DisplayLinkAction,
    /// path / errno / ip / url（#140：path 类命中做系统路径过滤）
    category: String,
    priority: u8,
    regex: regex::Regex,
}

/// 规则引擎：只含已启用且正则合法的规则。
/// 上限保护：单行最多返回 16 个链接，防止病态输出拖垮渲染。
struct DisplayLinkEngine {
    patterns: Vec<CompiledPattern>,
    max_hits_per_line: usize,
}

impl DisplayLinkEngine {
    /// 解析失败返回空引擎（功能关闭）。
    fn from_json(json: &str) -> Self {
        Self::from_rules(&parse_rules_lenient(json))
    }

    fn from_rules(rules: &[DisplayLinkRule]) -> Self {
        let mut patterns = Vec::new();
        for rule in rules {
            if !rule.enabled || rule.pattern_error.is_some() {
                continue;
            }
            let Some(action) = DisplayLinkAction::from_str(&rule.action) else {
                continue;
            };
            let Ok(regex) = regex::Regex::new(&rule.pattern) else {
                continue;
            };
            patterns.push(CompiledPattern {
                id: rule.id.clone(),
                action,
                category: rule.category.clone(),
                priority: rule.priority,
                regex,
            });
        }
        patterns.sort_by_key(|p| p.priority);
        Self {
            patterns,
            max_hits_per_line: 16,
        }
    }

    fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// 对一行输出做匹配，返回互不重叠、按出现位置排序的链接。
    ///
    /// 区间裁决：同一区间被多条规则命中时，取 priority 最小（最优先）的；
    /// 已占用区间不再接受新链接（先到先得，按 priority 顺序扫描）。
    /// 系统二进制路径（`/usr/sbin/sshd` 等）不产链接（#140）。
    fn scan(&self, line: &str) -> Vec<DisplayLink> {
        if self.patterns.is_empty() || line.is_empty() || line.len() > 8192 {
            return Vec::new();
        }
        let mut occupied: Vec<(usize, usize)> = Vec::new();
        let mut hits: Vec<DisplayLink> = Vec::new();
        for p in &self.patterns {
            for m in p.regex.captures_iter(line) {
                let (start, end, text) = match m.get(1).or_else(|| m.get(0)) {
                    Some(mm) => (mm.start(), mm.end(), mm.as_str().to_string()),
                    None => continue,
                };
                if end <= start {
                    continue;
                }
                if p.category == "path" && is_system_binary_path(&text) {
                    continue;
                }
                if occupied.iter().any(|(s, e)| start < *e && end > *s) {
                    continue;
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

/// SFTP 列表目标目录：去掉末尾斜杠；若末段像文件名（含 `.`）则缩到父目录。
/// 例：`/var/log/nginx/error.log` → `/var/log/nginx`；`/var/log` → `/var/log`。
pub fn sftp_list_dir(path: &str) -> String {
    let trimmed = path.trim().trim_end_matches(['/', '\\']);
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

struct LinkInner {
    path: PathBuf,
    /// `false` 时编辑不落盘（测试用内置快照，避免写到开发者配置目录）。
    persist: bool,
    customized: bool,
    rules: Vec<DisplayLinkRule>,
    engine: DisplayLinkEngine,
    save_error: Option<String>,
}

/// 用户链接规则与编译后的引擎。UI 和 `DisplayLinkModule` 各持一份 `Clone`（内部是 `Arc`）。
#[derive(Clone)]
pub struct DisplayLinkStore {
    inner: Arc<RwLock<LinkInner>>,
}

impl DisplayLinkStore {
    /// 配置目录下的用户规则。文件不存在 = 还没自定义 = 内置规则，且不创建文件。
    pub fn load() -> Self {
        Self::load_from(&Self::default_path())
    }

    pub fn load_from(path: &Path) -> Self {
        if !path.exists() {
            return Self::from_parts(path, builtin_link_rules(), false, true);
        }
        let doc: RuleDocument = crate::security::encrypted_file::load_encrypted_json(path);
        let customized_flag = doc.customized;
        let has_rules = !doc.rules.is_empty() || !doc.patterns.is_empty();
        if !customized_flag && !has_rules {
            log::warn!(
                "display-link rules file {} is empty or unreadable; using built-in rules",
                path.display()
            );
            return Self::from_parts(path, builtin_link_rules(), false, true);
        }
        let rules = document_rules(doc);
        Self::from_parts(path, rules, true, true)
    }

    /// 只含内置规则，不读也不写配置目录。`builtin_modules()` 与单测走这里。
    pub fn builtin_ephemeral() -> Self {
        Self::from_parts(&Self::default_path(), builtin_link_rules(), false, false)
    }

    pub fn default_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("display-link-rules.json");
        p
    }

    fn from_parts(
        path: &Path,
        rules: Vec<DisplayLinkRule>,
        customized: bool,
        persist: bool,
    ) -> Self {
        let engine = DisplayLinkEngine::from_rules(&rules);
        Self {
            inner: Arc::new(RwLock::new(LinkInner {
                path: path.to_path_buf(),
                persist,
                customized,
                rules,
                engine,
                save_error: None,
            })),
        }
    }

    fn write<R>(&self, f: impl FnOnce(&mut LinkInner) -> R) -> R {
        let mut guard = self.inner.write().unwrap_or_else(|err| err.into_inner());
        f(&mut guard)
    }

    fn read<R>(&self, f: impl FnOnce(&LinkInner) -> R) -> R {
        let guard = self.inner.read().unwrap_or_else(|err| err.into_inner());
        f(&guard)
    }

    pub fn customized(&self) -> bool {
        self.read(|inner| inner.customized)
    }

    pub fn rules(&self) -> Vec<DisplayLinkRule> {
        self.read(|inner| inner.rules.clone())
    }

    pub fn last_save_error(&self) -> Option<String> {
        self.read(|inner| inner.save_error.clone())
    }

    /// 对一行视口文本做匹配。引擎为空或行长超限返回空（与现网一致）。
    pub fn scan(&self, line: &str) -> Vec<DisplayLink> {
        self.read(|inner| inner.engine.scan(line))
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), LinkRuleError> {
        self.write(|inner| {
            let Some(rule) = inner.rules.iter_mut().find(|rule| rule.id == id) else {
                return Err(LinkRuleError::NotFound);
            };
            rule.enabled = enabled;
            inner.customized = true;
            recompile(inner);
            remember_save(inner);
            Ok(())
        })
    }

    pub fn upsert(&self, draft: LinkRuleDraft) -> Result<DisplayLinkRule, LinkRuleError> {
        check_link_pattern(&draft.pattern)?;
        if !ACTION_WHITELIST.contains(&draft.action.as_str()) {
            return Err(LinkRuleError::BadAction);
        }
        self.write(|inner| {
            let id = if let Some(id) = draft.id.as_deref() {
                if !inner.rules.iter().any(|rule| rule.id == id) {
                    return Err(LinkRuleError::NotFound);
                }
                id.to_string()
            } else {
                if inner.rules.len() >= MAX_RULES {
                    return Err(LinkRuleError::TooMany);
                }
                fresh_link_id(&inner.rules)
            };
            let rule = DisplayLinkRule {
                id,
                pattern: draft.pattern,
                category: draft.category,
                action: draft.action,
                enabled: draft.enabled,
                priority: draft.priority,
                pattern_error: None,
            };
            if let Some(slot) = inner.rules.iter_mut().find(|existing| existing.id == rule.id) {
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
            inner.rules = builtin_link_rules();
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

    /// 用 JSON 整表替换当前规则（支持 `rules`，也支持内置 assets 的 `patterns`）。
    /// 正则无效的条目会留下但不会命中；action 不在白名单的条目跳过。
    pub fn import_json(&self, json: &str) -> Result<LinkImportReport, LinkRuleError> {
        if json.len() > MAX_IMPORT_BYTES {
            return Err(LinkRuleError::InvalidFile("file is too large".into()));
        }
        let doc: RuleDocument =
            serde_json::from_str(json).map_err(|err| LinkRuleError::InvalidFile(err.to_string()))?;
        let dtos = if !doc.rules.is_empty() {
            doc.rules
        } else {
            doc.patterns
        };
        if dtos.is_empty() {
            return Err(LinkRuleError::EmptyFile);
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
            return Err(LinkRuleError::InvalidFile("no rules could be imported".into()));
        }
        self.write(|inner| {
            inner.rules = rules;
            inner.customized = true;
            recompile(inner);
            remember_save(inner);
        });
        Ok(LinkImportReport {
            imported: self.rules().len(),
            invalid_pattern_ids,
            skipped,
        })
    }
}

fn recompile(inner: &mut LinkInner) {
    inner.engine = DisplayLinkEngine::from_rules(&inner.rules);
}

fn remember_save(inner: &mut LinkInner) {
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

fn fresh_link_id(existing: &[DisplayLinkRule]) -> String {
    loop {
        let compact = uuid::Uuid::new_v4().simple().to_string();
        let id = format!("DL-USER-{}", &compact[..8]);
        if !existing.iter().any(|rule| rule.id == id) {
            return id;
        }
    }
}

/// display-link 内置模块。规则经 [`DisplayLinkStore`] 热更新；禁用后 registry 不再调用钩子。
pub struct DisplayLinkModule {
    store: DisplayLinkStore,
}

impl DisplayLinkModule {
    pub fn new() -> Self {
        Self::with_store(DisplayLinkStore::builtin_ephemeral())
    }

    pub fn with_store(store: DisplayLinkStore) -> Self {
        Self { store }
    }
}

impl Default for DisplayLinkModule {
    fn default() -> Self {
        Self::new()
    }
}

impl MistModule for DisplayLinkModule {
    fn id(&self) -> ModuleId {
        MODULE_ID
    }

    fn name_zh(&self) -> &str {
        "输出可点击链接"
    }

    fn name_en(&self) -> &str {
        "Clickable output links"
    }

    fn on_output_line(&self, ctx: &OutputLineCtx<'_>) -> Vec<ModuleAction> {
        // 通知类模块认 Pty；装饰必须对齐视口格子，重扫同一屏不能当成新输出。
        if ctx.origin != LineOrigin::Viewport {
            return Vec::new();
        }
        self.store.scan(ctx.line)
            .into_iter()
            .map(|hit| ModuleAction::DecorateLink {
                start: hit.start,
                end: hit.end,
                action: hit.action.as_str().to_string(),
                text: hit.text,
            })
            .collect()
    }

    fn contribute_context_menu(&self, ctx: &MenuCtx<'_>) -> Vec<MenuItem> {
        // IP 右键才出现「新开连接」。URL / 路径 / 错误码 / 空白处不贡献，避免菜单语义变化。
        if ctx.link_action != Some(ACTION_COPY_OR_CONNECT) {
            return Vec::new();
        }
        vec![MenuItem {
            action: ACTION_NEW_CONNECTION.to_string(),
            label_zh: "新开连接到此 IP".to_string(),
            label_en: "New connection to this IP".to_string(),
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::ModuleRegistry;

    fn engine() -> DisplayLinkEngine {
        DisplayLinkEngine::from_json(BUILTIN_JSON)
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
        let errno = hits
            .iter()
            .find(|h| h.rule_id == "DL-ERR-001")
            .expect("errno hit");
        assert_eq!(errno.text, "ECONNREFUSED");
        assert_eq!(errno.action, DisplayLinkAction::RunbookRecall);
    }

    #[test]
    fn ipv4_hit_and_version_not_mismatched() {
        let e = engine();
        let hits = e.scan("connect to 192.168.1.10 ok; version 1.2.3 shipped");
        let ip = hits
            .iter()
            .find(|h| h.rule_id == "DL-IPV4-001")
            .expect("ip hit");
        assert_eq!(ip.text, "192.168.1.10");
        assert!(hits.iter().all(|h| h.text != "1.2.3"));
    }

    #[test]
    fn url_hit() {
        let e = engine();
        let hits = e.scan("see https://docs.example.com/guide for details");
        assert!(hits
            .iter()
            .any(|h| h.action == DisplayLinkAction::BrowserOpen
                && h.text.starts_with("https://docs.example.com/guide")));
    }

    #[test]
    fn path_hit() {
        let e = engine();
        let hits = e.scan("failed to open /var/log/nginx/error.log");
        let p = hits
            .iter()
            .find(|h| h.action == DisplayLinkAction::SftpOpen);
        assert!(p.is_some(), "path hit expected, got {:?}", hits);
    }

    #[test]
    fn sftp_list_dir_shrinks_file_to_parent() {
        assert_eq!(sftp_list_dir("/var/log/nginx/error.log"), "/var/log/nginx");
        assert_eq!(sftp_list_dir("/var/log/nginx/"), "/var/log/nginx");
        assert_eq!(sftp_list_dir("/var/log"), "/var/log");
        assert_eq!(sftp_list_dir("/"), "/");
        assert_eq!(sftp_list_dir(""), "/");
    }

    #[test]
    fn overlaps_priority_url_beats_ip() {
        let e = engine();
        let hits = e.scan("at http://10.0.0.7:8080/health");
        let url = hits
            .iter()
            .find(|h| h.action == DisplayLinkAction::BrowserOpen);
        assert!(url.is_some(), "url should win overlap");
        let ip_inside = hits
            .iter()
            .find(|h| h.action == DisplayLinkAction::Copy && h.text == "10.0.0.7");
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

    #[test]
    fn viewport_line_becomes_decorate_link_pty_line_does_not() {
        let m = DisplayLinkModule::new();
        let line = "curl: (7) Failed to connect: ECONNREFUSED 127.0.0.1:6379";
        assert!(m.on_output_line(&OutputLineCtx::pty(line)).is_empty());
        let actions = m.on_output_line(&OutputLineCtx::viewport(line));
        let errno = actions.iter().find_map(|a| match a {
            ModuleAction::DecorateLink { action, text, .. }
                if action == ACTION_RUNBOOK_RECALL && text == "ECONNREFUSED" =>
            {
                Some(())
            }
            _ => None,
        });
        assert!(
            errno.is_some(),
            "viewport scan must decorate errno, got {actions:?}"
        );
        assert!(actions.iter().any(|a| matches!(
            a,
            ModuleAction::DecorateLink { action, text, .. }
                if action == ACTION_COPY_OR_CONNECT && text == "127.0.0.1"
        )));
    }

    #[test]
    fn context_menu_only_for_ip_link() {
        let m = DisplayLinkModule::new();
        let ip = m.contribute_context_menu(&MenuCtx {
            link_action: Some(ACTION_COPY_OR_CONNECT),
            link_text: Some("192.168.1.10"),
        });
        assert_eq!(ip.len(), 1);
        assert_eq!(ip[0].action, ACTION_NEW_CONNECTION);
        assert_eq!(ip[0].label_zh, "新开连接到此 IP");
        assert_eq!(ip[0].label_en, "New connection to this IP");

        assert!(m
            .contribute_context_menu(&MenuCtx {
                link_action: Some(ACTION_BROWSER_OPEN),
                link_text: Some("https://example.com"),
            })
            .is_empty());
        assert!(m.contribute_context_menu(&MenuCtx::plain()).is_empty());
    }

    #[test]
    fn disabled_module_is_absent_from_registry() {
        let reg = ModuleRegistry::with_modules(crate::modules::builtin_modules(std::sync::Arc::new(
            std::sync::Mutex::new(crate::core::CmdAuditEngine::new()),
        )));
        assert!(reg.is_enabled(MODULE_ID));
        let line = "ECONNREFUSED 10.0.0.8 https://example.com/a";
        assert!(!reg
            .on_output_line_for(MODULE_ID, &OutputLineCtx::viewport(line))
            .is_empty());

        reg.set_enabled(MODULE_ID, false);
        assert!(reg
            .on_output_line_for(MODULE_ID, &OutputLineCtx::viewport(line))
            .is_empty());
        assert!(reg
            .on_output_line(&OutputLineCtx::viewport(line))
            .is_empty());
        assert!(reg
            .context_menu_items(&MenuCtx {
                link_action: Some(ACTION_COPY_OR_CONNECT),
                link_text: Some("10.0.0.8"),
            })
            .is_empty());
    }

    // --- #140 系统二进制路径不产链接 ---

    #[test]
    fn system_binary_paths_are_not_linked() {
        let engine = DisplayLinkEngine::from_json(BUILTIN_JSON);
        // ps 输出典型行：CMD 列的系统二进制路径不画下划线（#140）。
        let line = "/usr/sbin/sshd  /usr/sbin/crond  /usr/bin/python";
        assert!(engine.scan(line).is_empty());

        // 目录名本身也不链。
        assert!(engine.scan("/usr/sbin /bin /lib64").is_empty());

        // 系统目录下的配置/数据（/etc /var /opt）不受影响，仍可点。
        let cfg = engine.scan("see /etc/nginx/nginx.conf and /var/log/app/error.log");
        assert!(cfg.iter().any(|l| l.text == "/etc/nginx/nginx.conf"));
        assert!(cfg.iter().any(|l| l.text == "/var/log/app/error.log"));

        // 用户/项目路径不受影响。
        let user = engine.scan("edit /home/tian/app/config.toml and /opt/svc/run.sh");
        assert!(user.iter().any(|l| l.text == "/home/tian/app/config.toml"));
        assert!(user.iter().any(|l| l.text == "/opt/svc/run.sh"));

        // 相对路径与 ~ 路径不在过滤范围（deny-list 只拦绝对路径）。
        let rel = engine.scan("run ~/bin/tool and /usr/local/bin/node");
        assert!(rel.iter().any(|l| l.text == "~/bin/tool"));
        assert!(!rel.iter().any(|l| l.text == "/usr/local/bin/node"));
    }

    #[test]
    fn is_system_binary_path_unit() {
        assert!(is_system_binary_path("/usr/sbin/sshd"));
        assert!(is_system_binary_path("/usr/bin/python3.11"));
        assert!(is_system_binary_path("/bin/bash"));
        assert!(is_system_binary_path("/usr/local/bin/node"));
        assert!(is_system_binary_path("/usr/sbin"));
        assert!(!is_system_binary_path("/etc/nginx/nginx.conf"));
        assert!(!is_system_binary_path("/var/log/app"));
        assert!(!is_system_binary_path("/home/tian/app"));
        assert!(!is_system_binary_path("./sbin/tool"));
        assert!(!is_system_binary_path("./usr/local/tool"));
        assert!(!is_system_binary_path("~/bin/tool"));
        assert!(!is_system_binary_path("~/bin/tool"));
        assert!(!is_system_binary_path("/usr/share/doc")); // /usr/ 前缀不拦，拦的是 /usr/bin|sbin
    }

    // --- #186 DisplayLinkStore ---

    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_rules_path() -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "mistterm-display-link-{}-{}-{}.json",
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

    fn link_draft(pattern: &str) -> LinkRuleDraft {
        LinkRuleDraft {
            id: None,
            pattern: pattern.to_string(),
            category: "url".to_string(),
            action: ACTION_BROWSER_OPEN.to_string(),
            enabled: true,
            priority: 100,
        }
    }

    #[test]
    fn store_builtin_ephemeral_matches_builtin_engine() {
        let store = DisplayLinkStore::builtin_ephemeral();
        assert!(!store.customized());
        let rules = store.rules();
        assert!(rules.len() >= 4);
        // 未自定义时行为与烤进包的内置一致（验收点：抽样 url/ip/errno）。
        let hits = store.scan("see https://example.com/x and 10.1.2.3 ECONNREFUSED");
        assert!(hits.iter().any(|l| l.rule_id() == "DL-URL-001"));
        assert!(hits.iter().any(|l| l.rule_id() == "DL-IPV4-001"));
        assert!(hits.iter().any(|l| l.rule_id() == "DL-ERR-001"));
    }

    #[test]
    fn store_upsert_delete_and_persist_roundtrip() {
        let path = temp_rules_path();
        let _keep = TempRules(path.clone());
        let store = DisplayLinkStore::load_from(&path);
        assert!(!path.exists(), "未自定义不写用户文件");

        // Pattern must not collide with builtin URL (priority 10); use a unique token.
        let rule = store
            .upsert(link_draft("mistterm-hot-[a-z]+"))
            .expect("upsert");
        assert!(store.customized());
        assert!(path.exists(), "首次编辑落盘 customized=true");
        assert_eq!(rule.id, "DL-USER-".to_string() + &rule.id[8..]);

        // 热更新：下一帧 scan 立即用新规则。
        let hits = store.scan("open mistterm-hot-reload now");
        assert!(hits
            .iter()
            .any(|l| l.rule_id() == rule.id && l.action_str() == ACTION_BROWSER_OPEN));

        // 重启路径：从磁盘读回同一份规则。
        let reloaded = DisplayLinkStore::load_from(&path);
        assert!(reloaded.customized());
        assert!(reloaded.rules().iter().any(|r| r.id == rule.id));

        // 删除 + 恢复内置。
        assert!(store.delete(&rule.id));
        store.reset_to_builtin().expect("reset");
        assert!(!store.customized());
        assert!(!path.exists(), "重置删掉用户文件");
    }

    #[test]
    fn store_rejects_bad_action_and_bad_regex() {
        let store = DisplayLinkStore::builtin_ephemeral();
        let mut bad_action = link_draft("x+");
        bad_action.action = "rm-rf".to_string();
        assert!(matches!(
            store.upsert(bad_action),
            Err(LinkRuleError::BadAction)
        ));
        assert!(matches!(
            store.upsert(link_draft("(")),
            Err(LinkRuleError::InvalidRegex(_))
        ));
    }

    #[test]
    fn store_import_and_disable() {
        let store = DisplayLinkStore::builtin_ephemeral();
        // 先停用一条内置规则再恢复。
        let first = store.rules()[0].id.clone();
        store.set_enabled(&first, false).expect("disable");
        assert!(store
            .rules()
            .iter()
            .find(|r| r.id == first)
            .is_some_and(|r| !r.enabled));

        // 导入：合法 + 非法正则 + 白名单外 action。
        let json = r#"{"rules":[
            {"id":"IMP-1","pattern":"imp-[0-9]+","category":"errno","action":"runbook-recall","priority":50},
            {"id":"IMP-2","pattern":"(","category":"path","action":"sftp-open"},
            {"id":"IMP-3","pattern":"y+","category":"url","action":"no-such"}
        ]}"#;
        let report = store.import_json(json).expect("import");
        assert_eq!(report.imported, 2);
        assert_eq!(report.invalid_pattern_ids, vec!["IMP-2".to_string()]);
        assert!(report.skipped.iter().any(|s| s.contains("IMP-3")));
        // 坏正则留在列表但不参与匹配。
        assert!(store.rules().iter().any(|r| r.id == "IMP-2"));
        let hits = store.scan("value imp-42 here");
        assert!(hits.iter().any(|l| l.rule_id() == "IMP-1"));
    }

    #[test]
    fn module_uses_store_hot_reload() {
        let store = DisplayLinkStore::builtin_ephemeral();
        let module = DisplayLinkModule::with_store(store.clone());
        use crate::capability::{LineOrigin, OutputLineCtx};
        let scan = |module: &DisplayLinkModule, line: &str| {
            let ctx = OutputLineCtx {
                line,
                origin: LineOrigin::Viewport,
            };
            module.on_output_line(&ctx)
        };
        assert!(scan(&module, "plain text").is_empty());
        let rule = store
            .upsert(link_draft("MAGICLINK-[0-9]+"))
            .expect("upsert");
        let actions = scan(&module, "got MAGICLINK-7 here");
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            ModuleAction::DecorateLink { action, text, .. } => {
                assert_eq!(action, ACTION_BROWSER_OPEN);
                assert_eq!(text, "MAGICLINK-7");
            }
            other => panic!("expected DecorateLink, got {other:?}"),
        }
        assert!(store.delete(&rule.id));
    }
}
