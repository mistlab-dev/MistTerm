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

/// Machine-parseable host-key-changed error (`HOST_KEY_CHANGED|host|port|expected|got`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyChanged {
    pub host: String,
    pub port: u16,
    pub expected: String,
    pub got: String,
}

fn host_key_changed_error(host: &str, port: u16, expected: &str, got: &str) -> String {
    format!("HOST_KEY_CHANGED|{host}|{port}|{expected}|{got}")
}

/// Parse a connect-stage error into a host-key-changed offer, if any.
pub fn parse_host_key_changed(message: &str) -> Option<HostKeyChanged> {
    let s = message.trim();
    if let Some(rest) = s.strip_prefix("HOST_KEY_CHANGED|") {
        let parts: Vec<&str> = rest.splitn(4, '|').collect();
        if parts.len() == 4 {
            if let Ok(port) = parts[1].parse::<u16>() {
                return Some(HostKeyChanged {
                    host: parts[0].to_string(),
                    port,
                    expected: parts[2].to_string(),
                    got: parts[3].to_string(),
                });
            }
        }
    }
    // Legacy English protocol text (pre-#243) still seen in logs / older builds.
    let lower = s.to_lowercase();
    if !lower.contains("host key changed") {
        return None;
    }
    // "Host key changed for HOST:PORT (expected EXP, got GOT). ..."
    let after = s.split("for ").nth(1)?;
    let (hostport, rest) = after.split_once(" (expected ")?;
    let (host, port_str) = hostport.rsplit_once(':')?;
    let port = port_str.parse::<u16>().ok()?;
    let (expected, rest) = rest.split_once(", got ")?;
    let got = rest
        .trim_end_matches('.')
        .split(')')
        .next()?
        .trim()
        .to_string();
    Some(HostKeyChanged {
        host: host.to_string(),
        port,
        expected: expected.trim().to_string(),
        got,
    })
}

