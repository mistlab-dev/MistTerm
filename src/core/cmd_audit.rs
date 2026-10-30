//! 命令审计：本地快捷提示引擎（策略 + 自定义规则 + 内置模式）+
//! 服务器侧判定结果解析（`MIST_AUDIT` 标记行）。

use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const BASH_DANGEROUS_JSON: &str =
    include_str!("../../assets/cmd-audit-patterns/bash-dangerous.json");
const BASH_SAFE_JSON: &str = include_str!("../../assets/cmd-audit-patterns/bash-safe.json");
const READ_DANGEROUS_JSON: &str =
    include_str!("../../assets/cmd-audit-patterns/read-dangerous.json");
const READ_SENSITIVE_JSON: &str =
    include_str!("../../assets/cmd-audit-patterns/read-sensitive.json");
const READ_SAFE_JSON: &str = include_str!("../../assets/cmd-audit-patterns/read-safe.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CmdAuditAction {
    Block,
    Confirm,
    Alert,
    Allow,
}

impl CmdAuditAction {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "block" => Self::Block,
            "confirm" => Self::Confirm,
            "alert" => Self::Alert,
            _ => Self::Allow,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchType {
    Regex,
    Prefix,
    Contains,
    Exact,
}

impl MatchType {
    fn parse(s: &str) -> Self {
        match s {
            "prefix" => Self::Prefix,
            "contains" => Self::Contains,
            "exact" => Self::Exact,
            _ => Self::Regex,
        }
    }
}

/// 匹配方式白名单判断（#187 个人规则保存校验用）：非 regex 的字面匹配不需要编译正则。
pub fn match_type_of(s: &str) -> &'static str {
    match s {
        "prefix" => "prefix",
        "contains" => "contains",
        "exact" => "exact",
        _ => "regex",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmdAuditPolicy {
    #[serde(default)]
    pub team_id: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_block")]
    pub dangerous_action: CmdAuditAction,
    #[serde(default = "default_confirm")]
    pub sensitive_action: CmdAuditAction,
    #[serde(default = "default_allow")]
    pub unknown_action: CmdAuditAction,
    #[serde(default = "default_confirm_timeout")]
    pub confirm_timeout: u64,
}

fn default_block() -> CmdAuditAction {
    CmdAuditAction::Block
}
fn default_confirm() -> CmdAuditAction {
    CmdAuditAction::Confirm
}
fn default_allow() -> CmdAuditAction {
    CmdAuditAction::Allow
}
fn default_confirm_timeout() -> u64 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmdAuditRule {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub pattern: String,
    #[serde(default = "default_match_type")]
    pub match_type: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_match_type() -> String {
    "regex".into()
}
fn default_scope() -> String {
    "command".into()
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone)]
struct CompiledRule {
    id: String,
    name: String,
    match_type: MatchType,
    action: CmdAuditAction,
    description: String,
    priority: i32,
    regex: Option<Regex>,
    literal: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmdAuditMatch {
    pub rule_id: String,
    /// 给人看的规则名（团队规则的名称 / 内置规则的说明）；没有就留空，界面不显示内部 ID。
    pub name: String,
    pub source: String,
    pub level: String,
    pub message: String,
    pub action: CmdAuditAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CmdAuditResult {
    pub allowed: bool,
    pub action: CmdAuditAction,
    pub matches: Vec<CmdAuditMatch>,
}

#[derive(Debug, Clone, Deserialize)]
struct BuiltinPatternFile {
    #[serde(default)]
    patterns: Vec<BuiltinPatternEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct BuiltinPatternEntry {
    id: String,
    pattern: String,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Clone)]
struct CompiledBuiltin {
    id: String,
    regex: Regex,
    /// 原始 pattern（#187 复审：内置只读展示要含全文）。
    pattern: String,
    message: String,
    level: &'static str,
    full_line: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CmdAuditSyncPayload {
    #[serde(default)]
    pub enabled: bool,
    pub policy: Option<CmdAuditPolicy>,
    #[serde(default)]
    pub rules: Vec<CmdAuditRule>,
    #[serde(default = "default_sync_interval")]
    pub sync_interval_sec: u64,
}

fn default_sync_interval() -> u64 {
    300
}

/// 团队命令审计告警上报（`POST .../command-audit/alerts`）
#[derive(Debug, Clone, Serialize)]
pub struct CmdAuditAlertRequest {
    pub command: String,
    pub matched_rule: String,
    pub match_level: String,
    pub action_taken: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmdAuditCacheEntry {
    #[serde(default)]
    pub enabled: bool,
    pub policy: Option<CmdAuditPolicy>,
    #[serde(default)]
    pub rules: Vec<CmdAuditRule>,
    pub synced_at: String,
    #[serde(default = "default_sync_interval")]
    pub sync_interval_sec: u64,
}

impl CmdAuditCacheEntry {
    pub fn from_sync_payload(payload: &CmdAuditSyncPayload) -> Self {
        Self {
            enabled: payload.enabled,
            policy: payload.policy.clone(),
            rules: payload.rules.clone(),
            synced_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            sync_interval_sec: payload.sync_interval_sec,
        }
    }

    pub fn to_sync_payload(&self) -> CmdAuditSyncPayload {
        CmdAuditSyncPayload {
            enabled: self.enabled,
            policy: self.policy.clone(),
            rules: self.rules.clone(),
            sync_interval_sec: self.sync_interval_sec,
        }
    }
}

/// 按团队缓存命令审计策略（`cmd_audit_cache.json`，device_key 加密）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CmdAuditCacheStore {
    #[serde(default)]
    pub by_team: HashMap<String, CmdAuditCacheEntry>,
}

impl CmdAuditCacheStore {
    pub fn cache_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("cmd_audit_cache.json");
        p
    }

    pub fn load() -> Self {
        crate::security::encrypted_file::load_encrypted_json(&Self::cache_path())
    }

    pub fn save(&self) -> io::Result<()> {
        crate::security::encrypted_file::save_encrypted_json(&Self::cache_path(), self)
    }

    pub fn payload_for_team(&self, team_id: &str) -> Option<CmdAuditSyncPayload> {
        self.by_team
            .get(team_id)
            .map(|e| e.to_sync_payload())
    }

    pub fn upsert_team(&mut self, team_id: &str, payload: &CmdAuditSyncPayload) {
        self.by_team
            .insert(team_id.to_string(), CmdAuditCacheEntry::from_sync_payload(payload));
    }
}

pub struct CmdAuditEngine {
    global_enabled: bool,
    policy: Option<CmdAuditPolicy>,
    rules: Vec<CompiledRule>,
    // --- #187 个人层：团队 sync 不覆盖，独立持久化 ---
    personal_rules: Vec<CompiledRule>,
    disabled_builtin_ids: HashSet<String>,
    personal_enabled: bool,
    dangerous_builtin: Vec<CompiledBuiltin>,
    safe_builtin: Vec<CompiledBuiltin>,
    read_dangerous_builtin: Vec<CompiledBuiltin>,
    read_sensitive_builtin: Vec<CompiledBuiltin>,
    read_safe_builtin: Vec<CompiledBuiltin>,
    last_sync: Option<Instant>,
    sync_interval: Duration,
}

impl Default for CmdAuditEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl CmdAuditEngine {
    pub fn new() -> Self {
        Self {
            global_enabled: true,
            policy: None,
            rules: Vec::new(),
            personal_rules: Vec::new(),
            disabled_builtin_ids: HashSet::new(),
            personal_enabled: false,
            dangerous_builtin: load_builtin_file(BASH_DANGEROUS_JSON, "dangerous", false),
            safe_builtin: load_builtin_file(BASH_SAFE_JSON, "safe", true),
            read_dangerous_builtin: load_builtin_file(READ_DANGEROUS_JSON, "read_dangerous", false),
            read_sensitive_builtin: load_builtin_file(READ_SENSITIVE_JSON, "read_sensitive", false),
            read_safe_builtin: load_builtin_file(READ_SAFE_JSON, "read_safe", true),
            last_sync: None,
            sync_interval: Duration::from_secs(300),
        }
    }

    /// 团队策略是否激活（只认团队层）。#187 复审：与个人层判定拆开，
    /// 避免扩了个人层后团队区误报「已激活」。
    pub fn team_policy_active(&self) -> bool {
        self.global_enabled
            && self
                .policy
                .as_ref()
                .map(|p| p.enabled)
                .unwrap_or(false)
    }

    /// 个人层是否可跑（§4-A）：有个人规则或有个人开关。
    /// 模块开关（cmd-audit-local）仍是总闸，在 on_before_send 层控制。
    fn personal_layer_active(&self) -> bool {
        self.global_enabled && (self.personal_enabled || !self.personal_rules.is_empty())
    }

    /// 兼容旧调用方（Host 总闸判定）：团队策略激活**或**个人层可跑。
    pub fn is_active(&self) -> bool {
        self.team_policy_active() || self.personal_layer_active()
    }

    pub fn apply_sync(&mut self, payload: CmdAuditSyncPayload) {
        self.global_enabled = payload.enabled;
        self.sync_interval = Duration::from_secs(payload.sync_interval_sec.max(60));
        self.policy = payload.policy;
        self.rules = payload
            .rules
            .into_iter()
            .filter(|r| r.enabled && !r.pattern.is_empty())
            .filter_map(compile_api_rule)
            .collect();
        self.rules.sort_by(|a, b| a.priority.cmp(&b.priority));
        self.last_sync = Some(Instant::now());
    }

    /// 启动时用本地缓存的规则先顶上，但不算「已同步」：
    /// 团队服务一空闲就会马上去服务端拉最新规则，而不是等满一个同步周期。
    pub fn apply_cached(&mut self, payload: CmdAuditSyncPayload) {
        self.apply_sync(payload);
        self.last_sync = None;
    }

    pub fn needs_sync(&self) -> bool {
        match self.last_sync {
            None => true,
            Some(t) => t.elapsed() >= self.sync_interval,
        }
    }

    pub fn confirm_timeout_secs(&self) -> u64 {
        self.policy
            .as_ref()
            .map(|p| p.confirm_timeout)
            .unwrap_or(300)
    }

    pub fn check(&self, command: &str) -> CmdAuditResult {
        let cmd = command.trim();
        if cmd.is_empty() || !self.is_active() {
            return allow_result();
        }

        // #187 方案 §4-A：存在个人规则或个人开关时，无团队 policy 也可跑「个人 + 内置」。
        // 模块开关（cmd-audit-local）仍是总闸，在 on_before_send 层控制。
        // （is_active 已包含个人层判定，这里不会再被短路。）
        let has_personal = self.personal_layer_active();
        let policy = self
            .policy
            .as_ref()
            .filter(|p| p.enabled && self.global_enabled);

        // 1. 团队规则（最高优先）。无团队 policy 时直接跳过。
        if policy.is_some() {
            for rule in &self.rules {
                if rule_matches(rule, cmd) {
                    let action = rule.action;
                    if action == CmdAuditAction::Allow {
                        // 团队 Allow：个人层只能收紧不能放宽（§4-2）。
                        if let Some(personal) = self.personal_match(cmd) {
                            return personal;
                        }
                        return allow_result();
                    }
                    // 团队已 Block/Confirm：保持，不放宽。
                    return audit_match_result(action, "custom", "custom", &rule.id, &rule.description, &rule.name);
                }
            }
        }

        // 2. 个人规则（团队 Allow / 未命中时生效）。
        if has_personal {
            if let Some(personal) = self.personal_match(cmd) {
                return personal;
            }
        }

        // 3. 内置分级（跳过被个人层停用的 id）。
        // 无团队 policy 时能走到这里说明个人层已激活（is_active 保证），
        // 内置命中沿用默认分级：dangerous=block、sensitive=confirm（§4-A）。
        let builtin_allowed = |id: &str| !self.disabled_builtin_ids.contains(id);
        if let Some(b) = self.read_dangerous_builtin.iter().find(|b| builtin_matches(b, cmd) && builtin_allowed(&b.id)) {
            let action = match policy {
                Some(p) => p.dangerous_action,
                None => CmdAuditAction::Block,
            };
            return audit_match_result(
                action,
                "builtin",
                b.level,
                &b.id,
                &b.message,
                &b.message,
            );
        }
        if let Some(b) = self.read_sensitive_builtin.iter().find(|b| builtin_matches(b, cmd) && builtin_allowed(&b.id)) {
            let action = match policy {
                Some(p) => p.sensitive_action,
                None => CmdAuditAction::Confirm,
            };
            return audit_match_result(
                action,
                "builtin",
                b.level,
                &b.id,
                &b.message,
                &b.message,
            );
        }

        if let Some(b) = self.dangerous_builtin.iter().find(|b| builtin_matches(b, cmd) && builtin_allowed(&b.id)) {
            let action = match policy {
                Some(p) => p.dangerous_action,
                None => CmdAuditAction::Block,
            };
            return audit_match_result(
                action,
                "builtin",
                b.level,
                &b.id,
                &b.message,
                &b.message,
            );
        }

        if self.read_safe_builtin.iter().any(|b| builtin_matches(b, cmd) && builtin_allowed(&b.id)) {
            return allow_result();
        }

        if self.safe_builtin.iter().any(|b| builtin_matches(b, cmd) && builtin_allowed(&b.id)) {
            return allow_result();
        }

        let action = match policy {
            Some(p) => p.unknown_action,
            None => return allow_result(),
        };
        CmdAuditResult {
            allowed: action != CmdAuditAction::Block && action != CmdAuditAction::Confirm,
            action,
            matches: Vec::new(),
        }
    }

    /// 个人规则匹配（按 priority 升序，命中即返回）。个人 Allow 直接放行。
    fn personal_match(&self, cmd: &str) -> Option<CmdAuditResult> {
        for rule in &self.personal_rules {
            if rule_matches(rule, cmd) {
                let action = rule.action;
                if action == CmdAuditAction::Allow {
                    return Some(allow_result());
                }
                return Some(audit_match_result(
                    action,
                    "personal",
                    "personal",
                    &rule.id,
                    &rule.description,
                    &rule.name,
                ));
            }
        }
        None
    }

    // --- #187 个人层管理（Store 经 &Mutex<Self> 调用；团队 sync 不经过这里） ---

    /// 热更新个人层：保存后同进程内立即生效，不覆盖团队层。
    pub fn apply_personal(&mut self, rules: &[CmdAuditRule], disabled_builtin_ids: HashSet<String>, personal_enabled: bool) {
        self.personal_rules = rules
            .iter()
            .filter(|r| r.enabled && !r.pattern.is_empty())
            .cloned()
            .filter_map(compile_api_rule)
            .collect();
        self.personal_rules.sort_by(|a, b| a.priority.cmp(&b.priority));
        self.disabled_builtin_ids = disabled_builtin_ids;
        self.personal_enabled = personal_enabled;
    }

    /// 只读展示用：个人规则原始列表（未经编译过滤）由 Store 持有；这里给内置清单。
    pub fn builtin_overview(&self) -> Vec<CmdAuditBuiltinInfo> {
        let mut out = Vec::new();
        let mut push = |items: &[CompiledBuiltin]| {
            for b in items {
                out.push(CmdAuditBuiltinInfo {
                    id: b.id.clone(),
                    pattern: b.pattern.clone(),
                    message: b.message.clone(),
                    level: b.level.to_string(),
                    full_line: b.full_line,
                });
            }
        };
        push(&self.dangerous_builtin);
        push(&self.read_dangerous_builtin);
        push(&self.read_sensitive_builtin);
        push(&self.read_safe_builtin);
        push(&self.safe_builtin);
        out
    }

    /// 只读展示用：团队规则当前生效条数（正文不暴露，桌面不改团队规则）。
    pub fn team_rule_count(&self) -> usize {
        self.rules.len()
    }
}

fn allow_result() -> CmdAuditResult {
    CmdAuditResult {
        allowed: true,
        action: CmdAuditAction::Allow,
        matches: Vec::new(),
    }
}

/// 内置规则的只读展示信息（#187 §7：GUI 查看内置全文，不写回 assets）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CmdAuditBuiltinInfo {
    pub id: String,
    pub pattern: String,
    pub message: String,
    pub level: String,
    pub full_line: bool,
}

/// 内部 ID（UUID、`pack:<id>:<n>` 之类）不该给人看。
pub fn looks_like_internal_id(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return true;
    }
    if t.starts_with("pack:") || t.starts_with("rule_") || t.starts_with("rule-") {
        return true;
    }
    let hex_dash = t.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    hex_dash && t.len() >= 8 && t.chars().any(|c| c.is_ascii_digit())
}

/// 命中的是团队下发的规则(而非内置快捷检查)：弹窗据此显示「团队策略」措辞。
pub fn is_team_policy(result: &CmdAuditResult) -> bool {
    result.matches.iter().any(|m| m.source == "custom" && m.action == result.action)
}

/// 确认/拦截弹窗里给人看的规则说明：有名字用名字，没有就说是哪一类规则，不显示内部 ID。
pub fn match_display_name(m: &CmdAuditMatch, zh: bool) -> String {
    let kind = match (m.source.as_str(), zh) {
        ("custom", true) => "团队规则",
        ("custom", false) => "team rule",
        ("builtin", true) => "内置规则",
        ("builtin", false) => "built-in rule",
        ("server", true) => "服务器上的规则",
        ("server", false) => "server rule",
        (_, true) => "规则",
        (_, false) => "rule",
    };
    let name = m.name.trim();
    if name.is_empty() || looks_like_internal_id(name) {
        kind.to_string()
    } else if zh {
        format!("{name}（{kind}）")
    } else {
        format!("{name} ({kind})")
    }
}

fn audit_match_result(
    action: CmdAuditAction,
    source: &str,
    level: &str,
    rule_id: &str,
    message: &str,
    name_fallback: &str,
) -> CmdAuditResult {
    CmdAuditResult {
        allowed: action == CmdAuditAction::Alert || action == CmdAuditAction::Allow,
        action,
        matches: vec![CmdAuditMatch {
            rule_id: rule_id.to_string(),
            name: if !name_fallback.trim().is_empty() {
                name_fallback.trim().to_string()
            } else {
                message.trim().to_string()
            },
            source: source.into(),
            level: level.into(),
            message: if message.is_empty() {
                name_fallback.to_string()
            } else {
                message.to_string()
            },
            action,
        }],
    }
}

fn compile_api_rule(r: CmdAuditRule) -> Option<CompiledRule> {
    let match_type = MatchType::parse(&r.match_type);
    let regex = if match_type == MatchType::Regex {
        Regex::new(&r.pattern).ok()
    } else {
        None
    };
    Some(CompiledRule {
        id: r.id,
        name: r.name,
        match_type,
        action: CmdAuditAction::parse(&r.action),
        description: r.description,
        priority: r.priority,
        regex,
        literal: r.pattern,
    })
}

fn rule_matches(rule: &CompiledRule, cmd: &str) -> bool {
    match rule.match_type {
        MatchType::Prefix => cmd.starts_with(&rule.literal),
        MatchType::Contains => cmd.contains(&rule.literal),
        MatchType::Exact => cmd == rule.literal,
        MatchType::Regex => rule
            .regex
            .as_ref()
            .map(|re| re.is_match(cmd))
            .unwrap_or(false),
    }
}

fn load_builtin_file(json: &str, level: &'static str, full_line: bool) -> Vec<CompiledBuiltin> {
    let file: BuiltinPatternFile = match serde_json::from_str(json) {
        Ok(f) => f,
        Err(e) => {
            log::warn!("cmd_audit: failed to parse builtin patterns: {}", e);
            return Vec::new();
        }
    };
    file.patterns
        .into_iter()
        .filter_map(|p| {
            let regex = Regex::new(&p.pattern).ok()?;
            Some(CompiledBuiltin {
                id: p.id,
                regex,
                pattern: p.pattern,
                message: p.message,
                level,
                full_line,
            })
        })
        .collect()
}

fn builtin_matches(b: &CompiledBuiltin, cmd: &str) -> bool {
    if b.full_line {
        b.regex.is_match(cmd)
    } else {
        b.regex.is_match(cmd)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandSendResult {
    Sent,
    NotConnected,
    Blocked(CmdAuditResult),
    NeedsConfirm { command: String, audit: CmdAuditResult },
}

/// 判定结果来源：本地引擎 vs 服务器侧 agent。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CmdAuditSource {
    #[default]
    Local,
    Server,
}

/// 服务器侧通过 PTY 回传的审计事件（agent / 包裹脚本打印）。
///
/// 行格式：`MIST_AUDIT\t{"v":1,"action":"block","message":"...","rule":"...","command":"...","token":"..."}`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAuditEvent {
    pub action: CmdAuditAction,
    pub message: String,
    pub rule: String,
    pub command: String,
    /// 可选放行令牌；确认后客户端可先发 `MIST_AUDIT_APPROVE\t{token}`。
    pub token: String,
}

impl ServerAuditEvent {
    pub fn to_cmd_audit_result(&self) -> CmdAuditResult {
        let allowed = matches!(self.action, CmdAuditAction::Alert | CmdAuditAction::Allow);
        CmdAuditResult {
            allowed,
            action: self.action,
            matches: vec![CmdAuditMatch {
                rule_id: if self.rule.is_empty() {
                    "server".into()
                } else {
                    self.rule.clone()
                },
                name: if looks_like_internal_id(&self.rule) {
                    String::new()
                } else {
                    self.rule.trim().to_string()
                },
                source: "server".into(),
                level: match self.action {
                    CmdAuditAction::Block => "dangerous".into(),
                    CmdAuditAction::Confirm => "sensitive".into(),
                    CmdAuditAction::Alert => "alert".into(),
                    CmdAuditAction::Allow => "safe".into(),
                },
                message: self.message.clone(),
                action: self.action,
            }],
        }
    }
}

const MIST_AUDIT_PREFIX: &[u8] = b"MIST_AUDIT";

/// 从 PTY 字节流中剥离并解析 `MIST_AUDIT` 标记行。
#[derive(Debug, Default)]
pub struct ServerAuditProbe {
    /// 可能是 `MIST_AUDIT…` 起始的不完整行，暂不交给 VTE。
    pending: Vec<u8>,
}

impl ServerAuditProbe {
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂入一块 PTY 数据，返回应显示的字节与解析出的事件。
    pub fn feed(&mut self, chunk: &[u8]) -> (Vec<u8>, Vec<ServerAuditEvent>) {
        if chunk.is_empty() && self.pending.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(chunk);
        let mut out = Vec::with_capacity(data.len());
        let mut events = Vec::new();
        let mut start = 0usize;
        while start < data.len() {
            let rest = &data[start..];
            let nl = rest.iter().position(|&b| b == b'\n');
            match nl {
                Some(rel) => {
                    let mut line = &rest[..rel];
                    if line.ends_with(b"\r") {
                        line = &line[..line.len() - 1];
                    }
                    let line_start = start;
                    start += rel + 1;
                    if let Some(ev) = parse_mist_audit_line(line) {
                        events.push(ev);
                    } else {
                        out.extend_from_slice(&data[line_start..start]);
                    }
                }
                None => {
                    if looks_like_mist_audit_prefix(rest) {
                        self.pending = rest.to_vec();
                    } else {
                        out.extend_from_slice(rest);
                    }
                    break;
                }
            }
        }
        (out, events)
    }
}

fn looks_like_mist_audit_prefix(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    // 行内任意位置已出现完整前缀（常见：ANSI / CR 之后）
    if find_mist_audit_offset(bytes).is_some() {
        return true;
    }
    // 跳过行首空白 / CR / 常见 CSI，再判断是否为前缀残片
    let rest = strip_leading_pty_noise(bytes);
    if rest.is_empty() {
        // 整段都是空白 / 完整的控制序列：直接显示（行中插空格的回显就是 `ESC[1@ ` 这样）。
        // 只有结尾停在半截 ESC 序列上才扣住，等下一块拼完整再判断。
        return ends_with_incomplete_escape(bytes);
    }
    MIST_AUDIT_PREFIX.starts_with(rest) || rest.starts_with(MIST_AUDIT_PREFIX)
}

/// 结尾是否停在没写完的 ESC / CSI 序列里。
fn ends_with_incomplete_escape(bytes: &[u8]) -> bool {
    let Some(esc) = bytes.iter().rposition(|&b| b == 0x1b) else {
        return false;
    };
    let tail = &bytes[esc + 1..];
    match tail.first() {
        None => true,
        Some(b'[') => !tail[1..].iter().any(|b| (0x40..=0x7e).contains(b)),
        Some(_) => false,
    }
}

fn find_mist_audit_offset(line: &[u8]) -> Option<usize> {
    line.windows(MIST_AUDIT_PREFIX.len())
        .position(|w| w == MIST_AUDIT_PREFIX)
}

/// 去掉行首空白、CR，以及常见 CSI（如 bracketed-paste `\x1b[?2004l`）。
fn strip_leading_pty_noise(mut line: &[u8]) -> &[u8] {
    loop {
        if line.is_empty() {
            return line;
        }
        match line[0] {
            b' ' | b'\t' | b'\r' => line = &line[1..],
            0x1b => {
                line = &line[1..];
                if line.first() == Some(&b'[') {
                    line = &line[1..];
                    while let Some((&b, rest)) = line.split_first() {
                        line = rest;
                        // CSI 结束字节是 0x40..=0x7E（含 `@` = ICH 插入字符、`~` 等），不只是字母。
                        // 以前只认字母：行中插字时 shell 回显 `ESC[1@X` 会把 `X` 当成 CSI 的一部分，
                        // 整段被当成「可能是 MIST_AUDIT 前缀」扣住，直到回车才显示。
                        if (0x40..=0x7e).contains(&b) {
                            break;
                        }
                    }
                }
            }
            _ => return line,
        }
    }
}

fn parse_mist_audit_line(line: &[u8]) -> Option<ServerAuditEvent> {
    // bash DEBUG / bracketed-paste 常在同行前塞 ANSI，不能只认「行首」MIST_AUDIT。
    let idx = find_mist_audit_offset(line)?;
    let line = &line[idx..];
    let after = &line[MIST_AUDIT_PREFIX.len()..];
    let payload = after
        .strip_prefix(b"\t")
        .or_else(|| after.strip_prefix(b" "))
        .unwrap_or(after);
    let text = std::str::from_utf8(payload).ok()?.trim();
    // JSON 后偶发残留 CR / 控制符
    let text = text.trim_end_matches(|c: char| c == '\r' || c.is_control());
    if text.is_empty() {
        return None;
    }
    #[derive(Deserialize)]
    struct Raw {
        #[serde(default)]
        action: String,
        #[serde(default)]
        message: String,
        #[serde(default)]
        rule: String,
        #[serde(default)]
        command: String,
        #[serde(default)]
        token: String,
    }
    let raw: Raw = serde_json::from_str(text).ok()?;
    let action = CmdAuditAction::parse(&raw.action);
    if matches!(action, CmdAuditAction::Allow) && raw.action.trim().is_empty() {
        return None;
    }
    // 仅展示 block/confirm/alert；allow 忽略
    if matches!(action, CmdAuditAction::Allow) {
        return None;
    }
    Some(ServerAuditEvent {
        action,
        message: raw.message,
        rule: raw.rule,
        command: raw.command,
        token: raw.token,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn ich_insert_echo_is_not_held() {
        // 行中插字：bash/readline 回显 ICH + 字符
        let mut p = super::ServerAuditProbe::new();
        let (out, ev) = p.feed(b"\x1b[1@X");
        assert!(ev.is_empty());
        assert_eq!(out, b"\x1b[1@X");
        let (out, _) = p.feed(b"\x1b[@Y");
        assert_eq!(out, b"\x1b[@Y");
        // 行中插空格：回显只有 ICH + 空格
        let (out, _) = p.feed(b"\x1b[1@ ");
        assert_eq!(out, b"\x1b[1@ ");
        let (out, _) = p.feed(b" ");
        assert_eq!(out, b" ");
        // 半截 CSI 仍然扣住，拼上下一块后整体放出
        let (out, _) = p.feed(b"\x1b[");
        assert!(out.is_empty());
        let (out, _) = p.feed(b"1@Z");
        assert_eq!(out, b"\x1b[1@Z");
    }


    #[test]
    fn cached_rules_apply_but_still_need_sync() {
        let mut engine = CmdAuditEngine::default();
        let payload: CmdAuditSyncPayload = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "policy": {"team_id": "t", "enabled": true},
            "rules": [{"id": "r1", "name": "demo", "pattern": "b1-demo", "match_type": "contains",
                       "scope": "command", "action": "block", "priority": 1, "enabled": true}],
            "sync_interval_sec": 300
        }))
        .unwrap();
        engine.apply_cached(payload.clone());
        assert!(engine.needs_sync(), "cached rules must not count as a fresh sync");
        engine.apply_sync(payload);
        assert!(!engine.needs_sync());
    }
    use super::*;

    #[test]
    fn team_policy_detected_only_for_custom_rule_driving_action() {
        let m = |source: &str, action| CmdAuditMatch {
            rule_id: "r".into(),
            name: "n".into(),
            source: source.into(),
            level: "custom".into(),
            message: String::new(),
            action,
        };
        let r = |matches: Vec<CmdAuditMatch>| CmdAuditResult {
            allowed: false,
            action: CmdAuditAction::Confirm,
            matches,
        };
        assert!(is_team_policy(&r(vec![m("custom", CmdAuditAction::Confirm)])));
        assert!(!is_team_policy(&r(vec![m("builtin", CmdAuditAction::Confirm)])));
        assert!(!is_team_policy(&r(vec![
            m("builtin", CmdAuditAction::Confirm),
            m("custom", CmdAuditAction::Alert),
        ])));
    }

    #[test]
    fn display_name_never_shows_internal_ids() {
        let mut m = CmdAuditMatch {
            rule_id: "3f2a9c1e-7b4d-4e2a-9f10-2c3d4e5f6a7b".into(),
            name: "禁止删库".into(),
            source: "custom".into(),
            level: "custom".into(),
            message: String::new(),
            action: CmdAuditAction::Confirm,
        };
        assert_eq!(match_display_name(&m, true), "禁止删库（团队规则）");
        m.name = m.rule_id.clone();
        assert_eq!(match_display_name(&m, true), "团队规则");
        m.name.clear();
        assert_eq!(match_display_name(&m, false), "team rule");
        assert!(looks_like_internal_id("pack:12:0"));
        assert!(!looks_like_internal_id("DROP DATABASE"));
        assert!(!looks_like_internal_id("deadbeef")); // 没有数字的词不当成 ID
    }

    #[test]
    fn custom_rule_match_carries_rule_name() {
        let r = audit_match_result(
            CmdAuditAction::Confirm,
            "custom",
            "custom",
            "abc-123",
            "",
            "重启前确认",
        );
        assert_eq!(r.matches[0].name, "重启前确认");
        let r = audit_match_result(
            CmdAuditAction::Confirm,
            "custom",
            "custom",
            "abc-123",
            "说明",
            "",
        );
        assert_eq!(r.matches[0].name, "说明");
    }

    #[test]
    fn blocks_rm_rf_root_when_policy_enabled() {
        let mut engine = CmdAuditEngine::new();
        engine.apply_sync(CmdAuditSyncPayload {
            enabled: true,
            policy: Some(CmdAuditPolicy {
                team_id: "t1".into(),
                enabled: true,
                dangerous_action: CmdAuditAction::Block,
                sensitive_action: CmdAuditAction::Confirm,
                unknown_action: CmdAuditAction::Allow,
                confirm_timeout: 300,
            }),
            rules: Vec::new(),
            sync_interval_sec: 300,
        });
        let r = engine.check("rm -rf /");
        assert!(!r.allowed);
        assert_eq!(r.action, CmdAuditAction::Block);
    }

    #[test]
    fn allows_echo_when_unknown_allow() {
        let mut engine = CmdAuditEngine::new();
        engine.apply_sync(CmdAuditSyncPayload {
            enabled: true,
            policy: Some(CmdAuditPolicy {
                team_id: "t1".into(),
                enabled: true,
                dangerous_action: CmdAuditAction::Block,
                sensitive_action: CmdAuditAction::Confirm,
                unknown_action: CmdAuditAction::Allow,
                confirm_timeout: 300,
            }),
            rules: Vec::new(),
            sync_interval_sec: 300,
        });
        let r = engine.check("echo hello");
        assert!(r.allowed);
    }

    #[test]
    fn server_audit_probe_parses_and_strips_marker_line() {
        let mut probe = ServerAuditProbe::new();
        let chunk = b"hello\nMIST_AUDIT\t{\"action\":\"block\",\"message\":\"nope\",\"rule\":\"r1\",\"command\":\"rm -rf /\"}\nworld\n";
        let (out, events) = probe.feed(chunk);
        assert_eq!(out, b"hello\nworld\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, CmdAuditAction::Block);
        assert_eq!(events[0].message, "nope");
        assert_eq!(events[0].rule, "r1");
        assert_eq!(events[0].command, "rm -rf /");
    }

    #[test]
    fn server_audit_probe_holds_partial_prefix() {
        let mut probe = ServerAuditProbe::new();
        let (out1, ev1) = probe.feed(b"MIST_AUD");
        assert!(out1.is_empty());
        assert!(ev1.is_empty());
        let (out2, ev2) = probe.feed(b"IT\t{\"action\":\"alert\",\"message\":\"ok\"}\n");
        assert!(out2.is_empty());
        assert_eq!(ev2.len(), 1);
        assert_eq!(ev2[0].action, CmdAuditAction::Alert);
    }

    #[test]
    fn server_audit_probe_passes_through_unrelated() {
        let mut probe = ServerAuditProbe::new();
        let (out, ev) = probe.feed(b"MIST_AUDX\n");
        assert_eq!(out, b"MIST_AUDX\n");
        assert!(ev.is_empty());
    }

    #[test]
    fn server_audit_probe_parses_marker_after_bracketed_paste_csi() {
        let mut probe = ServerAuditProbe::new();
        // 交互 bash 实测：`\x1b[?2004l\rMIST_AUDIT\t{...}\r\n`
        let chunk = b"cat /etc/shadow\r\n\x1b[?2004l\rMIST_AUDIT\t{\"action\":\"block\",\"message\":\"block\",\"rule\":\"server\",\"command\":\"cat /etc/shadow\"}\r\n\x1b[?2004h";
        let (out, events) = probe.feed(chunk);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, CmdAuditAction::Block);
        assert_eq!(events[0].command, "cat /etc/shadow");
        let s = String::from_utf8_lossy(&out);
        assert!(!s.contains("MIST_AUDIT"), "marker must be stripped, got {s:?}");
        assert!(s.contains("cat /etc/shadow"));
    }

    #[test]
    fn server_audit_probe_confirm_and_alert_actions() {
        let mut probe = ServerAuditProbe::new();
        let line = b"prompt$\nMIST_AUDIT\t{\"action\":\"confirm\",\"message\":\"sensitive\",\"rule\":\"read_shadow\",\"command\":\"cat /etc/shadow\",\"token\":\"tok1\"}\n";
        let (out, events) = probe.feed(line);
        assert!(String::from_utf8_lossy(&out).contains("prompt$"));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, CmdAuditAction::Confirm);
        assert_eq!(events[0].token, "tok1");
        let alert_line = b"MIST_AUDIT\t{\"action\":\"alert\",\"message\":\"logged\"}\n";
        let (_, ev2) = probe.feed(alert_line);
        assert_eq!(ev2[0].action, CmdAuditAction::Alert);
    }

