//! 本机默认目录：SFTP 本机侧初始目录与 ZMODEM `sz` 接收目录共用同一个设置。
//!
//! 设置为空时用系统「下载」目录（取不到则用用户主目录）。用户配置的目录不存在时不自动创建，
//! 回落到系统默认并告知调用方（由 UI 弹 Toast）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

static CONFIGURED: RwLock<Option<PathBuf>> = RwLock::new(None);
/// 每次偏好修改递增；SFTP 面板据此丢弃「本次运行记住的本机目录」，让新设置立即生效。
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// 解析结果：实际使用的目录；若用户配置的目录不可用，`missing` 带上原配置值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLocalDir {
    pub dir: PathBuf,
    pub missing: Option<PathBuf>,
}

/// 展开 `~`、去空白；空串返回 `None`（表示使用系统默认）。
pub fn parse_configured(raw: &str) -> Option<PathBuf> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let home = dirs::home_dir();
    if s == "~" {
        if let Some(h) = home {
            return Some(h);
        }
    }
    if let Some(rest) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        if let Some(h) = home {
            return Some(h.join(rest));
        }
    }
    Some(PathBuf::from(s))
}

/// 系统默认：下载目录 → 主目录 → 临时目录。
pub fn system_default() -> PathBuf {
    system_default_from(dirs::download_dir(), dirs::home_dir())
}

fn system_default_from(download: Option<PathBuf>, home: Option<PathBuf>) -> PathBuf {
    download
        .or_else(|| {
            home.clone()
                .map(|h| h.join("Downloads"))
                .filter(|p| p.is_dir())
        })
        .or(home)
        .unwrap_or_else(std::env::temp_dir)
}

/// 纯函数版本，便于单测：`is_dir` 判定目录可用性。
pub fn resolve_with(
    configured: Option<&Path>,
    default: PathBuf,
    is_dir: impl Fn(&Path) -> bool,
) -> ResolvedLocalDir {
    match configured {
        Some(p) if is_dir(p) => ResolvedLocalDir {
            dir: p.to_path_buf(),
            missing: None,
        },
        Some(p) => ResolvedLocalDir {
            dir: default,
            missing: Some(p.to_path_buf()),
        },
        None => ResolvedLocalDir {
            dir: default,
            missing: None,
        },
    }
}

/// 应用启动与偏好修改时调用。
pub fn set_configured(raw: &str) {
    let parsed = parse_configured(raw);
    if let Ok(mut g) = CONFIGURED.write() {
        *g = parsed;
    }
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// 当前生效的本机目录（SFTP 本机初始目录 / sz 保存目录）。
pub fn current() -> ResolvedLocalDir {
    let configured = CONFIGURED.read().ok().and_then(|g| g.clone());
    let default = system_default();
    let r = resolve_with(configured.as_deref(), default, |p| p.is_dir());
    // 实际使用的是系统默认目录（如 ~/Downloads）时缺了就建；用户配置目录不会走到这里创建。
    let _ = std::fs::create_dir_all(&r.dir);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_means_system_default() {
        assert_eq!(parse_configured(""), None);
        assert_eq!(parse_configured("   "), None);
    }

    #[test]
    fn tilde_expands_to_home() {
        let home = dirs::home_dir().expect("home");
        assert_eq!(parse_configured("~"), Some(home.clone()));
        assert_eq!(parse_configured(" ~/dl "), Some(home.join("dl")));
        assert_eq!(parse_configured("/srv/x"), Some(PathBuf::from("/srv/x")));
    }

    #[test]
    fn default_prefers_downloads_then_home() {
        assert_eq!(
            system_default_from(Some("/d".into()), Some("/h-nonexistent".into())),
            PathBuf::from("/d")
        );
        assert_eq!(
            system_default_from(None, Some("/h-nonexistent".into())),
            PathBuf::from("/h-nonexistent")
        );
        assert_eq!(system_default_from(None, None), std::env::temp_dir());
    }

    #[test]
    fn configured_existing_dir_wins() {
        let r = resolve_with(Some(Path::new("/cfg")), "/def".into(), |_| true);
        assert_eq!(r.dir, PathBuf::from("/cfg"));
        assert_eq!(r.missing, None);
    }

    #[test]
    fn missing_configured_falls_back_and_reports() {
        let r = resolve_with(Some(Path::new("/gone")), "/def".into(), |_| false);
        assert_eq!(r.dir, PathBuf::from("/def"));
        assert_eq!(r.missing, Some(PathBuf::from("/gone")));
    }

    #[test]
    fn unconfigured_uses_default_silently() {
        let r = resolve_with(None, "/def".into(), |_| false);
        assert_eq!(r.dir, PathBuf::from("/def"));
        assert_eq!(r.missing, None);
    }
}
