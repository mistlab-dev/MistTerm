//! 结构化安全审计（JSONL），与 [`crate::core::session_logger`] 终端回放分离。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use rand::Rng;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
const HTTP_QUEUE_CAP: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum AuditCategory {
    Auth,
    Session,
    Credential,
    Vault,
    Config,
    Fragment,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Success,
    Failure,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditActor {
    pub os_user: String,
    pub hostname: String,
    pub app_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub ts: String,
    pub event_id: String,
    pub actor: AuditActor,
    pub category: AuditCategory,
    pub action: String,
    pub outcome: AuditOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(default)]
    pub detail: Value,
}

impl AuditEvent {
    pub fn new(category: AuditCategory, action: impl Into<String>, outcome: AuditOutcome) -> Self {
        Self {
            ts: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            event_id: new_audit_event_id(),
            actor: current_actor(),
            category,
            action: action.into(),
            outcome,
            session_id: None,
            host: None,
            resource: None,
            detail: Value::Null,
        }
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    pub fn with_resource(mut self, resource: impl Into<String>) -> Self {
        self.resource = Some(resource.into());
        self
    }

    pub fn with_detail(mut self, detail: Value) -> Self {
        self.detail = detail;
        self
    }
}

fn current_actor() -> AuditActor {
    AuditActor {
        os_user: std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".into()),
        hostname: std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .unwrap_or_else(|_| "unknown".into()),
        app_version: APP_VERSION.to_string(),
    }
}

pub fn default_audit_dir() -> PathBuf {
    let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("mistterm");
    p.push("audit");
    p
}

fn pending_team_events_path() -> PathBuf {
    default_audit_dir().join("pending-team-events.jsonl")
}

/// 后台线程或 token 刷新等无 `AuditLogger` 时写入审计。
pub fn record_audit_blocking(event: AuditEvent) {
    let cfg = crate::core::AppSettings::load().audit;
    if !cfg.enabled {
        return;
    }
    let _ = write_event_file(&cfg, &event);
    if cfg.http.enabled && !cfg.http.url.is_empty() {
        let _ = append_pending_team_events(&[event]);
    }
}

fn append_pending_team_events(events: &[AuditEvent]) -> std::io::Result<()> {
    let path = pending_team_events_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    for ev in events {
        let line = serde_json::to_string(ev)? + "\n";
        f.write_all(line.as_bytes())?;
    }
    f.sync_data()?;
    Ok(())
}

fn load_pending_team_events() -> Vec<AuditEvent> {
    let path = pending_team_events_path();
    if !path.exists() {
        return Vec::new();
    }
    let Ok(raw) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Ok(ev) = serde_json::from_str::<AuditEvent>(t) {
            out.push(ev);
        }
    }
    out
}

fn clear_pending_team_events_file() {
    let path = pending_team_events_path();
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpSinkSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub bearer_token: String,
    /// 团队审计上报时附带 `team_id`（`POST /v1/audit/events`）
    #[serde(default)]
    pub team_id: String,
    #[serde(default = "default_http_batch")]
    pub batch_size: usize,
    #[serde(default = "default_http_interval_ms")]
    pub flush_interval_ms: u64,
}

fn default_http_batch() -> usize {
    50
}
fn default_http_interval_ms() -> u64 {
    30_000
}

/// `evt_{unix_ms}_{hex}`，便于服务端按时间窗去重。
fn new_audit_event_id() -> String {
    let ts = Utc::now().timestamp_millis();
    let r: u32 = rand::thread_rng().gen();
    format!("evt_{ts}_{r:08x}")
}

impl Default for HttpSinkSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            url: String::new(),
            bearer_token: String::new(),
            team_id: String::new(),
            batch_size: default_http_batch(),
            flush_interval_ms: default_http_interval_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyslogSinkSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_syslog_host")]
    pub host: String,
    #[serde(default = "default_syslog_port")]
    pub port: u16,
    #[serde(default)]
    pub use_tcp: bool,
}

fn default_syslog_host() -> String {
    "127.0.0.1".to_string()
}
fn default_syslog_port() -> u16 {
    514
}

impl Default for SyslogSinkSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            host: default_syslog_host(),
            port: default_syslog_port(),
            use_tcp: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditSettings {
    #[serde(default = "default_audit_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub file_dir: PathBuf,
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
    #[serde(default = "default_log_commands")]
    pub log_command_preview: bool,
    #[serde(default)]
    pub http: HttpSinkSettings,
    #[serde(default)]
    pub syslog: SyslogSinkSettings,
}

fn default_audit_enabled() -> bool {
    true
}
fn default_retention_days() -> u32 {
    90
}
fn default_log_commands() -> bool {
    true
}