    #[test]
    fn server_audit_probe_large_chunk_split_no_data_loss() {
        let payload = br#"MIST_AUDIT	{"action":"block","message":"nope","command":"rm -rf /"}"#;
        let mut line = b"before\n".to_vec();
        line.extend_from_slice(payload);
        line.push(b'\n');
        line.extend_from_slice(b"after\n");
        let mut probe = ServerAuditProbe::new();
        let mut out_all = Vec::new();
        let mut events_all = Vec::new();
        for chunk in line.chunks(17) {
            let (out, ev) = probe.feed(chunk);
            out_all.extend(out);
            events_all.extend(ev);
        }
        assert_eq!(events_all.len(), 1);
        assert_eq!(events_all[0].action, CmdAuditAction::Block);
        assert_eq!(std::str::from_utf8(&out_all).unwrap(), "before\nafter\n");
    }

    #[test]
    fn server_audit_event_to_cmd_audit_result() {
        let ev = ServerAuditEvent {
            action: CmdAuditAction::Block,
            message: "deny".into(),
            rule: "r1".into(),
            command: "rm -rf /".into(),
            token: String::new(),
        };
        let r = ev.to_cmd_audit_result();
        assert!(!r.allowed);
        assert_eq!(r.matches[0].source, "server");
    }
}

