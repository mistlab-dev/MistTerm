//! 内置的更新签名公钥。
//!
//! 公钥本身是公开信息，直接放在仓库里（`resources/update/*.pub`），方便审计。
//! - `minisign-primary.pub`：日常发布用的钥匙。
//! - `minisign-backup.pub`：离线保存的备用钥匙。日常私钥泄露时，用它签发一个换钥版本，老客户端仍能验证。
//!
//! 两把都是 Tian 在自己电脑上生成的正式公钥（私钥加密保存在他那里，主钥私钥另存为 CI `release` 环境 secret）：
//! - 主钥 key ID `2F850D3521ADC099`
//! - 备用钥 key ID `E3C0CEA51587625C`
//!
//! 防呆仍保留：如果文件被换回占位符（含 `PLACEHOLDER`），普通构建会忽略它（没有可信公钥，检查更新会提示
//! 「无法验证更新」）；官方发布构建（`MIST_DIST_CHANNEL=github-release`）则由 `build.rs` **直接让构建失败**。
//!
//! 测试构建（`--features update-test`）额外信任编译时传入的临时测试公钥 `MIST_UPDATE_TEST_PUBKEY`，
//! 正式构建不包含这个 feature（`build.rs` 也禁止二者同时出现）。

/// 占位符标记；`build.rs` 用同一字符串识别。
pub const PLACEHOLDER_MARKER: &str = "PLACEHOLDER";

pub const PRIMARY_PUBKEY_FILE: &str = include_str!("../../../resources/update/minisign-primary.pub");
pub const BACKUP_PUBKEY_FILE: &str = include_str!("../../../resources/update/minisign-backup.pub");

/// 从 minisign 公钥文件内容中取出 base64 公钥行；占位符或格式不对时返回 `None`。
pub fn pubkey_line(file_content: &str) -> Option<&str> {
    if file_content.contains(PLACEHOLDER_MARKER) {
        return None;
    }
    file_content
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("untrusted comment:"))
}

/// 本构建信任的全部公钥（base64）。
pub fn embedded_pubkeys() -> Vec<String> {
    #[allow(unused_mut)]
    let mut keys: Vec<String> = [PRIMARY_PUBKEY_FILE, BACKUP_PUBKEY_FILE]
        .iter()
        .filter_map(|f| pubkey_line(f))
        .map(str::to_string)
        .collect();
    #[cfg(feature = "update-test")]
    if let Some(k) = option_env!("MIST_UPDATE_TEST_PUBKEY") {
        let k = k.trim();
        if !k.is_empty() {
            keys.push(k.to_string());
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_is_ignored() {
        assert_eq!(
            pubkey_line("untrusted comment: PLACEHOLDER\nPLACEHOLDER_KEY\n"),
            None
        );
    }

    /// 仓库里提交的正式公钥：格式正确、key ID 与注释一致、两把不同。换钥时同步更新这里的期望值。
    #[test]
    fn embedded_production_keys_are_valid() {
        let expected = [
            (PRIMARY_PUBKEY_FILE, "2F850D3521ADC099"),
            (BACKUP_PUBKEY_FILE, "E3C0CEA51587625C"),
        ];
        let mut lines = Vec::new();
        for (file, key_id) in expected {
            let line = pubkey_line(file).expect("embedded key must not be a placeholder");
            minisign_verify::PublicKey::from_base64(line).expect("embedded key must parse");
            assert!(
                file.lines().next().unwrap_or("").ends_with(key_id),
                "comment must name key ID {key_id}"
            );
            use base64::Engine;
            let raw = base64::engine::general_purpose::STANDARD.decode(line).unwrap();
            assert_eq!(&raw[..2], b"Ed");
            let id: String = raw[2..10].iter().rev().map(|b| format!("{b:02X}")).collect();
            assert_eq!(id, key_id);
            lines.push(line);
        }
        assert_ne!(lines[0], lines[1], "primary and backup keys must differ");
        #[cfg(not(feature = "update-test"))]
        assert_eq!(embedded_pubkeys(), lines);
    }

    #[test]
    fn real_key_file_is_read() {
        let f = "untrusted comment: minisign public key 318907ECCDF4E9F0\nRWTw6fTN7AeJMV2AcMgIoyLDXxzD430boD4J1d6V//064JvJxYyYyKBn\n";
        assert_eq!(
            pubkey_line(f),
            Some("RWTw6fTN7AeJMV2AcMgIoyLDXxzD430boD4J1d6V//064JvJxYyYyKBn")
        );
    }
}
