//! OpenSSH FIDO2 / security-key authentication (`ecdsa-sk` / `ed25519-sk`).
//!
//! Signing stays in the system OpenSSH agent (`ssh-agent` / Windows OpenSSH
//! Authentication Agent). MistTerm does not implement FIDO or WebAuthn.
//! Touch and PIN prompts are the agent's (or the key's); this module only
//! selects sk identities and turns agent failures into readable errors.

use std::path::{Path, PathBuf};

use base64::Engine;
use ssh2::Session;

use super::client::SshConfig;
use crate::core::session::SessionAuthMode;

/// How long to wait for a touch or system PIN prompt before failing the attempt.
pub const FIDO2_USER_PRESENCE_TIMEOUT_MS: u32 = 120_000;

pub const FIDO2_AGENT_UNAVAILABLE: &str = "FIDO2_AGENT_UNAVAILABLE";
pub const FIDO2_NO_SECURITY_KEY: &str = "FIDO2_NO_SECURITY_KEY";
pub const FIDO2_KEY_FILE_MISSING: &str = "FIDO2_KEY_FILE_MISSING";
pub const FIDO2_TOUCH_CANCELLED: &str = "FIDO2_TOUCH_CANCELLED";
pub const FIDO2_PIN_INCORRECT: &str = "FIDO2_PIN_INCORRECT";
pub const FIDO2_SIGN_FAILED: &str = "FIDO2_SIGN_FAILED";

/// Agent identity viewed as an OpenSSH public key blob plus comment.
#[derive(Debug, Clone, Copy)]
pub struct AgentIdentityRef<'a> {
    pub comment: &'a str,
    pub blob: &'a [u8],
}

pub fn fido2_err(code: &str, detail: impl std::fmt::Display) -> String {
    format!("{code}: {detail}")
}

pub fn is_openssh_sk_algorithm(alg: &str) -> bool {
    alg == "sk-ssh-ed25519@openssh.com"
        || alg == "sk-ecdsa-sha2-nistp256@openssh.com"
        || alg.starts_with("sk-")
}

/// First SSH wire string in a public key blob (the algorithm name).
pub fn openssh_pubkey_algorithm(blob: &[u8]) -> Option<&str> {
    let (raw, _) = ssh_wire_string(blob)?;
    std::str::from_utf8(raw).ok()
}

pub fn is_sk_pubkey_blob(blob: &[u8]) -> bool {
    openssh_pubkey_algorithm(blob).is_some_and(is_openssh_sk_algorithm)
}

fn ssh_wire_string(buf: &[u8]) -> Option<(&[u8], &[u8])> {
    if buf.len() < 4 {
        return None;
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let rest = buf.get(4..)?;
    if rest.len() < len {
        return None;
    }
    Some((&rest[..len], &rest[len..]))
}

/// Decode an OpenSSH `*.pub` line (`alg base64 comment`) into the wire blob.
pub fn openssh_authorized_key_blob(line: &str) -> Option<Vec<u8>> {
    let line = line.lines().next()?.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut parts = line.split_whitespace();
    let alg = parts.next()?;
    let b64 = parts.next()?;
    if !is_openssh_sk_algorithm(alg)
        && !alg.starts_with("ssh-")
        && !alg.starts_with("ecdsa-")
    {
        return None;
    }
    let engine = base64::engine::general_purpose::STANDARD;
    let blob = engine
        .decode(b64)
        .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(b64))
        .ok()?;
    if openssh_pubkey_algorithm(&blob).is_none() {
        return None;
    }
    Some(blob)
}

pub fn expand_user_path(path: &str) -> PathBuf {
    let path = path.trim();
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

fn pubkey_sibling(path: &Path) -> PathBuf {
    if path.extension().and_then(|e| e.to_str()) == Some("pub") {
        path.to_path_buf()
    } else {
        PathBuf::from(format!("{}.pub", path.display()))
    }
}

fn read_sk_pubkey_blob(private_or_pub: &Path) -> Result<Option<Vec<u8>>, String> {
    let pub_path = pubkey_sibling(private_or_pub);
    if !pub_path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&pub_path).map_err(|e| {
        fido2_err(
            FIDO2_KEY_FILE_MISSING,
            format!("could not read {}: {e}", pub_path.display()),
        )
    })?;
    let Some(blob) = openssh_authorized_key_blob(&text) else {
        return Err(fido2_err(
            FIDO2_NO_SECURITY_KEY,
            format!(
                "{} is not an OpenSSH public key",
                pub_path.display()
            ),
        ));
    };
    if !is_sk_pubkey_blob(&blob) {
        return Err(fido2_err(
            FIDO2_NO_SECURITY_KEY,
            format!(
                "{} is not an ecdsa-sk or ed25519-sk key",
                pub_path.display()
            ),
        ));
    }
    Ok(Some(blob))
}

