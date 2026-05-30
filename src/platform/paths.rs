//! 跨平台路径（配置、SSH、家目录）。

use std::path::PathBuf;

/// 用户主目录（`HOME` / `USERPROFILE`）。
pub fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Ok(p) = std::env::var("USERPROFILE") {
            return Some(PathBuf::from(p));
        }
    }
    std::env::var("HOME").ok().map(PathBuf::from).or_else(dirs::home_dir)
}

/// 默认 OpenSSH 配置文件路径。
pub fn default_ssh_config_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ssh")
        .join("config")
}
