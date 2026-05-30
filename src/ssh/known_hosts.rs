//! SSH 主机密钥信任（`~/.config/mistterm/known_hosts`）。

use ssh2::Session;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

pub fn known_hosts_path() -> PathBuf {
    let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("mistterm");
    p.push("known_hosts");
    p
}

fn host_key_line(host: &str, port: u16, fingerprint: &str) -> String {
    format!("{host}:{port} {fingerprint}\n")
}

fn read_entries() -> Vec<(String, u16, String)> {
    let path = known_hosts_path();
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((hostport, fp)) = line.split_once(' ') else {
            continue;
        };
        let fp = fp.trim().to_string();
        if let Some((host, port_str)) = hostport.rsplit_once(':') {
            if let Ok(port) = port_str.parse::<u16>() {
                out.push((host.to_string(), port, fp));
                continue;
            }
        }
        out.push((hostport.to_string(), 22, fp));
    }
    out
}

fn fingerprint_sha256(session: &Session) -> Result<String, String> {
    let hash = session
        .host_key_hash(ssh2::HashType::Sha256)
        .ok_or_else(|| "server did not provide host key".to_string())?;
    Ok(format!("SHA256:{}", base64_encode(hash)))
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(if i + 1 < bytes.len() {
            TABLE[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if i + 2 < bytes.len() {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
        i += 3;
    }
    out
}

fn append_entry(host: &str, port: u16, fingerprint: &str) -> Result<(), String> {
    let path = known_hosts_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("known_hosts write: {}", e))?;
    file.write_all(host_key_line(host, port, fingerprint).as_bytes())
        .map_err(|e| e.to_string())
}

/// 握手成功后校验主机密钥；未知主机自动信任并写入文件。
pub fn verify_or_record_host_key(session: &Session, host: &str, port: u16) -> Result<(), String> {
    let fp = fingerprint_sha256(session)?;
    let key = (host.to_string(), port);
    for (h, p, stored) in read_entries() {
        if h == key.0 && p == key.1 {
            if stored == fp {
                return Ok(());
            }
            return Err(format!(
                "Host key changed for {}:{} (expected {}, got {}). Refusing to connect.",
                host, port, stored, fp
            ));
        }
    }
    log::info!("Trusting new host key for {}:{} ({})", host, port, fp);
    append_entry(host, port, &fp)
}