/// `ssh-add` stores the path it was given as the identity comment.
pub fn comment_matches_key_path(comment: &str, key_path: &str) -> bool {
    let comment = comment.trim();
    let key_path = key_path.trim();
    if comment.is_empty() || key_path.is_empty() {
        return false;
    }
    if comment == key_path {
        return true;
    }
    let file_name = key_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(key_path);
    if file_name.is_empty() {
        return false;
    }
    comment == file_name
        || comment.ends_with(&format!("/{file_name}"))
        || comment.ends_with(&format!("\\{file_name}"))
}

/// Indices of sk identities to try. A requested key that is absent yields an empty list
/// (do not fall through to a different key).
pub fn select_sk_identity_indices(
    identities: &[AgentIdentityRef<'_>],
    wanted_blob: Option<&[u8]>,
    wanted_path: Option<&str>,
) -> Vec<usize> {
    let sk: Vec<usize> = identities
        .iter()
        .enumerate()
        .filter(|(_, id)| is_sk_pubkey_blob(id.blob))
        .map(|(i, _)| i)
        .collect();
    if let Some(blob) = wanted_blob {
        let exact: Vec<usize> = sk
            .iter()
            .copied()
            .filter(|&i| identities[i].blob == blob)
            .collect();
        if !exact.is_empty() {
            return exact;
        }
    }
    if let Some(path) = wanted_path.map(str::trim).filter(|p| !p.is_empty()) {
        return sk
            .into_iter()
            .filter(|&i| comment_matches_key_path(identities[i].comment, path))
            .collect();
    }
    sk
}

/// Map a signature failure from the agent / ssh-sk-helper onto a stable code.
///
/// The agent protocol often returns only "agent refused operation". That stays
/// [`FIDO2_SIGN_FAILED`] so the UI can name touch-cancel, bad PIN, and a missing
/// key without claiming a cause the agent did not report.
pub fn classify_fido2_sign_failure(err: &str) -> &'static str {
    let l = err.to_lowercase();
    if pin_rejected(&l) {
        return FIDO2_PIN_INCORRECT;
    }
    if touch_cancelled(&l) {
        return FIDO2_TOUCH_CANCELLED;
    }
    if key_or_device_missing(&l) {
        return FIDO2_NO_SECURITY_KEY;
    }
    FIDO2_SIGN_FAILED
}

fn pin_rejected(l: &str) -> bool {
    l.contains("incorrect pin")
        || l.contains("invalid pin")
        || l.contains("wrong pin")
        || l.contains("bad pin")
        || l.contains("pin invalid")
        || l.contains("pin incorrect")
        || l.contains("fido_err_pin")
        || l.contains("pin_auth_blocked")
        || l.contains("pin auth blocked")
        || l.contains("ssh_sk_err_pin")
}

fn touch_cancelled(l: &str) -> bool {
    l.contains("cancel")
        || l.contains("user refused")
        || l.contains("refused authorization")
        || l.contains("presence")
        || l.contains("timed out")
        || l.contains("timeout")
        || l.contains("action_timeout")
        || l.contains("keepalive_cancel")
}

fn key_or_device_missing(l: &str) -> bool {
    l.contains("no device")
        || l.contains("device not found")
        || l.contains("couldn't find device")
        || l.contains("could not find device")
        || l.contains("couldnt find device")
        || l.contains("invalid format")
        || l.contains("key handle")
        || l.contains("no identities")
        || l.contains("no security key")
        || l.contains("not loaded")
}

struct RestoreTimeout {
    session: Session,
    prev_ms: u32,
}

impl Drop for RestoreTimeout {
    fn drop(&mut self) {
        self.session.set_timeout(self.prev_ms);
    }
}

