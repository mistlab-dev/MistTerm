//! 产品文档目录（编译时相对仓库根路径）。

use std::path::PathBuf;

/// `docs/` 目录（开发构建时有效；发布包若未携带则为空目录）。
pub fn docs_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs")
}

/// 在系统文件管理器中打开文档目录。
pub fn reveal_docs_directory() -> bool {
    let path = docs_directory();
    if !path.is_dir() {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        return std::process::Command::new("open").arg(&path).spawn().is_ok();
    }
    #[cfg(target_os = "windows")]
    {
        return std::process::Command::new("explorer").arg(&path).spawn().is_ok();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return std::process::Command::new("xdg-open").arg(&path).spawn().is_ok();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        let _ = path;
        false
    }
}