impl Default for AuditSettings {
    fn default() -> Self {
        Self {
            enabled: default_audit_enabled(),
            file_dir: default_audit_dir(),
            retention_days: default_retention_days(),
            log_command_preview: default_log_commands(),
            http: HttpSinkSettings::default(),
            syslog: SyslogSinkSettings::default(),
        }
    }
}

enum AuditWorkerMsg {
    Event(AuditEvent),
    Shutdown,
}

/// 全局审计记录器（异步写盘 + 可选远程 sink）
pub struct AuditLogger {
    tx: Option<Sender<AuditWorkerMsg>>,
    settings: Arc<Mutex<AuditSettings>>,
    _join: Option<JoinHandle<()>>,
}

impl AuditLogger {
    pub fn new(settings: AuditSettings) -> Self {
        let settings = Arc::new(Mutex::new(settings));
        if !settings.lock().unwrap().enabled {
            return Self {
                tx: None,
                settings,
                _join: None,
            };
        }
        let (tx, rx) = mpsc::channel();
        let settings_clone = Arc::clone(&settings);
        let join = thread::spawn(move || audit_worker(rx, settings_clone));
        Self {
            tx: Some(tx),
            settings,
            _join: Some(join),
        }
    }

    pub fn update_settings(&self, settings: AuditSettings) {
        *self.settings.lock().unwrap() = settings;
    }

    pub fn settings(&self) -> AuditSettings {
        self.settings.lock().unwrap().clone()
    }

    pub fn record(&self, event: AuditEvent) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(AuditWorkerMsg::Event(event));
        }
    }

    pub fn record_simple(
        &self,
        category: AuditCategory,
        action: &str,
        outcome: AuditOutcome,
    ) {
        self.record(AuditEvent::new(category, action, outcome));
    }
}

impl Drop for AuditLogger {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(AuditWorkerMsg::Shutdown);
        }
        if let Some(j) = self._join.take() {
            let _ = j.join();
        }
    }
}

