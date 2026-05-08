//! 本机密钥派生（与会话密码、凭证库共用）
//!
//! 使用设备指纹 + 固定盐生成 AES-256 密钥，用于本地加密存储。

use aes_gcm::aead::Aead;
use aes_gcm::aead::KeyInit;
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use std::process::Command;

/// 构建设备指纹字符串（macOS 优先 IOPlatformUUID，其余环境退化）
pub fn build_device_fingerprint() -> String {
    let output = Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output();
    if let Ok(out) = output {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                if let Some(pos) = line.find("IOPlatformUUID") {
                    let tail = &line[pos..];
                    let parts: Vec<&str> = tail.split('"').collect();
                    if parts.len() >= 4 {
                        return parts[3].to_string();
                    }
                }
            }
        }
    }
    format!(
        "{}:{}:{}",
        std::env::consts::OS,
        std::env::var("USER").unwrap_or_default(),
        std::env::var("HOSTNAME").unwrap_or_default()
    )
}

/// 轻量 key 派生（依赖设备指纹 + 固定盐）
pub fn derive_key_from_fingerprint(fingerprint: &str) -> [u8; 32] {
    let mut key = [0u8; 32];
    let salt = b"MistTerm-Local-Device-Key-v1";
    let bytes = fingerprint.as_bytes();
    if bytes.is_empty() {
        return key;
    }
    for i in 0..32 {
        let a = bytes[i % bytes.len()];
        let b = salt[i % salt.len()];
        key[i] = a.wrapping_add(b).rotate_left((i % 8) as u32) ^ (i as u8);
    }
    key
}

/// 当前设备的本地加密密钥（与会话 `sessions.json` 一致）
pub fn device_key() -> [u8; 32] {
    derive_key_from_fingerprint(&build_device_fingerprint())
}

/// HKDF-SHA256：从设备根密钥派生凭证库专用数据密钥（与文件内随机盐绑定）
pub fn derive_credential_vault_data_key(
    device_root_key: &[u8; 32],
    salt: &[u8],
) -> Option<[u8; 32]> {
    if salt.is_empty() {
        return None;
    }
    let hk = Hkdf::<Sha256>::new(Some(salt), device_root_key.as_slice());
    let mut okm = [0u8; 32];
    hk.expand(b"MistTerm-CredentialVault-v2", &mut okm).ok()?;
    Some(okm)
}

/// AES-GCM 加密，返回 (base64 密文, base64 nonce)
pub fn encrypt_secret(key: &[u8; 32], plain: &str) -> Option<(String, String)> {
    if plain.is_empty() {
        return Some((String::new(), String::new()));
    }
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, plain.as_bytes()).ok()?;
    Some((B64.encode(ciphertext), B64.encode(nonce_bytes)))
}

/// 解密
pub fn decrypt_secret(key: &[u8; 32], encrypted_b64: &str, nonce_b64: &str) -> Option<String> {
    if encrypted_b64.is_empty() || nonce_b64.is_empty() {
        return Some(String::new());
    }
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    let ciphertext = B64.decode(encrypted_b64).ok()?;
    let nonce_raw = B64.decode(nonce_b64).ok()?;
    if nonce_raw.len() != 12 {
        return None;
    }
    let nonce = Nonce::from_slice(&nonce_raw);
    let plain = cipher.decrypt(nonce, ciphertext.as_ref()).ok()?;
    String::from_utf8(plain).ok()
}
