//! Personal cross-device host/credential sync client (#178).
//!
//! Contract: mist-team-server `docs/sync.md` (#25, merged 2026-10-29):
//! - `GET/PUT /v1/sync/hosts` — plaintext metadata snapshot (opaque JSON,
//!   no secrets: passwords / key material / sync passphrase / recovery
//!   code are forbidden by contract).
//! - `GET/PUT /v1/sync/credentials` — optional client-encrypted envelope
//!   (`ciphertext` + `kdf_hint`); the server never holds any key.
//! - Concurrency: strong ETag (decimal string, starts at 1). PUT sends
//!   `If-Match`; absent snapshot = revision 0 (first PUT may omit the
//!   header or send `"0"`). Wrong/missing precondition with a snapshot
//!   present → `409` with the server copy; client merges and retries.
//! - Limits: 4 MiB payload → 413. 404 = no snapshot yet.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Sync API base paths (mist-team-server #25 contract).
pub const HOSTS_PATH: &str = "/v1/sync/hosts";
pub const CREDENTIALS_PATH: &str = "/v1/sync/credentials";

/// Server snapshot envelope for GET responses (both kinds share the shape).
#[derive(Debug, Clone, Deserialize)]
pub struct SyncGetResponse {
    /// hosts / credentials document as stored (verbatim from the last PUT).
    #[serde(default)]
    pub blob: Option<Value>,
    #[serde(default)]
    pub revision: Option<i64>,
    #[serde(default)]
    pub updated_at: Option<String>,
    // credentials only
    #[serde(default)]
    pub kdf_hint: Option<String>,
}

/// Conflict payload: 409 carries the server's current copy.
#[derive(Debug, Clone, Deserialize)]
pub struct SyncConflictResponse {
    #[serde(default)]
    pub blob: Option<Value>,
    #[serde(default)]
    pub revision: Option<i64>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub kdf_hint: Option<String>,
}

/// Result of a fetch: either a snapshot or "nothing stored yet".
#[derive(Debug, Clone)]
pub enum SyncFetch {
    Snapshot {
        blob: Value,
        revision: i64,
        updated_at: Option<String>,
        kdf_hint: Option<String>,
    },
    /// 404: server has no snapshot for this user/kind yet.
    Empty,
}

impl std::fmt::Display for SyncFetch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncFetch::Snapshot { revision, .. } => write!(f, "snapshot(rev={revision})"),
            SyncFetch::Empty => write!(f, "empty"),
        }
    }
}

/// Host metadata document (client-side contract; server treats the whole
/// PUT body as opaque). No password/key material ever enters this struct.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SyncHostEntry {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub group: String,
    /// Auth-method hint, e.g. "password" | "key_ref" | "agent" — never the
    /// credential itself.
    #[serde(default)]
    pub auth: String,
    /// Key path or vault reference (`key://…`), not private-key content.
    #[serde(default)]
    pub key_ref: String,
    #[serde(default)]
    pub updated_at: Option<String>,
}

fn default_ssh_port() -> u16 {
    22
}

/// Envelope PUT for /v1/sync/credentials (server rejects extra fields).
#[derive(Debug, Clone, Serialize)]
pub struct CredentialsPut<'a> {
    pub ciphertext: &'a str,
    pub kdf_hint: &'a str,
}

/// One sync step's outcome for the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncOutcome {
    /// Pushed successfully; server returned this revision.
    Pushed { revision: i64 },
    /// Pulled a snapshot newer than local; caller applies it.
    Pulled { revision: i64 },
    /// In sync: server revision == expected, nothing to do.
    UpToDate,
    /// 409: caller must resolve (see `SyncConflictResponse`).
    Conflict {
        server_revision: i64,
        server_blob: Option<Value>,
    },
}

/// Non-network helpers (pure, unit-testable) ─────────────────────────────

/// Build the hosts document from local entries.
pub fn hosts_document(entries: &[SyncHostEntry]) -> Value {
    serde_json::to_value(entries).unwrap_or(Value::Array(Vec::new()))
}

