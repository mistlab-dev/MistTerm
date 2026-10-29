//! 跨设备主机列表 + 可选凭证同步（#178）。
//!
//! 对接已合并的 mist-server `GET`/`PUT /v1/sync/hosts` 与 `/v1/sync/credentials`：
//! 登录态（`Authorization: Bearer`）、按 `user_id` 隔离、主机正文是不透明 JSON、
//! 凭证正文只有 `ciphertext` + `kdf_hint`、`ETag` / `If-Match`、冲突 `409`（带服务端副本）、
//! 过大 `413`。
//!
//! 云端没有能解开凭证的密钥。同步口令和恢复码只在客户端派生；`device_key` 只保护本机文件，
//! 不用作跨设备解封密钥。未登录时这些函数不会被调用，本机 SSH 仍只读写本地会话。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::credential::{Credential, CredentialAuthKind, CredentialCategory, SecretBackend};
use crate::core::session::{SessionAuthMode, SessionConfig};
use crate::core::team::{RawSyncResponse, TeamClient};
use crate::security::encrypted_file;

const HOSTS_PATH: &str = "/v1/sync/hosts";
const CREDS_PATH: &str = "/v1/sync/credentials";
const SCHEMA: u32 = 1;
const EXPORT_FORMAT: &str = "mistterm-export-v1";

pub const EXPORT_HOSTS_FILE: &str = "hosts.json";
pub const EXPORT_CREDS_FILE: &str = "credentials.envelope.json";
pub const EXPORT_MANIFEST_FILE: &str = "manifest.json";

const PROD_M_KIB: u32 = 19_456;
const PROD_T: u32 = 2;
const PROD_P: u32 = 1;
const MAX_M_KIB: u32 = 262_144;
const MAX_T: u32 = 8;
const MAX_P: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockError {
    WrongPassphrase,
    WrongRecoveryCode,
    NoRecoveryCode,
    CorruptEnvelope,
}