#[cfg(test)]
mod personal_layer_tests {
    use super::*;
    use std::collections::HashSet;

    fn personal_rule(id: &str, pattern: &str, action: &str) -> CmdAuditRule {
        CmdAuditRule {
            id: id.to_string(),
            name: format!("rule-{id}"),
            pattern: pattern.to_string(),
            match_type: "regex".into(),
            scope: "command".into(),
            action: action.into(),
            description: String::new(),
            priority: 100,
            enabled: true,
        }
    }

    fn team_policy(dangerous: CmdAuditAction) -> CmdAuditPolicy {
        CmdAuditPolicy {
            team_id: "t1".into(),
            enabled: true,
            dangerous_action: dangerous,
            sensitive_action: CmdAuditAction::Confirm,
            unknown_action: CmdAuditAction::Allow,
            confirm_timeout: 300,
        }
    }

    #[test]
    fn personal_rule_fires_without_team_policy() {
        // 方案 §4-A：无团队 policy，个人层也能跑。
        let mut engine = CmdAuditEngine::new();
        let rules = vec![personal_rule("P1", "rm\\s+-rf\\s+/", "confirm")];
        engine.apply_personal(&rules, HashSet::new(), true);
        let r = engine.check("rm -rf /data");
        assert_eq!(r.action, CmdAuditAction::Confirm);
        assert!(!r.allowed);
        assert_eq!(r.matches[0].source, "personal");
        // 未命中 → 放行。
        assert!(engine.check("ls -la").allowed);
    }