/// Parse a hosts snapshot back into entries; tolerant of shape drift
/// (document may be an array or `{hosts: [...]}` — both were valid PUTs).
pub fn parse_hosts_document(blob: &Value) -> Vec<SyncHostEntry> {
    let arr = match blob {
        Value::Array(a) => a.clone(),
        Value::Object(o) => match o.get("hosts") {
            Some(Value::Array(a)) => a.clone(),
            _ => return Vec::new(),
        },
        _ => return Vec::new(),
    };
    arr.into_iter()
        .filter_map(|v| serde_json::from_value::<SyncHostEntry>(v).ok())
        .collect()
}

/// ETag header value for a known revision (`"N"` quoted strong form).
pub fn if_match_header(revision: i64) -> String {
    format!("\"{revision}\"")
}

/// Extract the decimal revision from a server ETag header.
/// Accepts `"3"` and `3`; rejects `*`, weak `W/"3"`, lists — those are
/// server-side errors per contract, so we surface them as invalid (None).
pub fn parse_etag(etag: &str) -> Option<i64> {
    let t = etag.trim();
    if t == "*" || t.starts_with("W/") || t.contains(',') {
        return None;
    }
    let unquoted = t.trim_matches('"');
    unquoted.parse::<i64>().ok()
}

/// Should the client send If-Match for this PUT? First PUT against an
/// absent snapshot (revision 0) may omit it; anything else must carry it.
pub fn should_send_if_match(local_revision: i64) -> bool {
    local_revision > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_document_roundtrip_array_shape() {
        let entries = vec![SyncHostEntry {
            id: "h1".into(),
            name: "web".into(),
            host: "10.0.0.8".into(),
            port: 22,
            user: "ops".into(),
            group: "prod".into(),
            auth: "key_ref".into(),
            key_ref: "key://local/id_ed25519".into(),
            updated_at: None,
        }];
        let doc = hosts_document(&entries);
        let parsed = parse_hosts_document(&doc);
        assert_eq!(parsed, entries);
    }

    #[test]
    fn parse_hosts_document_object_shape() {
        let doc: Value = serde_json::json!({ "hosts": [
            {"id": "h2", "host": "db1", "port": 2222}
        ]});
        let parsed = parse_hosts_document(&doc);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "h2");
        assert_eq!(parsed[0].port, 2222);
        assert_eq!(parsed[0].user, ""); // serde default
    }

    #[test]
    fn parse_hosts_document_garbage_is_empty_not_panic() {
        assert!(parse_hosts_document(&serde_json::json!("str")).is_empty());
        assert!(parse_hosts_document(&serde_json::json!({"hosts": 5})).is_empty());
        assert!(parse_hosts_document(&serde_json::json!(null)).is_empty());
    }

    #[test]
    fn parse_hosts_document_skips_invalid_rows() {
        let doc: Value = serde_json::json!([
            {"id": "ok", "host": "h"},
            {"no_id": true},
            {"id": 42, "host": "x"}
        ]);
        let parsed = parse_hosts_document(&doc);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "ok");
    }

    #[test]
    fn if_match_quoted_strong_form() {
        assert_eq!(if_match_header(3), "\"3\"");
        assert_eq!(if_match_header(0), "\"0\"");
    }

    #[test]
    fn parse_etag_accepts_quoted_and_bare() {
        assert_eq!(parse_etag("\"3\""), Some(3));
        assert_eq!(parse_etag("3"), Some(3));
        assert_eq!(parse_etag("  \"12\"  "), Some(12));
    }

    #[test]
    fn parse_etag_rejects_star_weak_and_lists() {
        assert_eq!(parse_etag("*"), None);
        assert_eq!(parse_etag("W/\"3\""), None);
        assert_eq!(parse_etag("\"1\", \"2\""), None);
        assert_eq!(parse_etag(""), None);
        assert_eq!(parse_etag("abc"), None);
    }

    #[test]
    fn first_put_omits_if_match() {
        assert!(!should_send_if_match(0));
        assert!(should_send_if_match(1));
        assert!(should_send_if_match(99));
    }

    #[test]
    fn sync_fetch_display_is_stable() {
        let s = SyncFetch::Snapshot {
            blob: serde_json::json!(null),
            revision: 2,
            updated_at: None,
            kdf_hint: None,
        };
        assert_eq!(s.to_string(), "snapshot(rev=2)");
        assert_eq!(SyncFetch::Empty.to_string(), "empty");
    }
}
