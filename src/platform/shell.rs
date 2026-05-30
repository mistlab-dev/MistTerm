//! 调用系统默认程序打开路径（文件 / 目录）。

use std::path::Path;
use std::process::Command;

/// 在系统文件管理器中显示目录。
pub fn reveal_directory(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    open_with_system(path)
}

/// 用系统默认应用打开文件。
pub fn open_file(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("File not found: {}", path.display()));
    }
    open_with_system(path)
        .then_some(())
        .ok_or_else(|| format!("Could not open: {}", path.display()))
}

/// 用系统默认浏览器打开 URL。
pub fn open_url(url: &str) -> bool {
    let url = url.trim();
    if url.is_empty() {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        return Command::new("open").arg(url).spawn().is_ok();
    }
    #[cfg(target_os = "windows")]
    {
        return Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .is_ok();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return Command::new("xdg-open").arg(url).spawn().is_ok();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        let _ = url;
        false
    }
}

fn open_with_system(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        return Command::new("open").arg(path).spawn().is_ok();
    }
    #[cfg(target_os = "windows")]
    {
        if path.is_dir() {
            return Command::new("explorer").arg(path).spawn().is_ok();
        }
        return Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn()
            .is_ok();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return Command::new("xdg-open").arg(path).spawn().is_ok();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        let _ = path;
        false
    }
}