/// Replace the stored fingerprint for `host:port` (or append if missing).
/// Used after the user explicitly trusts a *changed* host key in the GUI.
pub fn replace_host_key(host: &str, port: u16, fingerprint: &str) -> Result<(), String> {
    let path = known_hosts_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut entries = read_entries();
    entries.retain(|(h, p, _)| !(h == host && *p == port));
    entries.push((host.to_string(), port, fingerprint.to_string()));
    let mut body = String::new();
    for (h, p, fp) in &entries {
        body.push_str(&host_key_line(h, *p, fp));
    }
    // `known_hosts` has no real extension; avoid Path::with_extension stripping "hosts".
    let tmp = path.with_file_name("known_hosts.tmp");
    fs::write(&tmp, body.as_bytes()).map_err(|e| format!("known_hosts write: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("known_hosts replace: {e}"))?;
    log::info!("Replaced trusted host key for {host}:{port} ({fingerprint})");
    Ok(())
}

/// 握手成功后校验主机密钥；未知主机自动信任并写入文件。
/// 指纹变更时返回可解析的 `HOST_KEY_CHANGED|...` 错误（默认拒绝；GUI 可提供「信任并继续」）。
pub fn verify_or_record_host_key(session: &Session, host: &str, port: u16) -> Result<(), String> {
    let fp = fingerprint_sha256(session)?;
    let key = (host.to_string(), port);
    for (h, p, stored) in read_entries() {
        if h == key.0 && p == key.1 {
            if stored == fp {
                return Ok(());
            }
            return Err(host_key_changed_error(host, port, &stored, &fp));
        }
    }
    log::info!("Trusting new host key for {}:{} ({})", host, port, fp);
    append_entry(host, port, &fp)
}

/// Parse a `known_hosts` file *content* string into `(host, port, fingerprint)` tuples.
/// Mirrors the real `read_entries` parser so tests can exercise it without touching the disk.
fn parse_known_hosts_content(text: &str) -> Vec<(String, u16, String)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------ base64_encode

    #[test]
    fn base64_encode_empty_input() {
        assert_eq!(base64_encode(&[]), "");
    }

    #[test]
    fn base64_encode_one_byte_no_padding() {
        // 0x4D = 01001101 -> in 3-byte group padded: 0x4D 00 00 -> T A = =
        assert_eq!(base64_encode(&[0x4D]), "TQ==");
    }

    #[test]
    fn base64_encode_two_bytes_one_padding() {
        // 0x4D 0x61 = Man
        assert_eq!(base64_encode(b"Ma"), "TWE=");
    }

    #[test]
    fn base64_encode_three_bytes_no_padding() {
        assert_eq!(base64_encode(b"Man"), "TWFu");
    }

    #[test]
    fn base64_encode_standard_vectors() {
        // Well-known RFC vectors.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_encode_binary_with_symbols() {
        // Bytes which exercise the '+' and '/' chars.
        assert_eq!(base64_encode(&[0xFB, 0xFF, 0xFE]), "+//+");
        assert_eq!(base64_encode(&[0x00, 0x00, 0x00]), "AAAA");
    }

    // ------------------------------------------------------------------ host_key_line

    #[test]
    fn host_key_line_formats_host_port_fingerprint() {
        assert_eq!(
            host_key_line("srv", 2222, "SHA256:abcd"),
            "srv:2222 SHA256:abcd\n"
        );
    }

    #[test]
    fn host_key_line_default_port() {
        assert_eq!(host_key_line("host", 22, "fp1"), "host:22 fp1\n");
    }

    // ------------------------------------------------------------- parse_known_hosts_content

    #[test]
    fn parse_empty_content_returns_empty() {
        assert!(parse_known_hosts_content("").is_empty());
        assert!(parse_known_hosts_content("\n\n   \n").is_empty());
    }

    #[test]
    fn parse_skips_comments_and_blank_lines() {
        let text = "\n# a comment\n  \n# another\n";
        assert!(parse_known_hosts_content(text).is_empty());
    }

    #[test]
    fn parse_host_with_explicit_port() {
        let rows = parse_known_hosts_content("192.168.1.1:2222 SHA256:abc\n");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "192.168.1.1");
        assert_eq!(rows[0].1, 2222);
        assert_eq!(rows[0].2, "SHA256:abc");
    }

    #[test]
    fn parse_host_without_port_defaults_to_22() {
        let rows = parse_known_hosts_content("github.com SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s\n");
        assert_eq!(rows[0].0, "github.com");
        assert_eq!(rows[0].1, 22);
        assert_eq!(rows[0].2, "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s");
    }

    #[test]
    fn parse_host_with_colon_in_ipv6_addr_or_port_garbage_falls_back_to_default_port() {
        // If the token after the last ':' is not a valid u16, treat host:port as
        // just host with default port 22.
        let rows = parse_known_hosts_content("host:NOTAPORT fp_x\n");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "host:NOTAPORT");
        assert_eq!(rows[0].1, 22);
        assert_eq!(rows[0].2, "fp_x");
    }

    #[test]
    fn parse_line_missing_fingerprint_is_skipped() {
        // A single token with no space -> no split_once -> skipped.
        let rows = parse_known_hosts_content("only.host\n");
        assert!(rows.is_empty());
    }

    #[test]
    fn parse_multiple_mixed_lines_preserves_order() {
        let text = "\
# banner
serverA:22 SHA256:A
serverB:2022 SHA256:B
serverC  SHA256:C   # inline comment not in fp: fp is 'SHA256:C'
";
        let rows = parse_known_hosts_content(text);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].0, "serverA");
        assert_eq!(rows[0].1, 22);
        assert_eq!(rows[0].2, "SHA256:A");
        assert_eq!(rows[1].0, "serverB");
        assert_eq!(rows[1].1, 2022);
        assert_eq!(rows[1].2, "SHA256:B");
        assert_eq!(rows[2].0, "serverC");
        assert_eq!(rows[2].1, 22);
        // Because split_once splits on the FIRST space,
        // the fp part becomes "SHA256:C   # inline comment not in fp: fp is 'SHA256:C'"
        // This documents current behavior.
        assert!(rows[2].2.starts_with("SHA256:C"));
    }

    // ------------------------------------------------------------- host key changed parse / code

    #[test]
    fn host_key_changed_error_roundtrips_through_parser() {
        let err = host_key_changed_error(
            "127.0.0.1",
            2222,
            "SHA256:old",
            "SHA256:new",
        );
        let parsed = parse_host_key_changed(&err).expect("parse");
        assert_eq!(parsed.host, "127.0.0.1");
        assert_eq!(parsed.port, 2222);
        assert_eq!(parsed.expected, "SHA256:old");
        assert_eq!(parsed.got, "SHA256:new");
    }

    #[test]
    fn parse_legacy_host_key_changed_english() {
        let err = "Host key changed for 127.0.0.1:2222 (expected SHA256:aaa, got SHA256:bbb). Refusing to connect.";
        let parsed = parse_host_key_changed(err).expect("legacy");
        assert_eq!(parsed.host, "127.0.0.1");
        assert_eq!(parsed.port, 2222);
        assert_eq!(parsed.expected, "SHA256:aaa");
        assert_eq!(parsed.got, "SHA256:bbb");
    }

    #[test]
    fn parse_non_host_key_error_is_none() {
        assert!(parse_host_key_changed("Authentication failed").is_none());
    }
}
