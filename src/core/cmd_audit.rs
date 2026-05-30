//! 团队命令审计：本地匹配引擎（策略 + 自定义规则 + 内置模式）。

use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
            dangerous_builtin: load_builtin_file(BASH_DANGEROUS_JSON, "dangerous", false),
            safe_builtin: load_builtin_file(BASH_SAFE_JSON, "safe", true),
            read_dangerous_builtin: load_builtin_file(READ_DANGEROUS_JSON, "read_dangerous", false),
            read_sensitive_builtin: load_builtin_file(READ_SENSITIVE_JSON, "read_sensitive", false),
            read_safe_builtin: load_builtin_file(READ_SAFE_JSON, "read_safe", true),
            last_sync: None,
            sync_interval: Duration::from_secs(300),
        }
    }

    pub fn is_active(&self) -> bool {
        self.global_enabled
            && self
                .policy
                .as_ref()
                .map(|p| p.enabled)
                .unwrap_or(false)
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

        let policy = match self.policy.as_ref() {
            Some(p) if p.enabled => p,
            _ => return allow_result(),
        };

        for rule in &self.rules {
            if rule_matches(rule, cmd) {
                let action = rule.action;
                if action == CmdAuditAction::Allow {
                    return allow_result();
                }
                return audit_match_result(action, "custom", "custom", &rule.id, &rule.description, &rule.name);
            }
        }

        if let Some(b) = self.read_dangerous_builtin.iter().find(|b| builtin_matches(b, cmd)) {
            return audit_match_result(
                policy.dangerous_action,
                "builtin",
                b.level,
                &b.id,
                &b.message,
                &b.message,
            );
        }
        if let Some(b) = self.read_sensitive_builtin.iter().find(|b| builtin_matches(b, cmd)) {
            return audit_match_result(
                policy.sensitive_action,
                "builtin",
                b.level,
                &b.id,
                &b.message,
                &b.message,
            );
        }

        if let Some(b) = self.dangerous_builtin.iter().find(|b| builtin_matches(b, cmd)) {
            return audit_match_result(
                policy.dangerous_action,
                "builtin",
                b.level,
                &b.id,
                &b.message,
                &b.message,
            );
        }

        if self.read_safe_builtin.iter().any(|b| builtin_matches(b, cmd)) {
            return allow_result();
        }

        if self.safe_builtin.iter().any(|b| builtin_matches(b, cmd)) {
            return allow_result();
        }

        let action = policy.unknown_action;
        CmdAuditResult {
            allowed: action != CmdAuditAction::Block && action != CmdAuditAction::Confirm,
            action,
            matches: Vec::new(),
        }
    }
}

fn allow_result() -> CmdAuditResult {
    CmdAuditResult {
        allowed: true,
        action: CmdAuditAction::Allow,
        matches: Vec::new(),
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