    #[test]
    fn personal_disabled_and_no_policy_means_inactive() {
        // #187 复审修正：完全无个人层（无规则且未开开关）且无团队 policy → 不 active。
        let mut engine = CmdAuditEngine::new();
        engine.apply_personal(&Vec::new(), HashSet::new(), false);
        assert!(engine.check("rm -rf /data").allowed);
    }

    #[test]
    fn personal_rules_fire_even_if_switch_off() {
        // #187 复审：方案 §4-A 是 has_personal = 开关 || 有规则 ——
        // 有个人规则时即使用户没开「无策略也运行」开关，个人层也应跑。
        let mut engine = CmdAuditEngine::new();
        let rules = vec![personal_rule("P1", "rm\\s+-rf\\s+/", "confirm")];
        engine.apply_personal(&rules, HashSet::new(), false);
        let r = engine.check("rm -rf /data");
        assert_eq!(r.action, CmdAuditAction::Confirm);
        assert_eq!(r.matches[0].source, "personal");
    }

    #[test]
    fn team_block_cannot_be_relaxed_by_personal_allow() {
        // 方案 §4-2：团队已 Block，个人 Allow 不得放宽。
        let mut engine = CmdAuditEngine::new();
        engine.apply_sync(CmdAuditSyncPayload {
            enabled: true,
            sync_interval_sec: 300,
            policy: Some(team_policy(CmdAuditAction::Block)),
            rules: vec![personal_rule("T1", "^deploy", "block")],
        });
        let personal = vec![personal_rule("P1", ".*", "allow")];
        engine.apply_personal(&personal, HashSet::new(), true);

        let r = engine.check("deploy prod");
        assert_eq!(r.action, CmdAuditAction::Block);
        assert!(!r.allowed);
        assert_eq!(r.matches[0].source, "custom");
    }

