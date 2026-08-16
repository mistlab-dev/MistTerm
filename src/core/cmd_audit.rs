//! 命令审计：本地快捷提示引擎（策略 + 自定义规则 + 内置模式）+
//! 服务器侧判定结果解析（`MIST_AUDIT` 标记行）。

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
    // 跳过行首空白 / CR
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\r') {
        i += 1;
    }
    let rest = &bytes[i..];
    MIST_AUDIT_PREFIX.starts_with(rest) || rest.starts_with(MIST_AUDIT_PREFIX)
}

fn parse_mist_audit_line(line: &[u8]) -> Option<ServerAuditEvent> {
    let mut i = 0;
    while i < line.len() && matches!(line[i], b' ' | b'\t') {
        i += 1;
    }
    let line = &line[i..];
    if !line.starts_with(MIST_AUDIT_PREFIX) {
        return None;
    }
    let after = &line[MIST_AUDIT_PREFIX.len()..];
    let payload = after
        .strip_prefix(b"\t")
        .or_else(|| after.strip_prefix(b" "))
        .unwrap_or(after);
    let text = std::str::from_utf8(payload).ok()?.trim();
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
}
