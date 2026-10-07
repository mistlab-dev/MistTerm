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
    if let Some(h) = test_home() {
        return h.join("update-state.json");
    }
    let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    p.push("mistterm");
    p.push("update-state.json");
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