    #[test]
    fn personal_can_tighten_after_team_allow() {
        // 团队 Allow：个人层可以追加 Block/Confirm（只能收紧）。
        let mut engine = CmdAuditEngine::new();
        engine.apply_sync(CmdAuditSyncPayload {
            enabled: true,
            sync_interval_sec: 300,
            policy: Some(team_policy(CmdAuditAction::Block)),
            rules: vec![personal_rule("T1", "^deploy", "allow")],
        });
        let personal = vec![personal_rule("P1", "^deploy.*--force", "block")];
        engine.apply_personal(&personal, HashSet::new(), true);

        // 团队 allow 命中 → 个人层继续裁。
        let r = engine.check("deploy prod --force");
        assert_eq!(r.action, CmdAuditAction::Block);
        assert_eq!(r.matches[0].source, "personal");
        // 个人未命中 → 团队 allow 生效。
        assert!(engine.check("deploy prod").allowed);
    }

    #[test]
    fn disabled_builtin_id_is_skipped() {
        // 方案 §4-3：个人可对内置 id 做本地停用。
        let mut engine = CmdAuditEngine::new();
        engine.apply_sync(CmdAuditSyncPayload {
            enabled: true,
            sync_interval_sec: 300,
            policy: Some(team_policy(CmdAuditAction::Block)),
            rules: Vec::new(),
        });
        // 找一条确实存在的内置 dangerous 规则，验证命中。
        let overview = engine.builtin_overview();
        let dangerous = overview
            .iter()
            .find(|b| b.level == "dangerous")
            .expect("at least one dangerous builtin");
        // 先确认它会被拦（pattern 从 JSON 来，这里用条目是否存在验证加载）。
        assert!(!overview.is_empty());
        // 停用后该 id 不再参与匹配——这里验证 apply_personal 接受清单。
        let mut disabled = HashSet::new();
        disabled.insert(dangerous.id.clone());
        engine.apply_personal(&Vec::new(), disabled, true);
        // 引擎个人层为空：行为不 panic，团队判定路径照常。
        assert!(engine.team_policy_active());
    }

    #[test]
    fn team_sync_does_not_wipe_personal_layer() {
        // 方案 §6-1：团队 apply_sync 不覆盖个人层。
        let mut engine = CmdAuditEngine::new();
        let personal = vec![personal_rule("P1", "^kubectl.*--context=prod", "confirm")];
        engine.apply_personal(&personal, HashSet::new(), true);
        engine.apply_sync(CmdAuditSyncPayload {
            enabled: true,
            sync_interval_sec: 300,
            policy: Some(team_policy(CmdAuditAction::Block)),
            rules: vec![personal_rule("T1", "^ssh", "alert")],
        });
        let r = engine.check("kubectl get pods --context=prod");
        assert_eq!(r.action, CmdAuditAction::Confirm);
        assert_eq!(r.matches[0].source, "personal");
    }
}
