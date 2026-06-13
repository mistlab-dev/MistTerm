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

/// SFTP / 表单路径占位示例（随平台与真实家目录变化）。
pub fn home_dir_display_hint() -> String {
    if let Some(h) = home_dir() {
        return h.to_string_lossy().into_owned();
    }
    #[cfg(windows)]
    {
        return r"C:\Users\me".to_string();
    }
    #[cfg(target_os = "macos")]
    {
        return "/Users/me".to_string();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return "/home/me".to_string();
    }
    #[cfg(not(any(windows, unix)))]
    {
        "~/".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::home_dir_display_hint;

    #[test]
    fn home_dir_display_hint_non_empty() {
        let hint = home_dir_display_hint();
        assert!(!hint.is_empty());
    }
}
