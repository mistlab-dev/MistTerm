//! 更新用到的本地目录。
//!
//! - 状态文件：`<配置目录>/mistterm/update-state.json`（明文，内容不敏感）
//! - 下载缓存：`<缓存目录>/mistterm/updates/`（Linux `~/.cache`，Windows `%LOCALAPPDATA%`）
//!
//! 测试构建可用 `MIST_UPDATE_HOME` 把两者都指到临时目录。

use std::path::PathBuf;

fn test_home() -> Option<PathBuf> {
    #[cfg(feature = "update-test")]
    if let Some(h) = std::env::var_os("MIST_UPDATE_HOME") {
        if !h.is_empty() {
            return Some(PathBuf::from(h));
        }
    }
    None
}

pub fn state_file() -> PathBuf {
    state_file_for(super::UpdateChannel::Stable)
}

/// #435：按渠道隔离状态文件——稳定渠道沿用旧路径（老用户兼容），预览渠道独立文件，
/// 两渠道的最高已见版本互不影响。
pub fn state_file_for(channel: super::UpdateChannel) -> PathBuf {
    let name = match channel {
        super::UpdateChannel::Stable => "update-state.json",
        super::UpdateChannel::Preview => "update-state-preview.json",
    };
    if let Some(h) = test_home() {
        return h.join(name);
    }
    let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("mistterm");
    p.push(name);
    p
}

pub fn cache_dir() -> PathBuf {
    if let Some(h) = test_home() {
        return h.join("updates");
    }
    let mut p = dirs::cache_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(std::env::temp_dir);
    p.push("mistterm");
    p.push("updates");
    p
}

/// 程序启动时的可执行文件路径（解析符号链接后）。
///
/// 要在**替换文件之前**取一次并保存：Linux 上替换后 `/proc/self/exe` 指向的已是旧文件。
pub fn current_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(std::fs::canonicalize(&exe).unwrap_or(exe))
}

#[cfg(test)]
mod tests {
    use super::*;

    // #435 点查：两渠道状态文件必须真正隔离（不同文件名），不能以稳定路径落盘预览状态。
    #[test]
    fn channel_state_files_are_isolated() {
        let stable = state_file_for(super::super::UpdateChannel::Stable);
        let preview = state_file_for(super::super::UpdateChannel::Preview);
        assert_ne!(stable, preview, "stable/preview state files must differ");
        assert!(stable.file_name().and_then(|s| s.to_str()) == Some("update-state.json"));
        assert!(preview.file_name().and_then(|s| s.to_str()) == Some("update-state-preview.json"));
        assert_eq!(state_file(), stable, "state_file() must stay the stable path");
    }
}
