//! 内置的更新签名公钥。
//!
//! 公钥本身是公开信息，直接放在仓库里（`resources/update/*.pub`），方便审计。
//! - `minisign-primary.pub`：日常发布用的钥匙。
//! - `minisign-backup.pub`：离线保存的备用钥匙。日常私钥泄露时，用它签发一个换钥版本，老客户端仍能验证。
//!
//! 在 Tian 于自己电脑上生成正式密钥之前，这两个文件是**占位符**：
//! - 普通构建：占位符被忽略，客户端没有可信公钥，检查更新会明确提示「无法验证更新」，不会信任任何清单。
//! - 官方发布构建（`MIST_DIST_CHANNEL=github-release`）：`build.rs` 发现占位符会**直接让构建失败**，
//!   保证不会发出一个更新功能失效的正式版本。
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

    #[test]
    fn real_key_file_is_read() {
        let f = "untrusted comment: minisign public key 318907ECCDF4E9F0\nRWTw6fTN7AeJMV2AcMgIoyLDXxzD430boD4J1d6V//064JvJxYyYyKBn\n";
        assert_eq!(
            pubkey_line(f),
            Some("RWTw6fTN7AeJMV2AcMgIoyLDXxzD430boD4J1d6V//064JvJxYyYyKBn")
        );
    }
}