fn audit_worker(rx: Receiver<AuditWorkerMsg>, settings: Arc<Mutex<AuditSettings>>) {
    let mut http_pending: VecDeque<AuditEvent> = VecDeque::new();
    for ev in load_pending_team_events() {
        http_pending.push_back(ev);
    }
    if !http_pending.is_empty() {
        clear_pending_team_events_file();
    }
    let mut last_http_flush = std::time::Instant::now();
    loop {
        let timeout = Duration::from_millis(200);
        match rx.recv_timeout(timeout) {
            Ok(AuditWorkerMsg::Event(ev)) => {
                let cfg = settings.lock().unwrap().clone();
                if cfg.enabled {
                    let _ = write_event_file(&cfg, &ev);
                    if cfg.syslog.enabled {
                        let _ = send_syslog(&cfg.syslog, &ev);
                    }
                    if cfg.http.enabled {
                        http_pending.push_back(ev);
                    }
                }
            }
            Ok(AuditWorkerMsg::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let cfg = settings.lock().unwrap().clone();
        if cfg.http.enabled && !http_pending.is_empty() {
            let batch = cfg.http.batch_size.max(1);
            let interval = Duration::from_millis(cfg.http.flush_interval_ms.max(500));
            if http_pending.len() >= batch || last_http_flush.elapsed() >= interval {
                let take = http_pending.len().min(batch);
                let batch_events: Vec<AuditEvent> = http_pending.drain(..take).collect();
                if flush_http(&cfg.http, &batch_events).is_err() {
                    let _ = append_pending_team_events(&batch_events);
                    for ev in batch_events.into_iter().rev() {
                        http_pending.push_front(ev);
                    }
                    while http_pending.len() > HTTP_QUEUE_CAP {
                        if let Some(ev) = http_pending.pop_back() {
                            let _ = append_pending_team_events(&[ev]);
                        }
                    }
                } else if http_pending.is_empty() {
                    clear_pending_team_events_file();
                }
                last_http_flush = std::time::Instant::now();
            }
            while http_pending.len() > HTTP_QUEUE_CAP {
                let dropped = http_pending.pop_front();
                if let Some(ev) = dropped {
                    let _ = append_pending_team_events(&[ev]);
                }
            }
        }
    }
    let cfg = settings.lock().unwrap().clone();
    if cfg.http.enabled && !http_pending.is_empty() {
        let batch_events: Vec<AuditEvent> = http_pending.drain(..).collect();
        if flush_http(&cfg.http, &batch_events).is_ok() {
            clear_pending_team_events_file();
        } else {
            let _ = append_pending_team_events(&batch_events);
        }
    }
}

fn audit_file_path(dir: &Path, ts: &str) -> PathBuf {
    let day = ts.get(0..10).unwrap_or("unknown");
    dir.join(format!("audit-{day}.jsonl"))
}

fn write_event_file(cfg: &AuditSettings, event: &AuditEvent) -> std::io::Result<()> {
    fs::create_dir_all(&cfg.file_dir)?;
    let path = audit_file_path(&cfg.file_dir, &event.ts);
    rotate_if_needed(&path)?;
    let line = serde_json::to_string(event)? + "\n";
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    f.write_all(line.as_bytes())?;
    f.sync_data()?;
    cleanup_old_audit_files(&cfg.file_dir, cfg.retention_days);
    Ok(())
}

fn rotate_if_needed(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let meta = fs::metadata(path)?;
    if meta.len() < MAX_FILE_BYTES {
        return Ok(());
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audit");
    let rotated = path.with_file_name(format!(
        "{stem}-{}.jsonl",
        Utc::now().format("%H%M%S")
    ));
    fs::rename(path, rotated)?;
    Ok(())
}

fn cleanup_old_audit_files(dir: &Path, retention_days: u32) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
    for entry in read.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        let modified: DateTime<Utc> = modified.into();
        if modified < cutoff {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn send_syslog(cfg: &SyslogSinkSettings, event: &AuditEvent) -> std::io::Result<()> {
    let payload = syslog_payload(event);
    let msg = format!(
        "<134>1 {} {} MistTerm audit - - - {}",
        event.ts,
        event.actor.hostname,
        payload
    );
    if cfg.use_tcp {
        use std::io::Write as _;
        use std::net::TcpStream;
        let mut stream = TcpStream::connect((cfg.host.as_str(), cfg.port))?;
        stream.write_all(msg.as_bytes())?;
        stream.write_all(b"\n")?;
    } else {
        use std::net::UdpSocket;
        let sock = UdpSocket::bind("0.0.0.0:0")?;
        sock.send_to(msg.as_bytes(), (cfg.host.as_str(), cfg.port))?;
    }
    Ok(())
}

fn syslog_payload(event: &AuditEvent) -> String {
    serde_json::to_string(event).unwrap_or_else(|_| "{}".into())
}

fn flush_http(cfg: &HttpSinkSettings, events: &[AuditEvent]) -> Result<(), String> {
    if cfg.url.is_empty() || events.is_empty() {
        return Ok(());
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let body = if cfg.url.contains("/v1/audit/events") {
        audit_events_team_api_body(events, &cfg.team_id)
    } else {
        json!({ "events": events })
    };
    let mut req = client.post(&cfg.url).json(&body);
    if !cfg.bearer_token.is_empty() {
        req = req.bearer_auth(&cfg.bearer_token);
    }
    req.send().map_err(|e| e.to_string())?;
    Ok(())
}

/// Mist 团队平台 `POST /v1/audit/events` 请求体格式。
fn audit_events_team_api_body(events: &[AuditEvent], team_id: &str) -> Value {
    let team_opt = if team_id.is_empty() {
        None
    } else {
        Some(team_id)
    };
    let mapped: Vec<Value> = events
        .iter()
        .map(|ev| {
            json!({
                "event_id": ev.event_id,
                "category": audit_category_name(ev.category),
                "action": ev.action,
                "outcome": audit_outcome_name(ev.outcome),
                "team_id": team_opt,
                "ts": ev.ts,
                "session_id": ev.session_id,
                "host": ev.host,
                "resource": ev.resource,
                "detail": ev.detail,
            })
        })
        .collect();
    json!({ "events": mapped })
}

fn audit_category_name(c: AuditCategory) -> &'static str {
    match c {
        AuditCategory::Auth => "auth",
        AuditCategory::Session => "session",
        AuditCategory::Credential => "credential",
        AuditCategory::Vault => "vault",
        AuditCategory::Config => "config",
        AuditCategory::Fragment => "fragment",
        AuditCategory::Command => "command",
    }
}

fn audit_outcome_name(o: AuditOutcome) -> &'static str {
    match o {
        AuditOutcome::Success => "success",
        AuditOutcome::Failure => "failure",
        AuditOutcome::Denied => "denied",
    }
}

/// 命令预览：截断 + 不含换行（审计用，非 session_log）
pub fn command_preview(cmd: &str, max_len: usize) -> String {
    let one_line: String = cmd.chars().filter(|c| *c != '\n' && *c != '\r').collect();
    if one_line.len() <= max_len {
        one_line
    } else {
        format!("{}…", one_line.chars().take(max_len).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_event_serializes() {
        let ev = AuditEvent::new(AuditCategory::Session, "connect.start", AuditOutcome::Success)
            .with_host("10.0.0.1");
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains("connect.start"));
        assert!(s.contains("10.0.0.1"));
    }

    #[test]
    fn command_preview_truncates() {
        let s = command_preview("echo hello world", 8);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn audit_event_id_format() {
        let id = new_audit_event_id();
        assert!(id.starts_with("evt_"));
        let parts: Vec<_> = id.split('_').collect();
        assert_eq!(parts.len(), 3);
        assert!(parts[1].parse::<i64>().is_ok());
        assert_eq!(parts[2].len(), 8);
    }
}
