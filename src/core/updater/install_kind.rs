//! 判断程序是怎么装的，决定能不能自动更新。
//!
//! 判断顺序：编译时渠道标记 → 包管理器路径 → 平台（Windows 安装版 / 便携版、macOS .app、Linux 压缩包）。

use std::path::{Path, PathBuf};

use super::manifest::platform_keys;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOs {
    Linux,
    Windows,
    Macos,
    Other,
}

impl TargetOs {
    pub fn current() -> Self {
        if cfg!(target_os = "linux") {
            TargetOs::Linux
        } else if cfg!(target_os = "windows") {
            TargetOs::Windows
        } else if cfg!(target_os = "macos") {
            TargetOs::Macos
        } else {
            TargetOs::Other
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallKind {
    /// 官方 tar.gz 解压出来的目录，或官网脚本装到 `~/.local/bin` 的 `mist`。
    LinuxPortable { dir: PathBuf },
    /// Windows 便携版 zip。
    WindowsPortable { dir: PathBuf },
    /// Windows Inno 安装版（同目录有 `unins000.exe`）。
    WindowsInstaller { dir: PathBuf },
    /// macOS `.app`（签名前只提醒）。
    MacApp { app: PathBuf, translocated: bool },
    /// macOS 裸二进制。
    MacBinary { dir: PathBuf },
    /// 由包管理器安装：交给包管理器升级。
    PackageManager { name: &'static str, upgrade_hint: &'static str },
    /// 从源码自行编译（没有官方渠道标记）。
    SourceBuild,
    /// 其它平台或无法识别。
    Unknown,
}

impl InstallKind {
    /// 清单中对应的平台条目。
    pub fn asset_key(&self) -> Option<&'static str> {
        match self {
            InstallKind::LinuxPortable { .. } => linux_asset_key(cfg!(target_env = "musl"), std::env::consts::ARCH),
            InstallKind::WindowsPortable { .. } => {
                (std::env::consts::ARCH == "x86_64").then_some(platform_keys::WINDOWS_X86_64_PORTABLE)
            }
            InstallKind::WindowsInstaller { .. } => {
                (std::env::consts::ARCH == "x86_64").then_some(platform_keys::WINDOWS_X86_64_SETUP)
            }
            InstallKind::MacApp { .. } | InstallKind::MacBinary { .. } => Some(platform_keys::MACOS_UNIVERSAL),
            _ => None,
        }
    }

    /// 直接替换文件的目录（安装版 / macOS / 包管理器返回 `None`）。
    pub fn replace_dir(&self) -> Option<&Path> {
        match self {
            InstallKind::LinuxPortable { dir } | InstallKind::WindowsPortable { dir } => Some(dir),
            _ => None,
        }
    }
}

/// Linux 压缩包对应的清单条目。
///
/// 静态（musl）构建只有命令行 `mist`，只从只含 CLI 的包更新，不会去拉桌面版的包；
/// glibc 构建（桌面版压缩包里的 `Mist` / `mist`）照旧用 `linux-x86_64`。
pub fn linux_asset_key(static_cli: bool, arch: &str) -> Option<&'static str> {
    match (static_cli, arch) {
        (true, "x86_64") => Some(platform_keys::LINUX_X86_64_CLI),
        (true, "aarch64") => Some(platform_keys::LINUX_AARCH64_CLI),
        (false, "x86_64") => Some(platform_keys::LINUX_X86_64),
        _ => None,
    }
}

/// 包管理器安装路径特征（目前官方没有任何包管理器渠道，这里为以后准备）。
/// `(特征, 是否只匹配路径开头, 名称, 升级命令)`
const PACKAGE_MANAGER_PATHS: &[(&str, bool, &str, &str)] = &[
    ("/nix/store/", true, "Nix", "nix profile upgrade"),
    ("/snap/", true, "Snap", "sudo snap refresh"),
    ("/opt/homebrew/", true, "Homebrew", "brew upgrade"),
    ("/usr/local/cellar/", true, "Homebrew", "brew upgrade"),
    ("/home/linuxbrew/", true, "Homebrew", "brew upgrade"),
    ("/usr/bin/", true, "the system package manager", "your package manager's upgrade command"),
    ("/usr/lib/", true, "the system package manager", "your package manager's upgrade command"),
    ("/usr/share/", true, "the system package manager", "your package manager's upgrade command"),
    ("/app/", true, "Flatpak", "flatpak update"),
    ("/scoop/apps/", false, "Scoop", "scoop update mistterm"),
    ("/winget/packages/", false, "WinGet", "winget upgrade MistTerm"),
    ("/chocolatey/", false, "Chocolatey", "choco upgrade mistterm"),
];

/// 纯函数版本，便于测试。`has_uninstaller`：exe 同目录是否有 `unins000.exe`。
pub fn detect(exe: &Path, os: TargetOs, official_build: bool, has_uninstaller: bool) -> InstallKind {
    let lower = exe.to_string_lossy().to_ascii_lowercase().replace('\\', "/");
    for (needle, prefix_only, name, hint) in PACKAGE_MANAGER_PATHS {
        let hit = if *prefix_only {
            // 这些系统路径只在 Linux / macOS 上有意义。
            os != TargetOs::Windows && lower.starts_with(needle)
        } else {
            lower.contains(needle)
        };
        if hit {
            return InstallKind::PackageManager {
                name,
                upgrade_hint: hint,
            };
        }
    }
    if !official_build {
        return InstallKind::SourceBuild;
    }
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    match os {
        TargetOs::Linux => InstallKind::LinuxPortable { dir },
        TargetOs::Windows => {
            if has_uninstaller {
                InstallKind::WindowsInstaller { dir }
            } else {
                InstallKind::WindowsPortable { dir }
            }
        }
        TargetOs::Macos => {
            if let Some(app) = enclosing_app_bundle(exe) {
                InstallKind::MacApp {
                    translocated: lower.contains("/apptranslocation/"),
                    app,
                }
            } else {
                InstallKind::MacBinary { dir }
            }
        }
        TargetOs::Other => InstallKind::Unknown,
    }
}

fn enclosing_app_bundle(exe: &Path) -> Option<PathBuf> {
    // .../Mist.app/Contents/MacOS/Mist
    let macos_dir = exe.parent()?;
    let contents = macos_dir.parent()?;
    let app = contents.parent()?;
    let is_app = macos_dir.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && app.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"));
    is_app.then(|| app.to_path_buf())
}

/// 检测当前进程的安装方式。
pub fn detect_current(exe: &Path) -> InstallKind {
    let has_uninstaller = exe
        .parent()
        .map(|d| d.join("unins000.exe").is_file())
        .unwrap_or(false);
    detect(exe, TargetOs::current(), super::is_official_build(), has_uninstaller)
}

/// 目录是否可写（试着建一个临时文件）。
pub fn dir_is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".mist-update-probe-{}", std::process::id()));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// 本机 glibc 版本（仅 Linux gnu）。
pub fn local_glibc_version() -> Option<(u32, u32)> {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        // SAFETY: gnu_get_libc_version 返回指向静态字符串的指针。
        let ptr = unsafe { libc::gnu_get_libc_version() };
        if ptr.is_null() {
            return None;
        }
        let s = unsafe { std::ffi::CStr::from_ptr(ptr) }.to_string_lossy();
        super::manifest::parse_glibc_version(&s)
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_asset_keys() {
        assert_eq!(linux_asset_key(false, "x86_64"), Some("linux-x86_64"));
        assert_eq!(linux_asset_key(false, "aarch64"), None);
        assert_eq!(linux_asset_key(true, "x86_64"), Some("linux-x86_64-cli"));
        assert_eq!(linux_asset_key(true, "aarch64"), Some("linux-aarch64-cli"));
        assert_eq!(linux_asset_key(true, "riscv64"), None);
    }

    #[test]
    fn source_build_is_notify_only() {
        let k = detect(Path::new("/home/u/MistTerm/target/release/Mist"), TargetOs::Linux, false, false);
        assert_eq!(k, InstallKind::SourceBuild);
    }

    #[test]
    fn linux_portable() {
        let k = detect(Path::new("/home/u/Mist-linux-x86_64/Mist"), TargetOs::Linux, true, false);
        assert_eq!(
            k,
            InstallKind::LinuxPortable {
                dir: PathBuf::from("/home/u/Mist-linux-x86_64")
            }
        );
        let k = detect(Path::new("/home/u/.local/bin/mist"), TargetOs::Linux, true, false);
        assert!(matches!(k, InstallKind::LinuxPortable { .. }));
    }

    #[test]
    fn package_managers() {
        for p in [
            "/usr/bin/mist",
            "/nix/store/abc-mistterm/bin/Mist",
            "/snap/mistterm/1/Mist",
            "/app/bin/Mist",
        ] {
            assert!(
                matches!(detect(Path::new(p), TargetOs::Linux, true, false), InstallKind::PackageManager { .. }),
                "{p}"
            );
        }
        assert!(matches!(
            detect(Path::new("/opt/homebrew/bin/mist"), TargetOs::Macos, true, false),
            InstallKind::PackageManager { name: "Homebrew", .. }
        ));
        assert!(matches!(
            detect(
                Path::new(r"C:\Users\u\scoop\apps\mistterm\current\Mist.exe"),
                TargetOs::Windows,
                true,
                false
            ),
            InstallKind::PackageManager { name: "Scoop", .. }
        ));
        // ~/app/ 不是 Flatpak
        assert!(matches!(
            detect(Path::new("/home/u/app/Mist"), TargetOs::Linux, true, false),
            InstallKind::LinuxPortable { .. }
        ));
    }

    #[test]
    fn windows_installer_vs_portable() {
        let exe = Path::new(r"C:\Users\u\AppData\Local\Programs\MistTerm\Mist.exe");
        assert!(matches!(detect(exe, TargetOs::Windows, true, true), InstallKind::WindowsInstaller { .. }));
        assert!(matches!(detect(exe, TargetOs::Windows, true, false), InstallKind::WindowsPortable { .. }));
    }

    #[test]
    fn macos_app_and_translocation() {
        let k = detect(Path::new("/Applications/Mist.app/Contents/MacOS/Mist"), TargetOs::Macos, true, false);
        assert_eq!(
            k,
            InstallKind::MacApp {
                app: PathBuf::from("/Applications/Mist.app"),
                translocated: false
            }
        );
        let k = detect(
            Path::new("/private/var/folders/x/AppTranslocation/ABC/d/Mist.app/Contents/MacOS/Mist"),
            TargetOs::Macos,
            true,
            false,
        );
        assert!(matches!(k, InstallKind::MacApp { translocated: true, .. }));
        let k = detect(Path::new("/Users/u/bin/mist"), TargetOs::Macos, true, false);
        assert!(matches!(k, InstallKind::MacBinary { .. }));
    }

    #[test]
    fn writable_probe() {
        let dir = tempfile::tempdir().unwrap();
        assert!(dir_is_writable(dir.path()));
        assert!(!dir_is_writable(&dir.path().join("missing")));
    }

    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    #[test]
    fn glibc_detected() {
        let (major, _) = local_glibc_version().unwrap();
        assert_eq!(major, 2);
    }
}
