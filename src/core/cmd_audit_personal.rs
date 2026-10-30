//! 个人本地命令检查规则存储（#187）。
//!
//! 与团队同步（`CmdAuditCacheStore`）完全独立：本文件只存本机用户在
//! 「工具 → 自动化规则 → 本地命令检查」里编辑的个人规则与内置停用清单。
//! 团队规则仍走服务端 sync，本存储不涉及、不覆盖。
//!
//! 字段与团队 `CmdAuditRule` 对齐（方案 §5），便于以后导出到管理端。
//! 校验：保存时硬拒绝空 pattern / 非法正则（命令检查误放行成本更高）。

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::core::cmd_audit::CmdAuditRule;

pub const MAX_PERSONAL_RULES: usize = 64;
pub const MAX_PATTERN_BYTES: usize = 1024;
const MAX_ID_CHARS: usize = 64;

const PERSONAL_ACTIONS: [&str; 4] = ["block", "confirm", "alert", "allow"];
const MATCH_TYPES: [&str; 4] = ["regex", "prefix", "contains", "exact"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersonalRuleError {
    EmptyPattern,
    PatternTooLong,
    InvalidRegex(String),
    BadAction,
    BadMatchType,
    EmptyId,
    IdTooLong,
    NotFound,
    TooMany,
    EmptyFile,
    InvalidFile(String),
}

impl PersonalRuleError {
    pub fn explain(&self, zh: bool) -> String {
        match self {
            Self::EmptyPattern => "请填写匹配模式. 没有保存.".into(),
            Self::PatternTooLong => "这条匹配模式太长. 没有保存.".into(),
            Self::InvalidRegex(msg) => format!("正则表达式无效, 没有保存: {msg}"),
            Self::BadAction => "无法识别的动作. 请用 block, confirm, alert 或 allow.".into(),
            Self::BadMatchType => "无法识别的匹配方式. 请用 regex, prefix, contains 或 exact.".into(),
            Self::EmptyId => "规则 ID 为空.".into(),
            Self::IdTooLong => "规则 ID 太长.".into(),
            Self::NotFound => "这条规则已经不在了.".into(),
            Self::TooMany => "规则已满 (最多 64 条).".into(),
            Self::EmptyFile => "文件里没有规则. 当前规则没有改动.".into(),
            Self::InvalidFile(msg) => format!("无法导入这个文件, 当前规则没有改动: {msg}"),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PersonalDocument {
    #[serde(default)]
    version: String,
    #[serde(default)]
    customized: bool,
    #[serde(default)]
    personal_enabled: bool,
    #[serde(default)]
    disabled_builtin_ids: Vec<String>,
    #[serde(default)]
    rules: Vec<CmdAuditRule>,
}

#[derive(Debug, Clone)]
pub struct PersonalDraft {
    pub id: Option<String>,
    pub name: String,
    pub pattern: String,
    pub match_type: String,
    pub action: String,
    pub scope: String,
    pub description: String,
    pub priority: i32,
    pub enabled: bool,
}

/// 编辑/实时校验用（UI 实时提示与保存走同一套校验）。
pub fn check_pattern_for_ui(pattern: &str, match_type: &str) -> Result<(), PersonalRuleError> {
    check_pattern(pattern, match_type)
}

fn check_pattern(pattern: &str, match_type: &str) -> Result<(), PersonalRuleError> {
    if pattern.trim().is_empty() {
        return Err(PersonalRuleError::EmptyPattern);
    }
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(PersonalRuleError::PatternTooLong);
    }
    // 只有 regex 需要编译校验；其余是字面匹配。
    if crate::core::cmd_audit::match_type_of(match_type) == "regex" {
        regex::Regex::new(pattern)
            .map(|_| ())
            .map_err(|e| PersonalRuleError::InvalidRegex(e.to_string()))?;
    }
    Ok(())
}

fn validate(draft: &PersonalDraft) -> Result<(), PersonalRuleError> {
    if !PERSONAL_ACTIONS.contains(&draft.action.as_str()) {
        return Err(PersonalRuleError::BadAction);
    }
    if !MATCH_TYPES.contains(&draft.match_type.as_str()) {
        return Err(PersonalRuleError::BadMatchType);
    }
    check_pattern(&draft.pattern, &draft.match_type)
}

struct Inner {
    path: PathBuf,
    persist: bool,
    customized: bool,
    personal_enabled: bool,
    disabled_builtin_ids: Vec<String>,
    rules: Vec<CmdAuditRule>,
    save_error: Option<String>,
}

/// 个人命令检查规则。UI 持一份 `Clone`（内部 `Arc<RwLock>`），
/// 保存成功后调用 [`CmdAuditPersonalStore::sync_to_engine`] 热更新引擎个人层。
#[derive(Clone)]
pub struct CmdAuditPersonalStore {
    inner: Arc<RwLock<Inner>>,
}

impl CmdAuditPersonalStore {
    pub fn load() -> Self {
        Self::load_from(&Self::default_path())
    }

    pub fn load_from(path: &Path) -> Self {
        if !path.exists() {
            return Self::from_parts(path, false, false, Vec::new(), Vec::new(), true);
        }
        let doc: PersonalDocument = crate::security::encrypted_file::load_encrypted_json(path);
        if !doc.customized && doc.rules.is_empty() && doc.disabled_builtin_ids.is_empty() {
            log::warn!(
                "cmd-audit personal rules file {} is empty or unreadable; starting fresh",
                path.display()
            );
            return Self::from_parts(path, false, false, Vec::new(), Vec::new(), true);
        }
        Self::from_parts(
            path,
            doc.customized,
            doc.personal_enabled,
            doc.disabled_builtin_ids,
            doc.rules,
            true,
        )
    }

    pub fn ephemeral() -> Self {
        Self::from_parts(&Self::default_path(), false, false, Vec::new(), Vec::new(), false)
    }

    pub fn default_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("cmd-audit-personal-rules.json");
        p
    }

    fn from_parts(
        path: &Path,
        customized: bool,
        personal_enabled: bool,
        disabled_builtin_ids: Vec<String>,
        rules: Vec<CmdAuditRule>,
        persist: bool,
    ) -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner {
                path: path.to_path_buf(),
                persist,
                customized,
                personal_enabled,
                disabled_builtin_ids,
                rules,
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

    pub fn personal_enabled(&self) -> bool {
        self.read(|inner| inner.personal_enabled)
    }

    pub fn rules(&self) -> Vec<CmdAuditRule> {
        self.read(|inner| inner.rules.clone())
    }

    pub fn disabled_builtin_ids(&self) -> Vec<String> {
        self.read(|inner| inner.disabled_builtin_ids.clone())
    }

    pub fn last_save_error(&self) -> Option<String> {
        self.read(|inner| inner.save_error.clone())
    }

    /// 「启用个人本地检查」总开关（方案 §4-A）。也写回引擎。
    pub fn set_personal_enabled(&self, enabled: bool, engine: &std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>) {
        self.write(|inner| {
            inner.personal_enabled = enabled;
            inner.customized = true;
            remember_save(inner);
        });
        self.sync_to_engine(engine);
    }

    pub fn upsert(
        &self,
        draft: PersonalDraft,
        engine: &std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>,
    ) -> Result<CmdAuditRule, PersonalRuleError> {
        validate(&draft)?;
        let rule = self.write(|inner| {
            let id = if let Some(id) = draft.id.as_deref() {
                if !inner.rules.iter().any(|r| r.id == id) {
                    return Err(PersonalRuleError::NotFound);
                }
                id.to_string()
            } else {
                if inner.rules.len() >= MAX_PERSONAL_RULES {
                    return Err(PersonalRuleError::TooMany);
                }
                fresh_id(&inner.rules)
            };
            let rule = CmdAuditRule {
                id,
                name: draft.name,
                pattern: draft.pattern,
                match_type: draft.match_type,
                scope: draft.scope,
                action: draft.action,
                description: draft.description,
                priority: draft.priority,
                enabled: draft.enabled,
            };
            if let Some(slot) = inner.rules.iter_mut().find(|r| r.id == rule.id) {
                *slot = rule.clone();
            } else {
                inner.rules.push(rule.clone());
            }
            inner.customized = true;
            remember_save(inner);
            Ok(rule)
        })?;
        self.sync_to_engine(engine);
        Ok(rule)
    }

    pub fn delete(
        &self,
        id: &str,
        engine: &std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>,
    ) -> bool {
        let deleted = self.write(|inner| {
            let before = inner.rules.len();
            inner.rules.retain(|r| r.id != id);
            if inner.rules.len() == before {
                return false;
            }
            inner.customized = true;
            remember_save(inner);
            true
        });
        if deleted {
            self.sync_to_engine(engine);
        }
        deleted
    }

    /// 对内置规则做本地停用 / 恢复（只记 id，不改 pattern）。
    pub fn set_builtin_disabled(
        &self,
        builtin_id: &str,
        disabled: bool,
        engine: &std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>,
    ) {
        self.write(|inner| {
            if disabled {
                if !inner.disabled_builtin_ids.iter().any(|s| s == builtin_id) {
                    inner.disabled_builtin_ids.push(builtin_id.to_string());
                }
            } else {
                inner.disabled_builtin_ids.retain(|s| s != builtin_id);
            }
            inner.customized = true;
            remember_save(inner);
        });
        self.sync_to_engine(engine);
    }

    /// 清空个人层回内置（删用户文件 + 引擎个人层清零）。
    pub fn reset_to_builtin(
        &self,
        engine: &std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>,
    ) -> Result<(), String> {
        self.write(|inner| {
            if inner.persist && inner.path.exists() {
                std::fs::remove_file(&inner.path).map_err(|err| err.to_string())?;
            }
            inner.rules.clear();
            inner.disabled_builtin_ids.clear();
            inner.personal_enabled = false;
            inner.customized = false;
            inner.save_error = None;
            Ok::<(), String>(())
        })?;
        self.sync_to_engine(engine);
        Ok(())
    }

    /// 保存成功后同进程热更新引擎个人层（方案 §6-3，无需重启）。
    pub fn sync_to_engine(&self, engine: &std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>) {
        let (rules, disabled, enabled) = self.read(|inner| {
            (
                inner.rules.clone(),
                inner.disabled_builtin_ids.clone().into_iter().collect(),
                inner.personal_enabled,
            )
        });
        if let Ok(mut engine) = engine.lock() {
            engine.apply_personal(&rules, disabled, enabled);
        }
    }

    pub fn export_json(&self) -> String {
        self.read(|inner| {
            let doc = PersonalDocument {
                version: "1".to_string(),
                customized: true,
                personal_enabled: inner.personal_enabled,
                disabled_builtin_ids: inner.disabled_builtin_ids.clone(),
                rules: inner.rules.clone(),
            };
            serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string())
        })
    }

    pub fn import_json(
        &self,
        json: &str,
        engine: &std::sync::Mutex<crate::core::cmd_audit::CmdAuditEngine>,
    ) -> Result<usize, PersonalRuleError> {
        let doc: PersonalDocument = serde_json::from_str(json)
            .map_err(|err| PersonalRuleError::InvalidFile(err.to_string()))?;
        if doc.rules.is_empty() {
            return Err(PersonalRuleError::EmptyFile);
        }
        // #187 复审：导入同样受 64 条上限约束，不让 CRUD 绕过。
        if doc.rules.len() > MAX_PERSONAL_RULES {
            return Err(PersonalRuleError::TooMany);
        }
        for rule in &doc.rules {
            if rule.id.trim().is_empty() {
                return Err(PersonalRuleError::EmptyId);
            }
            validate(&PersonalDraft {
                id: Some(rule.id.clone()),
                name: rule.name.clone(),
                pattern: rule.pattern.clone(),
                match_type: rule.match_type.clone(),
                action: rule.action.clone(),
                scope: rule.scope.clone(),
                description: rule.description.clone(),
                priority: rule.priority,
                enabled: rule.enabled,
            })?;
        }
        self.write(|inner| {
            inner.rules = doc.rules;
            inner.disabled_builtin_ids = doc.disabled_builtin_ids;
            inner.personal_enabled = doc.personal_enabled;
            inner.customized = true;
            remember_save(inner);
        });
        self.sync_to_engine(engine);
        Ok(self.rules().len())
    }
}

fn remember_save(inner: &mut Inner) {
    if !inner.persist {
        inner.save_error = None;
        return;
    }
    let doc = PersonalDocument {
        version: "1".to_string(),
        customized: true,
        personal_enabled: inner.personal_enabled,
        disabled_builtin_ids: inner.disabled_builtin_ids.clone(),
        rules: inner.rules.clone(),
    };
    match crate::security::encrypted_file::save_encrypted_json(&inner.path, &doc) {
        Ok(()) => inner.save_error = None,
        Err(err) => inner.save_error = Some(err.to_string()),
    }
}

fn fresh_id(existing: &[CmdAuditRule]) -> String {
    loop {
        let compact = uuid::Uuid::new_v4().simple().to_string();
        let id = format!("CA-LOCAL-{}", &compact[..8]);
        if !existing.iter().any(|r| r.id == id) {
            return id;
        }
    }
}
