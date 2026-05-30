//! 调用系统 `ssh-keygen` 生成 SSH 密钥对。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 生成 Ed25519 密钥对；`passphrase` 为空表示无密码。
pub fn generate_ed25519(
    private_key_path: &Path,
    comment: &str,
    passphrase: &str,
) -> Result<PathBuf, String> {
    if private_key_path.exists() {
        return Err(format!(
            "file already exists: {}",
            private_key_path.display()
        ));
    }
    if let Some(parent) = private_key_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let mut cmd = Command::new("ssh-keygen");
    cmd.arg("-t")
        .arg("ed25519")
        .arg("-f")
        .arg(private_key_path)
        .arg("-C")
        .arg(comment)
        .arg("-N")
        .arg(passphrase);
    let output = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "ssh-keygen not found; install OpenSSH client tools".to_string()
        } else {
            e.to_string()
        }
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(if stderr.trim().is_empty() {
            "ssh-keygen failed".into()
        } else {
            stderr.trim().to_string()
        });
    }
    let pub_path = PathBuf::from(format!("{}.pub", private_key_path.display()));
    if !pub_path.exists() {
        return Err("ssh-keygen finished but .pub file missing".into());
    }
    Ok(pub_path)
}
