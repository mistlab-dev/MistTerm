//! 使用 [`device_key`] 作为 AES-256-GCM 密钥，整文件加密 JSON 配置（跨平台统一）。

use std::fs;
use std::io;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::device_key;

pub const ENVELOPE_FORMAT: &str = "mistterm-aes-v1";

#[derive(Debug, Serialize, serde::Deserialize)]
pub struct ConfigEnvelope {
    pub format: String,
    pub nonce_b64: String,
    pub ciphertext_b64: String,
}

/// 将 JSON 序列化后整文件加密写入。
pub fn save_encrypted_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let plain = serde_json::to_string_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let key = device_key::device_key();
    let (ciphertext_b64, nonce_b64) = device_key::encrypt_secret(&key, &plain).ok_or_else(|| {
        io::Error::new(io::ErrorKind::Other, "配置加密失败")
    })?;
    let envelope = ConfigEnvelope {
        format: ENVELOPE_FORMAT.to_string(),
        nonce_b64,
        ciphertext_b64,
    };
    let out = serde_json::to_string_pretty(&envelope)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(path, out)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// 读取并解密；若仍为明文 JSON 则自动迁移为加密格式。
pub fn load_encrypted_json<T: DeserializeOwned + Default + Serialize>(path: &Path) -> T {
    if !path.exists() {
        return T::default();
    }
    let text = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return T::default(),
    };
    if let Ok(env) = serde_json::from_str::<ConfigEnvelope>(&text) {
        if env.format == ENVELOPE_FORMAT {
            let key = device_key::device_key();
            if let Some(plain) =
                device_key::decrypt_secret(&key, &env.ciphertext_b64, &env.nonce_b64)
            {
                if let Ok(v) = serde_json::from_str(&plain) {
                    return v;
                }
            }
            return T::default();
        }
    }
    match serde_json::from_str::<T>(&text) {
        Ok(v) => {
            if let Err(e) = save_encrypted_json(path, &v) {
                tracing::warn!("Failed to migrate config to encrypted format ({}): {}", path.display(), e);
            } else {
                tracing::info!("Migrated config to device_key encryption: {}", path.display());
            }
            v
        }
        Err(_) => T::default(),
    }
}