impl UnlockError {
    pub fn text(self, zh: bool) -> String {
        match (self, zh) {
            (Self::WrongPassphrase, true) => {
                "同步口令不正确，云端凭证未改写。请重试，或改用恢复码。".into()
            }
            (Self::WrongPassphrase, false) => {
                "Sync passphrase is wrong. Cloud credentials were not overwritten. Try again, or use the recovery code.".into()
            }
            (Self::WrongRecoveryCode, true) => "恢复码不正确，云端凭证未改写。".into(),
            (Self::WrongRecoveryCode, false) => {
                "Recovery code is wrong. Cloud credentials were not overwritten.".into()
            }
            (Self::NoRecoveryCode, true) => {
                "这份凭证没有恢复码，只能用当初设置的同步口令解封。".into()
            }
            (Self::NoRecoveryCode, false) => {
                "This credential envelope has no recovery code. Only the original sync passphrase can unwrap it.".into()
            }
            (Self::CorruptEnvelope, true) => {
                "云端凭证信封无法识别或已损坏，本机数据未改写。".into()
            }
            (Self::CorruptEnvelope, false) => {
                "The cloud credential envelope is unreadable or damaged. Local data was left unchanged.".into()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncNote {
    HostAutoMerged { count: usize },
    HostConflict { name: String },
    HostPushDeferred,
    CredTie { label: String },
    CredPassphraseRejected,
    CredRecoveryReadOnly,
    CredSkippedNoKey,
    HostsUploaded,
    CredsUploaded,
}

pub fn format_notes(notes: &[SyncNote], zh: bool) -> String {
    let mut lines = Vec::new();
    for note in notes {
        lines.push(match (note, zh) {
            (SyncNote::HostAutoMerged { count }, true) => {
                format!("主机列表已按较新的 updated_at 合并 {count} 条。")
            }
            (SyncNote::HostAutoMerged { count }, false) => {
                format!("Merged {count} hosts by newer updated_at.")
            }
            (SyncNote::HostConflict { name }, true) => {
                format!("「{name}」两边同时修改且时间戳相同，已保留冲突副本，没有静默覆盖。")
            }
            (SyncNote::HostConflict { name }, false) => {
                format!("\"{name}\" changed on both sides with the same timestamp. A conflict copy was kept.")
            }
            (SyncNote::HostPushDeferred, true) => {
                "服务端版本在写入时又变了，冲突副本已留在本机，请再同步一次。".into()
            }
            (SyncNote::HostPushDeferred, false) => {
                "The server copy changed again during upload. Conflict copies stay local; sync once more.".into()
            }
            (SyncNote::CredTie { label }, true) => {
                format!("凭证「{label}」时间戳相同且内容不同：已保留本机，云端对应字段也未覆盖。")
            }
            (SyncNote::CredTie { label }, false) => {
                format!("Credential \"{label}\" differs with the same timestamp. Local and cloud copies were both kept.")
            }
            (SyncNote::CredPassphraseRejected, true) => {
                "同步口令不正确。已用恢复码把云端凭证应用到本机，未改写云端信封。".into()
            }
            (SyncNote::CredPassphraseRejected, false) => {
                "Sync passphrase was wrong. Cloud credentials were applied locally with the recovery code and the cloud envelope was not rewritten.".into()
            }
            (SyncNote::CredRecoveryReadOnly, true) => {
                "已用恢复码解封并应用到本机。要上传本机改动，请再设置同步口令。".into()
            }
            (SyncNote::CredRecoveryReadOnly, false) => {
                "Unwrapped with the recovery code and applied locally. Set a sync passphrase before uploading local changes.".into()
            }
            (SyncNote::CredSkippedNoKey, true) => {
                "已开启凭证同步，但还没有同步口令或恢复码，本次没有上传或下载凭证。".into()
            }
            (SyncNote::CredSkippedNoKey, false) => {
                "Credential sync is on, but no sync passphrase or recovery code is set. Credentials were not uploaded or downloaded.".into()
            }
            (SyncNote::HostsUploaded, true) => "主机列表已写回云端。".into(),
            (SyncNote::HostsUploaded, false) => "Host list uploaded.".into(),
            (SyncNote::CredsUploaded, true) => "凭证信封已写回云端（仅密文）。".into(),
            (SyncNote::CredsUploaded, false) => "Credential envelope uploaded (ciphertext only).".into(),
        });
    }
    lines.join("\n")
}

fn schema_v1() -> u32 {
    SCHEMA
}

/// 上传到 `/v1/sync/hosts` 的不透明文档。没有密码，也没有私钥正文。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostDocument {
    #[serde(default = "schema_v1")]
    pub schema: u32,
    #[serde(default)]
    pub hosts: Vec<HostRecord>,
}

impl Default for HostDocument {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            hosts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostRecord {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub auth_mode: SessionAuthMode,
    #[serde(default = "default_true")]
    pub use_ssh_agent: bool,
    /// 密钥路径或引用，不是私钥文件内容。
    #[serde(default)]
    pub private_key_path: String,
    #[serde(default)]
    pub secret_backend: SecretBackend,
    #[serde(default)]
    pub proxy_jump: String,
    #[serde(default)]
    pub proxy_command: String,
    #[serde(default)]
    pub color_tag: String,
    #[serde(default = "default_true")]
    pub keepalive_enabled: bool,
    #[serde(default = "default_keepalive_interval")]
    pub keepalive_interval_secs: u32,
    #[serde(default = "default_keepalive_count")]
    pub keepalive_count_max: u8,
    #[serde(default = "default_true")]
    pub keepalive_auto_reconnect: bool,
    #[serde(default)]
    pub local_forwards_text: String,
    #[serde(default)]
    pub remote_forwards_text: String,
    #[serde(default)]
    pub dynamic_forwards_text: String,
    #[serde(default)]
    pub post_connect_command: String,
    #[serde(default)]
    pub ssh_config_marker: Option<String>,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub deleted: bool,
}

fn default_port() -> u16 {
    22
}
fn default_category() -> CredentialCategory {
    CredentialCategory::Server
}
fn default_true() -> bool {
    true
}
fn default_keepalive_interval() -> u32 {
    30
}
fn default_keepalive_count() -> u8 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SyncCredentialBlob {
    #[serde(default = "schema_v1")]
    pub schema: u32,
    #[serde(default)]
    pub session_secrets: Vec<SyncSessionSecret>,
    #[serde(default)]
    pub vault: Vec<SyncVaultEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncSessionSecret {
    pub id: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncVaultEntry {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_category")]
    pub category: CredentialCategory,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub auth: CredentialAuthKind,
    #[serde(default)]
    pub secret: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub secret_backend: SecretBackend,
    #[serde(default)]
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CloudCredentialEnvelope {
    pub ciphertext: String,
    pub kdf_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct KdfHint {
    v: u32,
    kdf: String,
    m_kib: u32,
    t: u32,
    p: u32,
    salt_b64: String,
    wrap_nonce_b64: String,
    wrapped_dek_b64: String,
    payload_nonce_b64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recovery_salt_b64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recovery_wrap_nonce_b64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recovery_wrapped_dek_b64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretStamp {
    pub id: String,
    pub updated_at: i64,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultTombstone {
    pub id: String,
    pub name: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct HostSyncState {
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub hosts_etag: Option<String>,
    #[serde(default)]
    pub credentials_etag: Option<String>,
    #[serde(default)]
    pub last_document: HostDocument,
    #[serde(default)]
    pub known_vault_ids: Vec<String>,
    #[serde(default)]
    pub vault_tombstones: Vec<VaultTombstone>,
    #[serde(default)]
    pub session_secret_stamps: Vec<SecretStamp>,
}

impl HostSyncState {
    pub fn path() -> PathBuf {
        mistterm_dir().join("host_sync_state.json")
    }

    pub fn load() -> Self {
        encrypted_file::load_encrypted_json(&Self::path())
    }

    /// 换了 mistlab 账号就不沿用上一账号的 ETag / 墓碑，避免把别人的版本写错。
    pub fn load_for_user(user_id: &str) -> Self {
        let loaded = Self::load();
        if !loaded.user_id.is_empty() && loaded.user_id != user_id {
            return Self {
                user_id: user_id.to_string(),
                ..Self::default()
            };
        }
        let mut loaded = loaded;
        loaded.user_id = user_id.to_string();
        loaded
    }

    pub fn save(&self) {
        if let Err(e) = encrypted_file::save_encrypted_json(&Self::path(), self) {
            log::warn!("host sync state save failed: {e}");
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncKeyMaterial {
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub passphrase: String,
    #[serde(default)]
    pub recovery_code: String,
}

impl SyncKeyMaterial {
    pub fn path() -> PathBuf {
        mistterm_dir().join("sync_key.json")
    }

    pub fn load() -> Self {
        encrypted_file::load_encrypted_json(&Self::path())
    }

    pub fn for_user(&self, user_id: &str) -> Self {
        if self.user_id.is_empty() || self.user_id == user_id {
            let mut copy = self.clone();
            copy.user_id = user_id.to_string();
            copy
        } else {
            Self {
                user_id: user_id.to_string(),
                ..Self::default()
            }
        }
    }

    pub fn save(&self) {
        if let Err(e) = encrypted_file::save_encrypted_json(&Self::path(), self) {
            log::warn!("sync key save failed: {e}");
        }
    }
}

fn mistterm_dir() -> PathBuf {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mistterm");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn generate_recovery_code() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && i % 2 == 0 {
            s.push('-');
        }
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

fn fingerprint(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn host_record_from_session(s: &SessionConfig) -> HostRecord {
    HostRecord {
        id: s.id.clone(),
        name: s.name.clone(),
        host: s.host.clone(),
        port: s.port,
        username: s.username.clone(),
        group: s.group.clone(),
        auth_mode: s.auth_mode,
        use_ssh_agent: s.use_ssh_agent,
        private_key_path: s.private_key_path.clone(),
        secret_backend: s.secret_backend.clone(),
        proxy_jump: s.proxy_jump.clone(),
        proxy_command: s.proxy_command.clone(),
        color_tag: s.color_tag.clone(),
        keepalive_enabled: s.keepalive_enabled,
        keepalive_interval_secs: s.keepalive_interval_secs,
        keepalive_count_max: s.keepalive_count_max,
        keepalive_auto_reconnect: s.keepalive_auto_reconnect,
        local_forwards_text: s.local_forwards_text.clone(),
        remote_forwards_text: s.remote_forwards_text.clone(),
        dynamic_forwards_text: s.dynamic_forwards_text.clone(),
        post_connect_command: s.post_connect_command.clone(),
        ssh_config_marker: s.ssh_config_marker.clone(),
        created_at: s.created_at.unwrap_or(0),
        updated_at: s.updated_at.or(s.created_at).unwrap_or(0),
        deleted: false,
    }
}

pub fn host_document_from_sessions(sessions: &[SessionConfig]) -> HostDocument {
    local_host_document(sessions, &HostDocument::default(), 0)
}

pub fn local_host_document(
    sessions: &[SessionConfig],
    previous: &HostDocument,
    now: i64,
) -> HostDocument {
    let mut hosts = Vec::new();
    let mut live = HashSet::new();
    for session in sessions {
        if session.id.is_empty() {
            continue;
        }
        live.insert(session.id.clone());
        hosts.push(host_record_from_session(session));
    }
    for prev in &previous.hosts {
        if prev.id.is_empty() || live.contains(&prev.id) {
            continue;
        }
        let mut tomb = prev.clone();
        if !tomb.deleted {
            tomb.deleted = true;
            tomb.updated_at = now.max(tomb.updated_at);
        }
        hosts.push(tomb);
    }
    hosts.sort_by(|a, b| a.id.cmp(&b.id));
    HostDocument {
        schema: SCHEMA,
        hosts,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostMerge {
    pub document: HostDocument,
    pub conflict_names: Vec<String>,
    pub auto_newer: usize,
}

pub fn merge_hosts(local: &HostDocument, remote: &HostDocument) -> HostMerge {
    let mut index: HashMap<String, HostRecord> = HashMap::new();
    for host in remote.hosts.iter().chain(local.hosts.iter()) {
        if !host.id.is_empty() {
            index.entry(host.id.clone()).or_insert_with(|| host.clone());
        }
    }
    let mut ids: Vec<String> = index.keys().cloned().collect();
    ids.sort();
    let local_by: HashMap<&str, &HostRecord> =
        local.hosts.iter().map(|h| (h.id.as_str(), h)).collect();
    let remote_by: HashMap<&str, &HostRecord> =
        remote.hosts.iter().map(|h| (h.id.as_str(), h)).collect();
    let mut hosts = Vec::new();
    let mut conflict_names = Vec::new();
    let mut auto_newer = 0usize;
    for id in ids {
        if id.contains("~conflict~")
            && local_by.contains_key(id.as_str())
            && remote_by.contains_key(id.as_str())
        {
            // 冲突副本本身再按普通记录合并。
        }
        match (local_by.get(id.as_str()), remote_by.get(id.as_str())) {
            (Some(l), None) => hosts.push((*l).clone()),
            (None, Some(r)) => hosts.push((*r).clone()),
            (Some(l), Some(r)) => {
                if content_key(l) == content_key(r) {
                    hosts.push(if l.updated_at >= r.updated_at {
                        (*l).clone()
                    } else {
                        (*r).clone()
                    });
                    continue;
                }
                if l.updated_at != r.updated_at {
                    auto_newer += 1;
                    hosts.push(if l.updated_at > r.updated_at {
                        (*l).clone()
                    } else {
                        (*r).clone()
                    });
                    continue;
                }
                let (keep, other) = if l.deleted && !r.deleted {
                    (*r, *l)
                } else {
                    (*l, *r)
                };
                hosts.push(keep.clone());
                if keep.deleted == other.deleted {
                    let copy = conflict_copy(other);
                    conflict_names.push(keep.name.clone());
                    if !hosts.iter().any(|h| h.id == copy.id) {
                        hosts.push(copy);
                    }
                } else {
                    conflict_names.push(keep.name.clone());
                }
            }
            (None, None) => {}
        }
    }
    hosts.sort_by(|a, b| a.id.cmp(&b.id));
    HostMerge {
        document: HostDocument {
            schema: SCHEMA,
            hosts,
        },
        conflict_names,
        auto_newer,
    }
}

fn content_key(host: &HostRecord) -> String {
    let mut copy = host.clone();
    copy.updated_at = 0;
    serde_json::to_string(&copy).unwrap_or_default()
}

fn conflict_copy(host: &HostRecord) -> HostRecord {
    let mut copy = host.clone();
    copy.id = format!("{}~conflict~{}", host.id, host.updated_at);
    let suffix = " (conflict)";
    if !copy.name.ends_with(suffix) {
        copy.name.push_str(suffix);
    }
    copy.deleted = false;
    copy
}

fn canonical_hosts(doc: &HostDocument) -> String {
    let mut copy = doc.clone();
    copy.schema = SCHEMA;
    copy.hosts.sort_by(|a, b| a.id.cmp(&b.id));
    serde_json::to_string(&copy).unwrap_or_default()
}

pub fn sessions_after_sync(
    local: &[SessionConfig],
    doc: &HostDocument,
    creds: Option<&SyncCredentialBlob>,
) -> Vec<SessionConfig> {
    let mut by_id: HashMap<String, SessionConfig> =
        local.iter().cloned().map(|s| (s.id.clone(), s)).collect();
    let mut deleted = HashSet::new();
    let mut incoming = Vec::new();
    for host in &doc.hosts {
        if host.id.is_empty() {
            continue;
        }
        if host.deleted {
            deleted.insert(host.id.clone());
            by_id.remove(&host.id);
            continue;
        }
        let mut session = if let Some(mut existing) = by_id.remove(&host.id) {
            let password = existing.password.clone();
            apply_record(&mut existing, host);
            existing.password = password;
            existing
        } else {
            session_from_record(host)
        };
        if let Some(blob) = creds {
            apply_one_secret(&mut session, blob);
        }
        incoming.push(session);
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for session in local {
        if deleted.contains(&session.id) {
            continue;
        }
        if let Some(updated) = incoming.iter().find(|s| s.id == session.id) {
            out.push(updated.clone());
            seen.insert(session.id.clone());
        } else if let Some(kept) = by_id.get(&session.id) {
            let mut kept = kept.clone();
            if let Some(blob) = creds {
                apply_one_secret(&mut kept, blob);
            }
            out.push(kept);
            seen.insert(session.id.clone());
        }
    }
    for session in incoming {
        if seen.insert(session.id.clone()) {
            out.push(session);
        }
    }
    out
}

fn apply_one_secret(session: &mut SessionConfig, blob: &SyncCredentialBlob) {
    if session.secret_backend.is_vault() {
        return;
    }
    let Some(secret) = blob.session_secrets.iter().find(|s| s.id == session.id) else {
        return;
    };
    if secret.deleted {
        if !session.password.is_empty() {
            session.password.clear();
        }
        return;
    }
    if session.password != secret.password {
        session.password = secret.password.clone();
        session.updated_at = Some(secret.updated_at.max(session.updated_at.unwrap_or(0)));
    }
}

fn apply_record(dst: &mut SessionConfig, host: &HostRecord) {
    dst.id = host.id.clone();
    dst.name = host.name.clone();
    dst.host = host.host.clone();
    dst.port = host.port;
    dst.username = host.username.clone();
    dst.group = host.group.clone();
    dst.auth_mode = host.auth_mode;
    dst.use_ssh_agent = host.use_ssh_agent;
    dst.private_key_path = host.private_key_path.clone();
    dst.secret_backend = host.secret_backend.clone();
    dst.proxy_jump = host.proxy_jump.clone();
    dst.proxy_command = host.proxy_command.clone();
    dst.color_tag = host.color_tag.clone();
    dst.keepalive_enabled = host.keepalive_enabled;
    dst.keepalive_interval_secs = host.keepalive_interval_secs;
    dst.keepalive_count_max = host.keepalive_count_max;
    dst.keepalive_auto_reconnect = host.keepalive_auto_reconnect;
    dst.local_forwards_text = host.local_forwards_text.clone();
    dst.remote_forwards_text = host.remote_forwards_text.clone();
    dst.dynamic_forwards_text = host.dynamic_forwards_text.clone();
    dst.post_connect_command = host.post_connect_command.clone();
    dst.ssh_config_marker = host.ssh_config_marker.clone();
    dst.created_at = if host.created_at == 0 {
        dst.created_at
    } else {
        Some(host.created_at)
    };
    dst.updated_at = Some(host.updated_at);
}

fn session_from_record(host: &HostRecord) -> SessionConfig {
    let mut session = SessionConfig::default();
    session.password.clear();
    apply_record(&mut session, host);
    session
}

pub fn local_credential_blob(
    sessions: &[SessionConfig],
    vault: &[Credential],
    state: &HostSyncState,
    now: i64,
) -> SyncCredentialBlob {
    let mut session_secrets = Vec::new();
    for session in sessions {
        if session.id.is_empty() || session.secret_backend.is_vault() {
            continue;
        }
        let updated_at = session_secret_updated_at(session, &state.session_secret_stamps, now);
        session_secrets.push(SyncSessionSecret {
            id: session.id.clone(),
            password: session.password.clone(),
            updated_at,
            deleted: false,
        });
    }
    let mut vault_entries: Vec<SyncVaultEntry> = vault.iter().map(vault_from_credential).collect();
    let live_ids: HashSet<String> = vault_entries.iter().map(|e| e.id.clone()).collect();
    for tomb in &state.vault_tombstones {
        if tomb.id.is_empty() || live_ids.contains(&tomb.id) {
            continue;
        }
        vault_entries.push(deleted_vault(tomb));
    }
    for id in &state.known_vault_ids {
        if id.is_empty() || live_ids.contains(id) {
            continue;
        }
        if state.vault_tombstones.iter().any(|t| t.id == *id) {
            continue;
        }
        vault_entries.push(SyncVaultEntry {
            id: id.clone(),
            name: id.clone(),
            updated_at: now,
            deleted: true,
            ..empty_vault()
        });
    }
    session_secrets.sort_by(|a, b| a.id.cmp(&b.id));
    vault_entries.sort_by(|a, b| a.id.cmp(&b.id));
    SyncCredentialBlob {
        schema: SCHEMA,
        session_secrets,
        vault: vault_entries,
    }
}

fn session_secret_updated_at(session: &SessionConfig, stamps: &[SecretStamp], now: i64) -> i64 {
    let fp = fingerprint(&session.password);
    if let Some(stamp) = stamps.iter().find(|s| s.id == session.id) {
        if stamp.fingerprint == fp {
            return stamp.updated_at;
        }
        return now;
    }
    session.updated_at.or(session.created_at).unwrap_or(0)
}

fn empty_vault() -> SyncVaultEntry {
    SyncVaultEntry {
        id: String::new(),
        name: String::new(),
        category: CredentialCategory::Server,
        host: String::new(),
        port: 0,
        username: String::new(),
        auth: CredentialAuthKind::Password,
        secret: String::new(),
        notes: String::new(),
        tags: Vec::new(),
        created_at: 0,
        updated_at: 0,
        secret_backend: SecretBackend::default(),
        deleted: false,
    }
}

fn deleted_vault(tomb: &VaultTombstone) -> SyncVaultEntry {
    SyncVaultEntry {
        id: tomb.id.clone(),
        name: tomb.name.clone(),
        updated_at: tomb.updated_at,
        deleted: true,
        ..empty_vault()
    }
}

fn vault_from_credential(c: &Credential) -> SyncVaultEntry {
    let secret = if c.secret_backend.is_vault() {
        String::new()
    } else {
        c.secret.clone()
    };
    SyncVaultEntry {
        id: c.id.clone(),
        name: c.name.clone(),
        category: c.category,
        host: c.host.clone(),
        port: c.port,
        username: c.username.clone(),
        auth: c.auth,
        secret,
        notes: c.notes.clone(),
        tags: c.tags.clone(),
        created_at: c.created_at,
        updated_at: c.updated_at,
        secret_backend: c.secret_backend.clone(),
        deleted: false,
    }
}

pub fn live_vault_credentials(blob: &SyncCredentialBlob) -> Vec<Credential> {
    blob.vault
        .iter()
        .filter(|e| !e.deleted && !e.id.is_empty())
        .map(|e| Credential {
            id: e.id.clone(),
            name: e.name.clone(),
            category: e.category,
            host: e.host.clone(),
            port: e.port,
            username: e.username.clone(),
            auth: e.auth,
            secret: if e.secret_backend.is_vault() {
                String::new()
            } else {
                e.secret.clone()
            },
            notes: e.notes.clone(),
            tags: e.tags.clone(),
            created_at: e.created_at,
            updated_at: e.updated_at,
            secret_backend: e.secret_backend.clone(),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredMerge {
    pub local_view: SyncCredentialBlob,
    pub upload_view: SyncCredentialBlob,
    pub tie_labels: Vec<String>,
}

pub fn merge_credentials(
    local: &SyncCredentialBlob,
    remote: &SyncCredentialBlob,
    host_tombstones: &[(String, i64)],
) -> CredMerge {
    let (session_local, session_upload, mut ties) = merge_session_secrets(
        &local.session_secrets,
        &remote.session_secrets,
        host_tombstones,
    );
    let (vault_local, vault_upload, vault_ties) = merge_vault(&local.vault, &remote.vault);
    ties.extend(vault_ties);
    CredMerge {
        local_view: SyncCredentialBlob {
            schema: SCHEMA,
            session_secrets: session_local,
            vault: vault_local,
        },
        upload_view: SyncCredentialBlob {
            schema: SCHEMA,
            session_secrets: session_upload,
            vault: vault_upload,
        },
        tie_labels: ties,
    }
}

fn merge_session_secrets(
    local: &[SyncSessionSecret],
    remote: &[SyncSessionSecret],
    host_tombstones: &[(String, i64)],
) -> (Vec<SyncSessionSecret>, Vec<SyncSessionSecret>, Vec<String>) {
    let mut ids: HashSet<&str> = HashSet::new();
    for item in local.iter().chain(remote.iter()) {
        ids.insert(item.id.as_str());
    }
    let mut id_list: Vec<&str> = ids.into_iter().collect();
    id_list.sort();
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut ties = Vec::new();
    for id in id_list {
        let l = local.iter().find(|s| s.id == id);
        let r = remote.iter().find(|s| s.id == id);
        let tomb_at = host_tombstones
            .iter()
            .find(|(hid, _)| hid == id)
            .map(|(_, ts)| *ts);
        if let Some(ts) = tomb_at {
            let newest_secret = [l.map(|s| s.updated_at), r.map(|s| s.updated_at)]
                .into_iter()
                .flatten()
                .max()
                .unwrap_or(0);
            if ts >= newest_secret {
                let deleted = SyncSessionSecret {
                    id: id.to_string(),
                    password: String::new(),
                    updated_at: ts,
                    deleted: true,
                };
                left.push(deleted.clone());
                right.push(deleted);
                continue;
            }
        }
        match (l, r) {
            (Some(a), None) => {
                left.push(a.clone());
                right.push(a.clone());
            }
            (None, Some(b)) => {
                left.push(b.clone());
                right.push(b.clone());
            }
            (Some(a), Some(b)) => push_pair(
                a,
                b,
                secret_content,
                |s| s.id.clone(),
                &mut left,
                &mut right,
                &mut ties,
            ),
            (None, None) => {}
        }
    }
    (left, right, ties)
}

fn secret_content(s: &SyncSessionSecret) -> String {
    format!("{}:{}:{}", s.deleted, s.password, s.id)
}

fn merge_vault(
    local: &[SyncVaultEntry],
    remote: &[SyncVaultEntry],
) -> (Vec<SyncVaultEntry>, Vec<SyncVaultEntry>, Vec<String>) {
    let mut ids: HashSet<&str> = HashSet::new();
    for item in local.iter().chain(remote.iter()) {
        ids.insert(item.id.as_str());
    }
    let mut id_list: Vec<&str> = ids.into_iter().collect();
    id_list.sort();
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut ties = Vec::new();
    for id in id_list {
        match (
            local.iter().find(|s| s.id == id),
            remote.iter().find(|s| s.id == id),
        ) {
            (Some(a), None) => {
                left.push(a.clone());
                right.push(a.clone());
            }
            (None, Some(b)) => {
                left.push(b.clone());
                right.push(b.clone());
            }
            (Some(a), Some(b)) => push_pair(
                a,
                b,
                vault_content,
                |e| {
                    if e.name.is_empty() {
                        e.id.clone()
                    } else {
                        e.name.clone()
                    }
                },
                &mut left,
                &mut right,
                &mut ties,
            ),
            (None, None) => {}
        }
    }
    (left, right, ties)
}

fn push_pair<T, F, L>(
    local: &T,
    remote: &T,
    content: F,
    label: L,
    left: &mut Vec<T>,
    right: &mut Vec<T>,
    ties: &mut Vec<String>,
) where
    T: Clone + HasUpdated,
    F: Fn(&T) -> String,
    L: Fn(&T) -> String,
{
    if content(local) == content(remote) {
        let winner = if local.updated_at() >= remote.updated_at() {
            local.clone()
        } else {
            remote.clone()
        };
        left.push(winner.clone());
        right.push(winner);
        return;
    }
    if local.updated_at() != remote.updated_at() {
        let winner = if local.updated_at() > remote.updated_at() {
            local.clone()
        } else {
            remote.clone()
        };
        left.push(winner.clone());
        right.push(winner);
        return;
    }
    if local.is_deleted() != remote.is_deleted() {
        let live = if local.is_deleted() { remote } else { local };
        ties.push(label(live));
        left.push(live.clone());
        right.push(live.clone());
        return;
    }
    ties.push(label(local));
    left.push(local.clone());
    right.push(remote.clone());
}

trait HasUpdated {
    fn updated_at(&self) -> i64;
    fn is_deleted(&self) -> bool;
}

impl HasUpdated for SyncSessionSecret {
    fn updated_at(&self) -> i64 {
        self.updated_at
    }
    fn is_deleted(&self) -> bool {
        self.deleted
    }
}

impl HasUpdated for SyncVaultEntry {
    fn updated_at(&self) -> i64 {
        self.updated_at
    }
    fn is_deleted(&self) -> bool {
        self.deleted
    }
}

fn vault_content(e: &SyncVaultEntry) -> String {
    let mut copy = e.clone();
    copy.updated_at = 0;
    serde_json::to_string(&copy).unwrap_or_default()
}

fn canonical_blob(blob: &SyncCredentialBlob) -> String {
    let mut copy = blob.clone();
    copy.session_secrets.sort_by(|a, b| a.id.cmp(&b.id));
    copy.vault.sort_by(|a, b| a.id.cmp(&b.id));
    serde_json::to_string(&copy).unwrap_or_default()
}

pub fn seal_credentials(
    passphrase: &str,
    recovery: &str,
    blob: &SyncCredentialBlob,
) -> Result<CloudCredentialEnvelope, UnlockError> {
    let plain = serde_json::to_vec(blob).map_err(|_| UnlockError::CorruptEnvelope)?;
    seal_bytes(passphrase, recovery, &plain, PROD_M_KIB, PROD_T, PROD_P)
}

fn seal_bytes(
    passphrase: &str,
    recovery: &str,
    plain: &[u8],
    m_kib: u32,
    t: u32,
    p: u32,
) -> Result<CloudCredentialEnvelope, UnlockError> {
    if passphrase.is_empty() {
        return Err(UnlockError::WrongPassphrase);
    }
    let mut dek = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut dek);
    let (payload_ct, payload_nonce) = aes_seal(&dek, plain)?;
    let (wrapped, wrap_nonce, salt) = wrap_dek(&dek, passphrase, m_kib, t, p)?;
    let mut hint = KdfHint {
        v: 1,
        kdf: "argon2id".into(),
        m_kib,
        t,
        p,
        salt_b64: B64.encode(salt),
        wrap_nonce_b64: B64.encode(wrap_nonce),
        wrapped_dek_b64: B64.encode(wrapped),
        payload_nonce_b64: B64.encode(payload_nonce),
        recovery_salt_b64: None,
        recovery_wrap_nonce_b64: None,
        recovery_wrapped_dek_b64: None,
    };
    if !recovery.is_empty() {
        let (rwrapped, rnonce, rsalt) = wrap_dek(&dek, recovery, m_kib, t, p)?;
        hint.recovery_salt_b64 = Some(B64.encode(rsalt));
        hint.recovery_wrap_nonce_b64 = Some(B64.encode(rnonce));
        hint.recovery_wrapped_dek_b64 = Some(B64.encode(rwrapped));
    }
    let kdf_hint = serde_json::to_string(&hint).map_err(|_| UnlockError::CorruptEnvelope)?;
    Ok(CloudCredentialEnvelope {
        ciphertext: B64.encode(payload_ct),
        kdf_hint,
    })
}

pub fn open_credentials(
    envelope: &CloudCredentialEnvelope,
    passphrase: &str,
    recovery: &str,
) -> Result<SyncCredentialBlob, UnlockError> {
    let plain = open_bytes(envelope, passphrase, recovery)?;
    serde_json::from_slice(&plain).map_err(|_| UnlockError::CorruptEnvelope)
}

fn open_bytes(
    envelope: &CloudCredentialEnvelope,
    passphrase: &str,
    recovery: &str,
) -> Result<Vec<u8>, UnlockError> {
    let hint: KdfHint =
        serde_json::from_str(&envelope.kdf_hint).map_err(|_| UnlockError::CorruptEnvelope)?;
    if hint.v != 1 || hint.kdf != "argon2id" {
        return Err(UnlockError::CorruptEnvelope);
    }
    if hint.m_kib == 0
        || hint.m_kib > MAX_M_KIB
        || hint.t == 0
        || hint.t > MAX_T
        || hint.p == 0
        || hint.p > MAX_P
    {
        return Err(UnlockError::CorruptEnvelope);
    }
    let mut dek = if !passphrase.is_empty() {
        match unwrap_dek(
            passphrase,
            &hint.salt_b64,
            &hint.wrap_nonce_b64,
            &hint.wrapped_dek_b64,
            hint.m_kib,
            hint.t,
            hint.p,
        ) {
            Ok(key) => Some(key),
            Err(UnlockError::WrongPassphrase) => None,
            Err(e) => return Err(e),
        }
    } else {
        None
    };
    if dek.is_none() {
        if recovery.is_empty() {
            return Err(UnlockError::WrongPassphrase);
        }
        if hint.recovery_wrapped_dek_b64.is_none() {
            return Err(UnlockError::NoRecoveryCode);
        }
        dek = Some(
            match unwrap_dek(
                recovery,
                hint.recovery_salt_b64.as_deref().unwrap_or(""),
                hint.recovery_wrap_nonce_b64.as_deref().unwrap_or(""),
                hint.recovery_wrapped_dek_b64.as_deref().unwrap_or(""),
                hint.m_kib,
                hint.t,
                hint.p,
            ) {
                Ok(key) => key,
                Err(UnlockError::WrongPassphrase) => return Err(UnlockError::WrongRecoveryCode),
                Err(e) => return Err(e),
            },
        );
    }
    let dek = dek.ok_or(UnlockError::CorruptEnvelope)?;
    let nonce = B64
        .decode(hint.payload_nonce_b64)
        .map_err(|_| UnlockError::CorruptEnvelope)?;
    let ct = B64
        .decode(&envelope.ciphertext)
        .map_err(|_| UnlockError::CorruptEnvelope)?;
    aes_open(&dek, &nonce, &ct).map_err(|_| UnlockError::CorruptEnvelope)
}

/// 口令失败、恢复码成功时，调用方不应把失败口令写回信封。
pub fn passphrase_opens(envelope: &CloudCredentialEnvelope, passphrase: &str) -> bool {
    if passphrase.is_empty() {
        return false;
    }
    open_bytes(envelope, passphrase, "").is_ok()
}

fn wrap_dek(
    dek: &[u8; 32],
    secret: &str,
    m_kib: u32,
    t: u32,
    p: u32,
) -> Result<(Vec<u8>, [u8; 12], [u8; 16]), UnlockError> {
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    let kek = derive_kek(secret, &salt, m_kib, t, p)?;
    let (ct, nonce) = aes_seal(&kek, dek)?;
    Ok((ct, nonce, salt))
}

fn unwrap_dek(
    secret: &str,
    salt_b64: &str,
    nonce_b64: &str,
    wrapped_b64: &str,
    m_kib: u32,
    t: u32,
    p: u32,
) -> Result<[u8; 32], UnlockError> {
    if secret.is_empty() {
        return Err(UnlockError::WrongPassphrase);
    }
    let salt = B64
        .decode(salt_b64)
        .map_err(|_| UnlockError::CorruptEnvelope)?;
    let nonce = B64
        .decode(nonce_b64)
        .map_err(|_| UnlockError::CorruptEnvelope)?;
    let wrapped = B64
        .decode(wrapped_b64)
        .map_err(|_| UnlockError::CorruptEnvelope)?;
    let kek = derive_kek(secret, &salt, m_kib, t, p)?;
    let plain = aes_open(&kek, &nonce, &wrapped).map_err(|_| UnlockError::WrongPassphrase)?;
    if plain.len() != 32 {
        return Err(UnlockError::CorruptEnvelope);
    }
    let mut dek = [0u8; 32];
    dek.copy_from_slice(&plain);
    Ok(dek)
}

fn derive_kek(
    password: &str,
    salt: &[u8],
    m_kib: u32,
    t: u32,
    p: u32,
) -> Result<[u8; 32], UnlockError> {
    let params = Params::new(m_kib, t, p, Some(32)).map_err(|_| UnlockError::CorruptEnvelope)?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = [0u8; 32];
    argon
        .hash_password_into(password.as_bytes(), salt, &mut out)
        .map_err(|_| UnlockError::CorruptEnvelope)?;
    Ok(out)
}

fn aes_seal(key: &[u8; 32], plain: &[u8]) -> Result<(Vec<u8>, [u8; 12]), UnlockError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| UnlockError::CorruptEnvelope)?;
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plain)
        .map_err(|_| UnlockError::CorruptEnvelope)?;
    Ok((ct, nonce))
}

fn aes_open(key: &[u8; 32], nonce: &[u8], ct: &[u8]) -> Result<Vec<u8>, UnlockError> {
    if nonce.len() != 12 {
        return Err(UnlockError::CorruptEnvelope);
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| UnlockError::CorruptEnvelope)?;
    cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| UnlockError::WrongPassphrase)
}

pub fn parse_host_document(body: &str) -> Result<HostDocument, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("hosts JSON: {e}"))?;
    let value = unwrap_host_value(value);
    if value.is_null() {
        return Ok(HostDocument::default());
    }
    serde_json::from_value(value).map_err(|e| format!("hosts document: {e}"))
}

fn unwrap_host_value(value: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = value.as_object() {
        for key in ["document", "hosts_document", "server", "current"] {
            if let Some(inner) = obj.get(key) {
                if inner.get("hosts").is_some() || inner.get("schema").is_some() {
                    return inner.clone();
                }
            }
        }
    }
    value
}

fn etag_from_body(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value
        .get("etag")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

pub fn format_if_match(etag: &str) -> String {
    let t = etag.trim();
    if t.is_empty() {
        return String::new();
    }
    if t == "*" || t.starts_with('"') || t.starts_with("W/") || t.starts_with("w/") {
        t.to_string()
    } else {
        format!("\"{t}\"")
    }
}

fn pick_etag(header: Option<String>, body: &str) -> Option<String> {
    header
        .filter(|s| !s.is_empty())
        .or_else(|| etag_from_body(body))
}

#[derive(Debug)]
enum HostsRead {
    Ready {
        doc: HostDocument,
        etag: Option<String>,
    },
    Unauthorized,
    TooLarge,
    Failed {
        status: u16,
        body: String,
    },
}

fn read_hosts(resp: RawSyncResponse) -> HostsRead {
    match resp.status {
        200 | 201 => match parse_host_document(&resp.body) {
            Ok(doc) => HostsRead::Ready {
                doc,
                etag: pick_etag(resp.etag, &resp.body),
            },
            Err(e) => HostsRead::Failed {
                status: resp.status,
                body: e,
            },
        },
        204 | 404 => HostsRead::Ready {
            doc: HostDocument::default(),
            etag: None,
        },
        401 | 403 => HostsRead::Unauthorized,
        413 => HostsRead::TooLarge,
        409 => match parse_host_document(&resp.body) {
            Ok(doc) => HostsRead::Ready {
                doc,
                etag: pick_etag(resp.etag, &resp.body),
            },
            Err(e) => HostsRead::Failed {
                status: 409,
                body: e,
            },
        },
        status => HostsRead::Failed {
            status,
            body: clip(&resp.body),
        },
    }
}

enum PutRead {
    Stored {
        etag: Option<String>,
    },
    Conflict {
        doc: HostDocument,
        etag: Option<String>,
    },
    Unauthorized,
    TooLarge,
    Failed {
        status: u16,
        body: String,
    },
}

fn read_put(resp: RawSyncResponse) -> PutRead {
    match resp.status {
        200 | 201 | 204 => PutRead::Stored {
            etag: pick_etag(resp.etag, &resp.body),
        },
        409 => match parse_host_document(&resp.body) {
            Ok(doc) => PutRead::Conflict {
                doc,
                etag: pick_etag(resp.etag, &resp.body),
            },
            Err(e) => PutRead::Failed {
                status: 409,
                body: e,
            },
        },
        401 | 403 => PutRead::Unauthorized,
        412 | 428 => PutRead::Failed {
            status: resp.status,
            body: clip(&resp.body),
        },
        413 => PutRead::TooLarge,
        status => PutRead::Failed {
            status,
            body: clip(&resp.body),
        },
    }
}

fn clip(body: &str) -> String {
    let t = body.trim();
    if t.chars().count() > 280 {
        let s: String = t.chars().take(280).collect();
        format!("{s}…")
    } else {
        t.to_string()
    }
}

fn http_error(status: u16, body: &str, zh: bool) -> String {
    if zh {
        format!("同步失败（HTTP {status}）：{}", clip(body))
    } else {
        format!("Sync failed (HTTP {status}): {}", clip(body))
    }
}

fn unauthorized_text(zh: bool) -> String {
    if zh {
        "未登录或登录已过期。本机主机列表没有改动。".into()
    } else {
        "Not signed in, or the session expired. The local host list was not changed.".into()
    }
}

fn too_large_text(what_zh: &str, what_en: &str, zh: bool) -> String {
    if zh {
        format!("服务器拒绝同步（HTTP 413）：{what_zh}过大。本机数据未删除。")
    } else {
        format!("Server rejected the sync (HTTP 413): {what_en} is too large. Local data was not deleted.")
    }
}

#[derive(Debug, Clone)]
pub struct SyncRequest {
    pub api_base: String,
    pub access_token: String,
    pub user_id: String,
    pub zh: bool,
    pub sync_hosts: bool,
    pub sync_credentials: bool,
    pub passphrase: String,
    pub recovery_code: String,
    pub sessions: Vec<SessionConfig>,
    pub vault: Vec<Credential>,
    pub state: HostSyncState,
    pub now: i64,
}

#[derive(Debug, Clone)]
pub struct SyncOutcome {
    pub apply_hosts: Option<HostDocument>,
    pub apply_creds: Option<SyncCredentialBlob>,
    pub state: HostSyncState,
    pub message: String,
    pub is_error: bool,
    pub has_conflict: bool,
}

pub fn run_account_sync(req: SyncRequest) -> SyncOutcome {
    let client = match TeamClient::new(&req.api_base) {
        Ok(c) => c,
        Err(e) => {
            return SyncOutcome {
                apply_hosts: None,
                apply_creds: None,
                state: req.state,
                message: e,
                is_error: true,
                has_conflict: false,
            };
        }
    };
    let mut state = req.state.clone();
    state.user_id = req.user_id.clone();
    let mut notes = Vec::new();
    let mut errors = Vec::new();
    let mut apply_hosts = None;
    let mut apply_creds = None;

    let merged_hosts = if req.sync_hosts {
        match exchange_hosts(&client, &req, &mut notes) {
            Ok((doc, etag)) => {
                state.hosts_etag = etag;
                state.last_document = doc.clone();
                apply_hosts = Some(doc.clone());
                Some(doc)
            }
            Err(e) => {
                errors.push(e);
                None
            }
        }
    } else {
        None
    };

    if req.sync_credentials {
        match exchange_credentials(&client, &req, merged_hosts.as_ref(), &mut state, &mut notes) {
            Ok(Some(blob)) => apply_creds = Some(blob),
            Ok(None) => {}
            Err(e) => errors.push(e),
        }
    }

    let mut message = format_notes(&notes, req.zh);
    if !errors.is_empty() {
        if !message.is_empty() {
            message.push('\n');
        }
        message.push_str(&errors.join("\n"));
    }
    if message.is_empty() {
        message = if req.zh {
            "已与云端核对，没有需要写回的变更。".into()
        } else {
            "Checked the cloud copy. Nothing needed uploading.".into()
        };
    }
    let has_conflict = notes.iter().any(|note| {
        matches!(
            note,
            SyncNote::HostConflict { .. } | SyncNote::CredTie { .. } | SyncNote::HostPushDeferred
        )
    });
    SyncOutcome {
        apply_hosts,
        apply_creds,
        state,
        message,
        is_error: !errors.is_empty(),
        has_conflict,
    }
}

fn exchange_hosts(
    client: &TeamClient,
    req: &SyncRequest,
    notes: &mut Vec<SyncNote>,
) -> Result<(HostDocument, Option<String>), String> {
    let local = local_host_document(&req.sessions, &req.state.last_document, req.now);
    let got = client
        .sync_get(HOSTS_PATH, &req.access_token)
        .map_err(|e| e.message)?;
    let (mut remote, mut etag) = match read_hosts(got) {
        HostsRead::Ready { doc, etag } => {
            // 404 的空文档没有 ETag。不要把上一轮 ETag 拿去 If-Match 一个已经不存在的文档。
            if doc.hosts.is_empty() && etag.is_none() {
                (doc, None)
            } else {
                (doc, etag.or_else(|| req.state.hosts_etag.clone()))
            }
        }
        HostsRead::Unauthorized => return Err(unauthorized_text(req.zh)),
        HostsRead::TooLarge => return Err(too_large_text("主机列表", "the host list", req.zh)),
        HostsRead::Failed { status, body } => return Err(http_error(status, &body, req.zh)),
    };
    // 404 没有 ETag，不能把上一轮的 ETag 拿去 If-Match 一个已不存在的文档。
    if remote.hosts.is_empty() && etag.is_none() {
        etag = None;
    }
    let mut merged = merge_hosts(&local, &remote);
    if merged.auto_newer > 0 {
        notes.push(SyncNote::HostAutoMerged {
            count: merged.auto_newer,
        });
    }
    for name in &merged.conflict_names {
        notes.push(SyncNote::HostConflict { name: name.clone() });
    }
    if canonical_hosts(&merged.document) == canonical_hosts(&remote) {
        return Ok((merged.document, etag));
    }
    match put_hosts(client, &req.access_token, &merged.document, etag.clone()) {
        PutRead::Stored { etag: new_etag } => {
            notes.push(SyncNote::HostsUploaded);
            Ok((merged.document, new_etag.or(etag)))
        }
        PutRead::Conflict { doc, etag: cetag } => {
            remote = doc;
            let second = merge_hosts(&merged.document, &remote);
            if second.auto_newer > 0 {
                notes.push(SyncNote::HostAutoMerged {
                    count: second.auto_newer,
                });
            }
            for name in &second.conflict_names {
                notes.push(SyncNote::HostConflict { name: name.clone() });
            }
            merged = second;
            let etag = cetag.or(etag);
            if canonical_hosts(&merged.document) == canonical_hosts(&remote) {
                return Ok((merged.document, etag));
            }
            match put_hosts(client, &req.access_token, &merged.document, etag.clone()) {
                PutRead::Stored { etag: new_etag } => {
                    notes.push(SyncNote::HostsUploaded);
                    Ok((merged.document, new_etag.or(etag)))
                }
                PutRead::Conflict { .. } => {
                    notes.push(SyncNote::HostPushDeferred);
                    Ok((merged.document, etag))
                }
                PutRead::Unauthorized => Err(unauthorized_text(req.zh)),
                PutRead::TooLarge => Err(too_large_text("主机列表", "the host list", req.zh)),
                PutRead::Failed { status, body } => Err(http_error(status, &body, req.zh)),
            }
        }
        PutRead::Unauthorized => Err(unauthorized_text(req.zh)),
        PutRead::TooLarge => Err(too_large_text("主机列表", "the host list", req.zh)),
        PutRead::Failed { status: 428, body } | PutRead::Failed { status: 412, body } => {
            let retry = put_hosts(
                client,
                &req.access_token,
                &merged.document,
                Some("*".into()),
            );
            match retry {
                PutRead::Stored { etag: new_etag } => {
                    notes.push(SyncNote::HostsUploaded);
                    Ok((merged.document, new_etag))
                }
                PutRead::Conflict { doc, etag } => {
                    let _ = (doc, body);
                    notes.push(SyncNote::HostPushDeferred);
                    Ok((merged.document, etag))
                }
                PutRead::Unauthorized => Err(unauthorized_text(req.zh)),
                PutRead::TooLarge => Err(too_large_text("主机列表", "the host list", req.zh)),
                PutRead::Failed { status, body } => Err(http_error(status, &body, req.zh)),
            }
        }
        PutRead::Failed { status, body } => Err(http_error(status, &body, req.zh)),
    }
}

fn put_hosts(
    client: &TeamClient,
    token: &str,
    doc: &HostDocument,
    etag: Option<String>,
) -> PutRead {
    let value = match serde_json::to_value(doc) {
        Ok(v) => v,
        Err(e) => {
            return PutRead::Failed {
                status: 0,
                body: e.to_string(),
            }
        }
    };
    let if_match = etag
        .as_deref()
        .map(format_if_match)
        .filter(|s| !s.is_empty());
    match client.sync_put(HOSTS_PATH, token, if_match.as_deref(), &value) {
        Ok(resp) => read_put(resp),
        Err(e) => PutRead::Failed {
            status: e.status,
            body: e.message,
        },
    }
}

fn exchange_credentials(
    client: &TeamClient,
    req: &SyncRequest,
    merged_hosts: Option<&HostDocument>,
    state: &mut HostSyncState,
    notes: &mut Vec<SyncNote>,
) -> Result<Option<SyncCredentialBlob>, String> {
    if req.passphrase.is_empty() && req.recovery_code.is_empty() {
        notes.push(SyncNote::CredSkippedNoKey);
        return Ok(None);
    }
    let local = local_credential_blob(&req.sessions, &req.vault, state, req.now);
    let got = client
        .sync_get(CREDS_PATH, &req.access_token)
        .map_err(|e| e.message)?;
    let remote_env = match read_creds(got) {
        CredsRead::Missing => None,
        CredsRead::Ready { envelope, etag } => {
            state.credentials_etag = etag.or(state.credentials_etag.clone());
            Some(envelope)
        }
        CredsRead::Unauthorized => return Err(unauthorized_text(req.zh)),
        CredsRead::TooLarge => {
            return Err(too_large_text(
                "凭证信封",
                "the credential envelope",
                req.zh,
            ))
        }
        CredsRead::Failed { status, body } => return Err(http_error(status, &body, req.zh)),
    };

    let (remote_blob, passphrase_ok) = if let Some(env) = &remote_env {
        let opened = open_credentials(env, &req.passphrase, &req.recovery_code)
            .map_err(|e| e.text(req.zh))?;
        let pass_ok = passphrase_opens(env, &req.passphrase);
        if !pass_ok && !req.passphrase.is_empty() && !req.recovery_code.is_empty() {
            notes.push(SyncNote::CredPassphraseRejected);
        } else if !pass_ok && req.passphrase.is_empty() {
            notes.push(SyncNote::CredRecoveryReadOnly);
        }
        (opened, pass_ok)
    } else {
        (SyncCredentialBlob::default(), !req.passphrase.is_empty())
    };

    let tombs = host_tombstones(merged_hosts.unwrap_or(&state.last_document));
    let merged = merge_credentials(&local, &remote_blob, &tombs);
    for label in &merged.tie_labels {
        notes.push(SyncNote::CredTie {
            label: label.clone(),
        });
    }

    let mut local_view = merged.local_view;
    let upload_view = merged.upload_view;
    // 云端信封已经存在、但本次口令解不开时，只应用到本机，不用恢复码轮换口令信封。
    if remote_env.is_some() && !passphrase_ok {
        remember_cred_state(state, &local_view);
        return Ok(Some(local_view));
    }
    if remote_env.is_some() && canonical_blob(&upload_view) == canonical_blob(&remote_blob) {
        remember_cred_state(state, &local_view);
        return Ok(Some(local_view));
    }

    let wrap_pass = if !req.passphrase.is_empty() && passphrase_ok {
        req.passphrase.as_str()
    } else if !req.recovery_code.is_empty() {
        req.recovery_code.as_str()
    } else {
        notes.push(SyncNote::CredSkippedNoKey);
        remember_cred_state(state, &local_view);
        return Ok(Some(local_view));
    };
    let recovery_for_seal = if wrap_pass == req.recovery_code.as_str() {
        ""
    } else {
        req.recovery_code.as_str()
    };
    let envelope =
        seal_credentials(wrap_pass, recovery_for_seal, &upload_view).map_err(|e| e.text(req.zh))?;
    let value = serde_json::to_value(&envelope).map_err(|e| e.to_string())?;
    let if_match = state
        .credentials_etag
        .as_deref()
        .map(format_if_match)
        .filter(|s| !s.is_empty());
    let put = client
        .sync_put(CREDS_PATH, &req.access_token, if_match.as_deref(), &value)
        .map_err(|e| e.message)?;
    match read_creds_put(put) {
        CredsPut::Stored { etag } => {
            state.credentials_etag = etag.or(state.credentials_etag.clone());
            notes.push(SyncNote::CredsUploaded);
            remember_cred_state(state, &local_view);
            Ok(Some(local_view))
        }
        CredsPut::Conflict { envelope, etag } => {
            state.credentials_etag = etag;
            let remote2 = open_credentials(&envelope, &req.passphrase, &req.recovery_code)
                .map_err(|e| e.text(req.zh))?;
            let merged2 = merge_credentials(&local_view, &remote2, &tombs);
            for label in &merged2.tie_labels {
                notes.push(SyncNote::CredTie {
                    label: label.clone(),
                });
            }
            local_view = merged2.local_view;
            let envelope = seal_credentials(wrap_pass, recovery_for_seal, &merged2.upload_view)
                .map_err(|e| e.text(req.zh))?;
            let value = serde_json::to_value(&envelope).map_err(|e| e.to_string())?;
            let if_match = state
                .credentials_etag
                .as_deref()
                .map(format_if_match)
                .filter(|s| !s.is_empty());
            let put2 = client
                .sync_put(CREDS_PATH, &req.access_token, if_match.as_deref(), &value)
                .map_err(|e| e.message)?;
            match read_creds_put(put2) {
                CredsPut::Stored { etag } => {
                    state.credentials_etag = etag.or(state.credentials_etag.clone());
                    notes.push(SyncNote::CredsUploaded);
                }
                CredsPut::Conflict { etag, .. } => {
                    state.credentials_etag = etag;
                    notes.push(SyncNote::HostPushDeferred);
                }
                CredsPut::Unauthorized => return Err(unauthorized_text(req.zh)),
                CredsPut::TooLarge => {
                    return Err(too_large_text(
                        "凭证信封",
                        "the credential envelope",
                        req.zh,
                    ))
                }
                CredsPut::Failed { status, body } => return Err(http_error(status, &body, req.zh)),
            }
            remember_cred_state(state, &local_view);
            Ok(Some(local_view))
        }
        CredsPut::Unauthorized => Err(unauthorized_text(req.zh)),
        CredsPut::TooLarge => Err(too_large_text(
            "凭证信封",
            "the credential envelope",
            req.zh,
        )),
        CredsPut::Failed { status, body } => Err(http_error(status, &body, req.zh)),
    }
}

fn host_tombstones(doc: &HostDocument) -> Vec<(String, i64)> {
    doc.hosts
        .iter()
        .filter(|h| h.deleted)
        .map(|h| (h.id.clone(), h.updated_at))
        .collect()
}

fn remember_cred_state(state: &mut HostSyncState, local_view: &SyncCredentialBlob) {
    state.session_secret_stamps = local_view
        .session_secrets
        .iter()
        .filter(|s| !s.deleted)
        .map(|s| SecretStamp {
            id: s.id.clone(),
            updated_at: s.updated_at,
            fingerprint: fingerprint(&s.password),
        })
        .collect();
    state.known_vault_ids = local_view
        .vault
        .iter()
        .filter(|v| !v.deleted)
        .map(|v| v.id.clone())
        .collect();
    state.vault_tombstones = local_view
        .vault
        .iter()
        .filter(|v| v.deleted)
        .map(|v| VaultTombstone {
            id: v.id.clone(),
            name: v.name.clone(),
            updated_at: v.updated_at,
        })
        .collect();
}

enum CredsRead {
    Missing,
    Ready {
        envelope: CloudCredentialEnvelope,
        etag: Option<String>,
    },
    Unauthorized,
    TooLarge,
    Failed {
        status: u16,
        body: String,
    },
}

fn read_creds(resp: RawSyncResponse) -> CredsRead {
    match resp.status {
        204 | 404 => CredsRead::Missing,
        401 | 403 => CredsRead::Unauthorized,
        413 => CredsRead::TooLarge,
        200 | 201 => match parse_envelope(&resp.body) {
            Ok(envelope) => CredsRead::Ready {
                envelope,
                etag: pick_etag(resp.etag, &resp.body),
            },
            Err(e) => CredsRead::Failed {
                status: resp.status,
                body: e,
            },
        },
        status => CredsRead::Failed {
            status,
            body: clip(&resp.body),
        },
    }
}

enum CredsPut {
    Stored {
        etag: Option<String>,
    },
    Conflict {
        envelope: CloudCredentialEnvelope,
        etag: Option<String>,
    },
    Unauthorized,
    TooLarge,
    Failed {
        status: u16,
        body: String,
    },
}

fn read_creds_put(resp: RawSyncResponse) -> CredsPut {
    match resp.status {
        200 | 201 | 204 => CredsPut::Stored {
            etag: pick_etag(resp.etag, &resp.body),
        },
        409 => match parse_envelope(&resp.body) {
            Ok(envelope) => CredsPut::Conflict {
                envelope,
                etag: pick_etag(resp.etag, &resp.body),
            },
            Err(e) => CredsPut::Failed {
                status: 409,
                body: e,
            },
        },
        401 | 403 => CredsPut::Unauthorized,
        413 => CredsPut::TooLarge,
        status => CredsPut::Failed {
            status,
            body: clip(&resp.body),
        },
    }
}

fn parse_envelope(body: &str) -> Result<CloudCredentialEnvelope, String> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let value = value
        .get("document")
        .cloned()
        .filter(|v| v.get("ciphertext").is_some())
        .unwrap_or(value);
    let envelope: CloudCredentialEnvelope =
        serde_json::from_value(value).map_err(|e| format!("credentials envelope: {e}"))?;
    if envelope.ciphertext.is_empty() || envelope.kdf_hint.is_empty() {
        return Err("credentials envelope is missing ciphertext or kdf_hint".into());
    }
    Ok(envelope)
}

#[derive(Debug, Serialize, Deserialize)]
struct ExportManifest {
    format: String,
    has_credentials: bool,
}

pub fn write_export_files(
    dir: &std::path::Path,
    sessions: &[SessionConfig],
    vault: &[Credential],
    include_credentials: bool,
    passphrase: &str,
    recovery: &str,
    zh: bool,
) -> Result<Vec<String>, String> {
    if include_credentials && passphrase.is_empty() {
        return Err(if zh {
            "导出凭证需要同步口令。云端和导出包都不用本机 device_key 加密。".into()
        } else {
            "Exporting credentials needs a sync passphrase. The cloud and the pack do not use this machine's device_key.".into()
        });
    }
    let mut wrote = Vec::new();
    let hosts = host_document_from_sessions(sessions);
    let hosts_json = serde_json::to_string_pretty(&hosts).map_err(|e| e.to_string())?;
    fs::write(dir.join(EXPORT_HOSTS_FILE), hosts_json).map_err(|e| e.to_string())?;
    wrote.push(EXPORT_HOSTS_FILE.to_string());
    let mut has_credentials = false;
    if include_credentials {
        let blob = local_credential_blob(sessions, vault, &HostSyncState::default(), 0);
        let env = seal_credentials(passphrase, recovery, &blob).map_err(|e| e.text(zh))?;
        let text = serde_json::to_string_pretty(&env).map_err(|e| e.to_string())?;
        fs::write(dir.join(EXPORT_CREDS_FILE), text).map_err(|e| e.to_string())?;
        wrote.push(EXPORT_CREDS_FILE.to_string());
        has_credentials = true;
    }
    let manifest = ExportManifest {
        format: EXPORT_FORMAT.into(),
        has_credentials,
    };
    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(dir.join(EXPORT_MANIFEST_FILE), manifest_json).map_err(|e| e.to_string())?;
    wrote.push(EXPORT_MANIFEST_FILE.to_string());
    Ok(wrote)
}

#[derive(Debug)]
pub struct PortableImport {
    pub sessions: Vec<SessionConfig>,
    pub vault: Option<Vec<Credential>>,
    pub notes: Vec<SyncNote>,
}

pub fn import_portable_pack(
    dir: &std::path::Path,
    sessions: &[SessionConfig],
    vault: &[Credential],
    passphrase: &str,
    recovery: &str,
    import_credentials: bool,
    zh: bool,
) -> Result<PortableImport, String> {
    let hosts_path = dir.join(EXPORT_HOSTS_FILE);
    let text = fs::read_to_string(&hosts_path).map_err(|e| format!("hosts.json: {e}"))?;
    let remote = parse_host_document(&text)?;
    let local = host_document_from_sessions(sessions);
    let merged = merge_hosts(&local, &remote);
    let mut notes = Vec::new();
    if merged.auto_newer > 0 {
        notes.push(SyncNote::HostAutoMerged {
            count: merged.auto_newer,
        });
    }
    for name in &merged.conflict_names {
        notes.push(SyncNote::HostConflict { name: name.clone() });
    }
    let creds_path = dir.join(EXPORT_CREDS_FILE);
    if import_credentials && creds_path.exists() {
        let env_text = fs::read_to_string(&creds_path).map_err(|e| e.to_string())?;
        let env = parse_envelope(&env_text)?;
        let remote_blob = open_credentials(&env, passphrase, recovery).map_err(|e| e.text(zh))?;
        let local_blob = local_credential_blob(sessions, vault, &HostSyncState::default(), 0);
        let creds = merge_credentials(&local_blob, &remote_blob, &[]);
        for label in &creds.tie_labels {
            notes.push(SyncNote::CredTie {
                label: label.clone(),
            });
        }
        let sessions = sessions_after_sync(sessions, &merged.document, Some(&creds.local_view));
        Ok(PortableImport {
            sessions,
            vault: Some(live_vault_credentials(&creds.local_view)),
            notes,
        })
    } else {
        Ok(PortableImport {
            sessions: sessions_after_sync(sessions, &merged.document, None),
            vault: None,
            notes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_seal(pass: &str, recovery: &str, blob: &SyncCredentialBlob) -> CloudCredentialEnvelope {
        let plain = serde_json::to_vec(blob).unwrap();
        seal_bytes(pass, recovery, &plain, 8, 1, 1).unwrap()
    }

    fn session(id: &str, name: &str, host: &str, updated: i64, password: &str) -> SessionConfig {
        let mut s = SessionConfig::default();
        s.id = id.into();
        s.name = name.into();
        s.host = host.into();
        s.username = "root".into();
        s.port = 22;
        s.group = "默认".into();
        s.password = password.into();
        s.private_key_path = "/home/me/.ssh/id_ed25519".into();
        s.updated_at = Some(updated);
        s.created_at = Some(1);
        s
    }

    fn record(id: &str, name: &str, updated: i64) -> HostRecord {
        let mut s = session(id, name, "10.0.0.1", updated, "secret-password");
        s.private_key_path = "/keys/id_ed25519".into();
        host_record_from_session(&s)
    }

    #[test]
    fn host_json_omits_password_and_key_material() {
        let mut s = session("a", "web", "10.1.1.1", 10, "s3cret-password");
        s.private_key_path = "/home/me/.ssh/id_ed25519".into();
        let doc = host_document_from_sessions(&[s]);
        let json = serde_json::to_string(&doc).unwrap();
        assert!(!json.contains("s3cret-password"));
        assert!(!json.contains("BEGIN OPENSSH PRIVATE KEY"));
        assert!(json.contains("10.1.1.1"));
        assert!(json.contains("/home/me/.ssh/id_ed25519"));
        assert!(json.contains("\"username\":\"root\""));
    }

    #[test]
    fn newer_updated_at_wins() {
        let local = HostDocument {
            schema: 1,
            hosts: vec![record("a", "local-name", 10)],
        };
        let mut remote_rec = record("a", "remote-name", 20);
        remote_rec.host = "10.9.9.9".into();
        let remote = HostDocument {
            schema: 1,
            hosts: vec![remote_rec],
        };
        let merged = merge_hosts(&local, &remote);
        assert_eq!(merged.document.hosts.len(), 1);
        assert_eq!(merged.document.hosts[0].name, "remote-name");
        assert_eq!(merged.auto_newer, 1);
        assert!(merged.conflict_names.is_empty());
    }

    #[test]
    fn equal_timestamp_keeps_conflict_copy() {
        let local = HostDocument {
            schema: 1,
            hosts: vec![record("a", "from-a", 5)],
        };
        let mut remote_rec = record("a", "from-b", 5);
        remote_rec.host = "10.2.2.2".into();
        let remote = HostDocument {
            schema: 1,
            hosts: vec![remote_rec],
        };
        let merged = merge_hosts(&local, &remote);
        assert_eq!(merged.conflict_names, vec!["from-a".to_string()]);
        assert_eq!(merged.document.hosts.len(), 2);
        assert!(merged.document.hosts.iter().any(|h| h.name == "from-a"));
        assert!(merged
            .document
            .hosts
            .iter()
            .any(|h| h.id == "a~conflict~5" && h.host == "10.2.2.2"));
    }

    #[test]
    fn newer_tombstone_removes_host_and_older_tombstone_does_not() {
        let mut live = record("a", "web", 10);
        live.deleted = false;
        let mut tomb = record("a", "web", 30);
        tomb.deleted = true;
        let merged = merge_hosts(
            &HostDocument {
                schema: 1,
                hosts: vec![live.clone()],
            },
            &HostDocument {
                schema: 1,
                hosts: vec![tomb],
            },
        );
        assert!(merged.document.hosts[0].deleted);

        let mut newer_live = record("a", "web", 40);
        newer_live.host = "10.8.8.8".into();
        let mut older_tomb = record("a", "web", 30);
        older_tomb.deleted = true;
        let merged = merge_hosts(
            &HostDocument {
                schema: 1,
                hosts: vec![newer_live],
            },
            &HostDocument {
                schema: 1,
                hosts: vec![older_tomb],
            },
        );
        assert!(!merged.document.hosts[0].deleted);
        assert_eq!(merged.document.hosts[0].host, "10.8.8.8");
    }

    #[test]
    fn local_delete_becomes_tombstone() {
        let previous = host_document_from_sessions(&[session("a", "web", "h", 4, "")]);
        let doc = local_host_document(&[], &previous, 9);
        assert_eq!(doc.hosts.len(), 1);
        assert!(doc.hosts[0].deleted);
        assert_eq!(doc.hosts[0].updated_at, 9);
    }

    #[test]
    fn metadata_apply_keeps_local_password() {
        let local = session("a", "old", "10.0.0.1", 1, "local-secret");
        let mut remote = record("a", "new", 8);
        remote.host = "10.9.9.9".into();
        let doc = HostDocument {
            schema: 1,
            hosts: vec![remote],
        };
        let out = sessions_after_sync(&[local], &doc, None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "new");
        assert_eq!(out[0].host, "10.9.9.9");
        assert_eq!(out[0].password, "local-secret");
    }

    #[test]
    fn sync_preserves_local_order_and_appends_new_hosts() {
        let a = session("a", "aaa", "h", 1, "p");
        let b = session("b", "bbb", "h", 1, "q");
        let mut remote = record("c", "ccc", 2);
        remote.host = "10.3.3.3".into();
        let mut updated_a = record("a", "aaa-new", 9);
        updated_a.host = "10.1.1.1".into();
        let doc = HostDocument {
            schema: 1,
            hosts: vec![updated_a, remote],
        };
        let out = sessions_after_sync(&[a, b], &doc, None);
        let ids: Vec<&str> = out.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
        assert_eq!(out[0].name, "aaa-new");
        assert_eq!(out[0].password, "p");
        assert_eq!(out[1].password, "q");
        assert_eq!(out[2].host, "10.3.3.3");
        assert!(out[2].password.is_empty());
    }

    #[test]
    fn envelope_roundtrip_and_wrong_passphrase_is_readable() {
        let blob = SyncCredentialBlob {
            schema: 1,
            session_secrets: vec![SyncSessionSecret {
                id: "a".into(),
                password: "pw-1".into(),
                updated_at: 3,
                deleted: false,
            }],
            vault: vec![],
        };
        let env = fast_seal("correct horse", "abcd-ef01", &blob);
        let opened = open_credentials(&env, "correct horse", "").unwrap();
        assert_eq!(opened.session_secrets[0].password, "pw-1");
        let err = open_credentials(&env, "nope", "").unwrap_err();
        assert_eq!(err, UnlockError::WrongPassphrase);
        assert!(err.text(true).contains("同步口令不正确"));
        assert!(err.text(true).contains("未改写"));
        let via_recovery = open_credentials(&env, "nope", "abcd-ef01").unwrap();
        assert_eq!(via_recovery.session_secrets[0].password, "pw-1");
        let no_recovery = CloudCredentialEnvelope {
            ciphertext: env.ciphertext.clone(),
            kdf_hint: {
                let mut hint: serde_json::Value = serde_json::from_str(&env.kdf_hint).unwrap();
                hint.as_object_mut().unwrap().remove("recovery_salt_b64");
                hint.as_object_mut()
                    .unwrap()
                    .remove("recovery_wrap_nonce_b64");
                hint.as_object_mut()
                    .unwrap()
                    .remove("recovery_wrapped_dek_b64");
                hint.to_string()
            },
        };
        assert_eq!(
            open_credentials(&no_recovery, "", "abcd-ef01").unwrap_err(),
            UnlockError::NoRecoveryCode
        );
        assert_eq!(
            open_credentials(&env, "", "ffff-ffff").unwrap_err(),
            UnlockError::WrongRecoveryCode
        );
        assert!(UnlockError::WrongRecoveryCode
            .text(true)
            .contains("恢复码不正确"));
    }

    #[test]
    fn device_key_is_not_the_unwrap_key() {
        let blob = SyncCredentialBlob {
            schema: 1,
            session_secrets: vec![SyncSessionSecret {
                id: "a".into(),
                password: "not-on-server".into(),
                updated_at: 1,
                deleted: false,
            }],
            vault: vec![],
        };
        let env = fast_seal("phrase", "", &blob);
        assert!(!env.ciphertext.contains("not-on-server"));
        assert!(!env.kdf_hint.contains("not-on-server"));
        let device = crate::security::device_key::derive_key_from_fingerprint("other-machine");
        let hint: serde_json::Value = serde_json::from_str(&env.kdf_hint).unwrap();
        let nonce = B64
            .decode(hint["payload_nonce_b64"].as_str().unwrap())
            .unwrap();
        let ct = B64.decode(&env.ciphertext).unwrap();
        assert!(aes_open(&device, &nonce, &ct).is_err());
        let value = serde_json::to_value(&env).unwrap();
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        assert_eq!(keys, vec!["ciphertext", "kdf_hint"]);
    }

    #[test]
    fn credential_tie_keeps_both_sides() {
        let local = SyncCredentialBlob {
            schema: 1,
            session_secrets: vec![SyncSessionSecret {
                id: "a".into(),
                password: "local".into(),
                updated_at: 4,
                deleted: false,
            }],
            vault: vec![],
        };
        let remote = SyncCredentialBlob {
            schema: 1,
            session_secrets: vec![SyncSessionSecret {
                id: "a".into(),
                password: "remote".into(),
                updated_at: 4,
                deleted: false,
            }],
            vault: vec![],
        };
        let merged = merge_credentials(&local, &remote, &[]);
        assert_eq!(merged.tie_labels, vec!["a".to_string()]);
        assert_eq!(merged.local_view.session_secrets[0].password, "local");
        assert_eq!(merged.upload_view.session_secrets[0].password, "remote");
    }

    #[test]
    fn newer_password_wins() {
        let local = SyncCredentialBlob {
            schema: 1,
            session_secrets: vec![SyncSessionSecret {
                id: "a".into(),
                password: "old".into(),
                updated_at: 1,
                deleted: false,
            }],
            vault: vec![],
        };
        let remote = SyncCredentialBlob {
            schema: 1,
            session_secrets: vec![SyncSessionSecret {
                id: "a".into(),
                password: "new".into(),
                updated_at: 9,
                deleted: false,
            }],
            vault: vec![],
        };
        let merged = merge_credentials(&local, &remote, &[]);
        assert!(merged.tie_labels.is_empty());
        assert_eq!(merged.local_view.session_secrets[0].password, "new");
        assert_eq!(merged.upload_view.session_secrets[0].password, "new");
    }

    #[test]
    fn parse_wrapped_conflict_and_etag() {
        let body = r#"{"error":"conflict","etag":"rev-2","document":{"schema":1,"hosts":[{"id":"a","name":"n","host":"h","port":22,"username":"u","updated_at":3}]}}"#;
        let doc = parse_host_document(body).unwrap();
        assert_eq!(doc.hosts[0].id, "a");
        assert_eq!(etag_from_body(body).as_deref(), Some("rev-2"));
        assert_eq!(format_if_match("rev-2"), "\"rev-2\"");
        assert_eq!(format_if_match("\"rev-2\""), "\"rev-2\"");
        let resp = RawSyncResponse {
            status: 413,
            etag: None,
            body: "too big".into(),
        };
        assert!(matches!(read_hosts(resp), HostsRead::TooLarge));
        let missing = RawSyncResponse {
            status: 404,
            etag: None,
            body: String::new(),
        };
        match read_hosts(missing) {
            HostsRead::Ready { doc, etag } => {
                assert!(doc.hosts.is_empty());
                assert!(etag.is_none());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn portable_export_roundtrip_without_device_key() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = vec![session("a", "web", "10.0.0.8", 4, "pack-secret")];
        let wrote = write_export_files(
            dir.path(),
            &sessions,
            &[],
            true,
            "pack-phrase",
            "recover",
            true,
        )
        .unwrap();
        assert!(wrote.iter().any(|f| f == "hosts.json"));
        assert!(wrote.iter().any(|f| f == "credentials.envelope.json"));
        let hosts_text = fs::read_to_string(dir.path().join("hosts.json")).unwrap();
        assert!(!hosts_text.contains("pack-secret"));
        let imported =
            import_portable_pack(dir.path(), &[], &[], "pack-phrase", "", true, true).unwrap();
        assert_eq!(imported.sessions.len(), 1);
        assert_eq!(imported.sessions[0].host, "10.0.0.8");
        assert_eq!(imported.sessions[0].password, "pack-secret");
        let bad = import_portable_pack(dir.path(), &[], &[], "wrong", "", true, true).unwrap_err();
        assert!(bad.contains("同步口令不正确"));
    }

    #[test]
    fn password_change_bumps_stamp_but_metadata_edit_does_not() {
        let s = session("a", "web", "h", 50, "same");
        let state = HostSyncState {
            session_secret_stamps: vec![SecretStamp {
                id: "a".into(),
                updated_at: 3,
                fingerprint: fingerprint("same"),
            }],
            ..HostSyncState::default()
        };
        let blob = local_credential_blob(&[s.clone()], &[], &state, 80);
        assert_eq!(blob.session_secrets[0].updated_at, 3);
        let mut changed = s;
        changed.password = "edited".into();
        let blob = local_credential_blob(&[changed], &[], &state, 80);
        assert_eq!(blob.session_secrets[0].updated_at, 80);
        assert_eq!(blob.session_secrets[0].password, "edited");
    }
}