/// Authenticate with sk keys already loaded in the system OpenSSH agent.
///
/// Does not try a password, a non-sk file key, or `userauth_agent` (which would
/// accept any key the agent offers). Failure is always an error.
pub fn authenticate_fido2(session: &Session, config: &SshConfig) -> Result<(), String> {
    if config.auth_mode != SessionAuthMode::Fido2SecurityKey {
        return Err(fido2_err(
            FIDO2_SIGN_FAILED,
            "internal: FIDO2 authenticator called for a non-FIDO2 session",
        ));
    }
    let username = config.username.trim();
    if username.is_empty() {
        return Err(fido2_err(
            FIDO2_SIGN_FAILED,
            "username is required for security-key login",
        ));
    }

    let requested = config.private_key_path.trim();
    let expanded = if requested.is_empty() {
        None
    } else {
        Some(expand_user_path(requested))
    };
    if let Some(path) = &expanded {
        let pub_path = pubkey_sibling(path);
        if !path.is_file() && !pub_path.is_file() {
            return Err(fido2_err(
                FIDO2_KEY_FILE_MISSING,
                format!(
                    "security key file not found: {} (also looked for {})",
                    path.display(),
                    pub_path.display()
                ),
            ));
        }
    }

    let wanted_blob = if let Some(path) = &expanded {
        read_sk_pubkey_blob(path)?
    } else {
        None
    };
    let wanted_path = expanded.as_ref().map(|p| p.display().to_string());

    let _timeout = RestoreTimeout {
        session: session.clone(),
        prev_ms: session.timeout(),
    };
    session.set_timeout(FIDO2_USER_PRESENCE_TIMEOUT_MS);
    log::info!(
        "FIDO2 security key auth for {username}: waiting for a touch or the system PIN prompt"
    );

    let mut agent = session.agent().map_err(|e| {
        fido2_err(
            FIDO2_AGENT_UNAVAILABLE,
            format!("SSH agent is not available ({e})"),
        )
    })?;
    agent.connect().map_err(|e| {
        fido2_err(
            FIDO2_AGENT_UNAVAILABLE,
            format!("could not connect to the SSH agent ({e})"),
        )
    })?;
    agent.list_identities().map_err(|e| {
        fido2_err(
            FIDO2_AGENT_UNAVAILABLE,
            format!("could not list SSH agent identities ({e})"),
        )
    })?;
    let identities = agent.identities().map_err(|e| {
        fido2_err(
            FIDO2_AGENT_UNAVAILABLE,
            format!("could not read SSH agent identities ({e})"),
        )
    })?;

    let refs: Vec<AgentIdentityRef<'_>> = identities
        .iter()
        .map(|key| AgentIdentityRef {
            comment: key.comment(),
            blob: key.blob(),
        })
        .collect();
    let idxs = select_sk_identity_indices(
        &refs,
        wanted_blob.as_deref(),
        wanted_path.as_deref(),
    );
    if idxs.is_empty() {
        let detail = if let Some(path) = &wanted_path {
            format!(
                "no ecdsa-sk or ed25519-sk key matching {path} is loaded in the SSH agent"
            )
        } else {
            "no ecdsa-sk or ed25519-sk key is loaded in the SSH agent".to_string()
        };
        return Err(fido2_err(FIDO2_NO_SECURITY_KEY, detail));
    }

    let mut last = String::from("agent refused operation");
    for i in idxs {
        let comment = identities[i].comment();
        let alg = openssh_pubkey_algorithm(identities[i].blob()).unwrap_or("sk");
        log::info!("FIDO2 offering {alg} from SSH agent ({comment})");
        match agent.userauth(username, &identities[i]) {
            Ok(()) if session.authenticated() => {
                log::info!("FIDO2 security key authentication succeeded");
                return Ok(());
            }
            Ok(()) => {
                last = "agent reported success but the session is not authenticated".to_string();
                log::warn!("FIDO2 agent userauth returned Ok without authenticated session");
            }
            Err(e) => {
                last = e.to_string();
                log::warn!("FIDO2 security key sign failed: {last}");
            }
        }
    }
    let code = classify_fido2_sign_failure(&last);
    Err(fido2_err(code, last))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire_string(s: &str) -> Vec<u8> {
        let b = s.as_bytes();
        let mut v = (b.len() as u32).to_be_bytes().to_vec();
        v.extend_from_slice(b);
        v
    }

    fn sk_blob(alg: &str) -> Vec<u8> {
        let mut v = wire_string(alg);
        v.extend_from_slice(&wire_string("key-handle"));
        v
    }

    #[test]
    fn detects_sk_algorithms_and_ignores_plain_keys() {
        let sk = sk_blob("sk-ssh-ed25519@openssh.com");
        assert_eq!(
            openssh_pubkey_algorithm(&sk),
            Some("sk-ssh-ed25519@openssh.com")
        );
        assert!(is_sk_pubkey_blob(&sk));
        let ecdsa = sk_blob("sk-ecdsa-sha2-nistp256@openssh.com");
        assert!(is_sk_pubkey_blob(&ecdsa));
        let plain = sk_blob("ssh-ed25519");
        assert!(!is_sk_pubkey_blob(&plain));
        assert!(!is_sk_pubkey_blob(&[]));
    }

    #[test]
    fn parses_authorized_key_line() {
        let blob = sk_blob("sk-ssh-ed25519@openssh.com");
        let b64 = base64::engine::general_purpose::STANDARD.encode(&blob);
        let line = format!("sk-ssh-ed25519@openssh.com {b64} user@host");
        let decoded = openssh_authorized_key_blob(&line).unwrap();
        assert_eq!(decoded, blob);
        let plain = wire_string("ssh-ed25519");
        let plain_b64 = base64::engine::general_purpose::STANDARD.encode(&plain);
        let plain_line = format!("ssh-ed25519 {plain_b64} comment");
        let plain_blob = openssh_authorized_key_blob(&plain_line).unwrap();
        assert!(!is_sk_pubkey_blob(&plain_blob));
    }

    #[test]
    fn selects_only_sk_identities() {
        let sk = sk_blob("sk-ssh-ed25519@openssh.com");
        let other = sk_blob("sk-ecdsa-sha2-nistp256@openssh.com");
        let plain = sk_blob("ssh-ed25519");
        let ids = [
            AgentIdentityRef {
                comment: "plain",
                blob: &plain,
            },
            AgentIdentityRef {
                comment: "/home/a/.ssh/id_ed25519_sk",
                blob: &sk,
            },
            AgentIdentityRef {
                comment: "ecdsa",
                blob: &other,
            },
        ];
        assert_eq!(select_sk_identity_indices(&ids, None, None), vec![1, 2]);
        assert_eq!(select_sk_identity_indices(&ids, Some(&sk), None), vec![1]);
        assert_eq!(
            select_sk_identity_indices(&ids, None, Some("/home/a/.ssh/id_ed25519_sk")),
            vec![1]
        );
        assert!(
            select_sk_identity_indices(&ids, None, Some("/home/a/.ssh/missing_sk")).is_empty(),
            "a named key must not fall through to a different sk identity"
        );
        let mut wrong_blob = sk.clone();
        wrong_blob.push(9);
        assert_eq!(
            select_sk_identity_indices(&ids, Some(wrong_blob.as_slice()), Some("id_ed25519_sk")),
            vec![1],
            "if the .pub blob does not match the agent, still honor the filename comment"
        );
    }

    #[test]
    fn comment_match_accepts_filename_suffix() {
        assert!(comment_matches_key_path(
            r"C:\Users\a\.ssh\id_ecdsa_sk",
            r"C:\Users\a\.ssh\id_ecdsa_sk"
        ));
        assert!(comment_matches_key_path(
            "/home/a/.ssh/id_ed25519_sk",
            "id_ed25519_sk"
        ));
        assert!(!comment_matches_key_path("id_ed25519", "id_ed25519_sk"));
        assert!(!comment_matches_key_path("", "id_ed25519_sk"));
    }

    #[test]
    fn classifies_pin_touch_missing_and_generic_refusal() {
        assert_eq!(
            classify_fido2_sign_failure(
                "sign_and_send_pubkey: signing failed for ED25519-SK: incorrect PIN"
            ),
            FIDO2_PIN_INCORRECT
        );
        assert_eq!(
            classify_fido2_sign_failure("FIDO_ERR_PIN_INVALID"),
            FIDO2_PIN_INCORRECT
        );
        assert_eq!(
            classify_fido2_sign_failure("user presence check timed out"),
            FIDO2_TOUCH_CANCELLED
        );
        assert_eq!(
            classify_fido2_sign_failure("user refused authorization"),
            FIDO2_TOUCH_CANCELLED
        );
        assert_eq!(
            classify_fido2_sign_failure(
                "ssh_sk_sign: couldn't find device for key handle: invalid format"
            ),
            FIDO2_NO_SECURITY_KEY
        );
        assert_eq!(
            classify_fido2_sign_failure(
                "sign_and_send_pubkey: signing failed from agent: agent refused operation"
            ),
            FIDO2_SIGN_FAILED
        );
    }
}
