//! 检查更新：按顺序请求清单 → 验签 → 防降级 / 防冻结 → 决定能否自动安装。

use std::path::PathBuf;

use super::error::UpdateError;
use super::fetch::Fetcher;
use super::install_kind::{self, InstallKind};
use super::manifest::{Manifest, PlatformAsset, KIND_INNO_SETUP, KIND_TAR_GZ, KIND_ZIP, MAX_MANIFEST_BYTES};
use super::state::UpdateState;
use super::verify::{self, TrustedKeys};
use super::{UpdateChannel, DOWNLOAD_PAGE_URL};

#[derive(Debug, Clone)]
pub struct CheckOptions {
    pub channel: UpdateChannel,
    pub manifest_urls: Vec<String>,
    pub keys: TrustedKeys,
    pub current: semver::Version,
    /// 允许「更新」到更低的版本（仅 CLI 隐藏参数，用于手动回退）。
    pub allow_downgrade: bool,
    pub allow_loopback_http: bool,
}

impl CheckOptions {
    /// 本构建的默认配置：内置公钥、渠道地址、当前版本。
    pub fn for_current_build(channel: UpdateChannel) -> Self {
        Self {
            channel,
            manifest_urls: channel.manifest_urls(),
            keys: TrustedKeys::embedded(),
            current: super::current_version(),
            allow_downgrade: false,
            allow_loopback_http: super::fetch::allow_loopback_http(),
        }
    }
}

/// 为什么只能提醒、不能自动安装。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualReason {
    /// 从源码自行编译。
    SourceBuild,
    /// 包管理器安装。
    PackageManager { name: &'static str, upgrade_hint: &'static str },
    /// macOS：目前只提醒，引导手动替换 Mist.app。
    MacOs { translocated: bool },
    /// 安装目录没有写权限。
    NotWritable(PathBuf),
    /// 本机 glibc 太旧，新版本跑不起来。
    GlibcTooOld { need: (u32, u32), have: (u32, u32) },
    /// 清单里没有本平台的文件。
    NoBuildForPlatform,
    /// 发布方在清单里关掉了这个平台的自动更新。
    DisabledByPublisher,
    /// 无法识别的安装方式。
    Unrecognized,
}

impl ManualReason {
    /// 一句话说明为什么只能手动更新，以及该怎么做。
    pub fn user_message(&self, zh: bool) -> String {
        let t = |en: &str, cn: &str| if zh { cn.to_string() } else { en.to_string() };
        match self {
            ManualReason::SourceBuild => t(
                "This copy was built from source, so it won't replace itself. Download the new version from the website, or rebuild it from source.",
                "这份程序是从源码自己编译的，不会自动替换。请到官网下载新版本，或重新从源码编译。",
            ),
            ManualReason::PackageManager { name, upgrade_hint } => {
                if zh {
                    format!("这份程序是通过 {name} 安装的，请用它来更新：{upgrade_hint}")
                } else {
                    format!("Installed with {name}. Update it there: {upgrade_hint}")
                }
            }
            ManualReason::MacOs { translocated: false } => t(
                "Mist can't update itself on macOS yet. Download the new version and drag Mist.app into Applications to replace the old one.",
                "macOS 上暂时不能自动更新。请下载新版本，把 Mist.app 拖进「应用程序」文件夹替换旧版。",
            ),
            ManualReason::MacOs { translocated: true } => t(
                "Mist is running from a temporary location chosen by macOS. Move Mist.app into Applications first, then download the new version and replace it.",
                "Mist 现在是在 macOS 临时指定的位置运行的。请先把 Mist.app 移到「应用程序」文件夹，再下载新版本替换。",
            ),
            ManualReason::NotWritable(dir) => UpdateError::NotWritable(dir.clone()).user_message(zh),
            ManualReason::GlibcTooOld { need, have } => {
                if zh {
                    format!(
                        "新版本需要 glibc {}.{} 或更新，这台机器是 {}.{}。可以继续用当前版本，或先升级系统。",
                        need.0, need.1, have.0, have.1
                    )
                } else {
                    format!(
                        "The new version needs glibc {}.{} or newer; this system has {}.{}. Keep using the current version, or upgrade the system first.",
                        need.0, need.1, have.0, have.1
                    )
                }
            }
            ManualReason::NoBuildForPlatform => t(
                "There's no automatic update for this platform. Download the new version from the website.",
                "这个平台没有自动更新包，请到官网下载新版本。",
            ),
            ManualReason::DisabledByPublisher => t(
                "Automatic install isn't offered for this platform right now. Download the new version from the website.",
                "这个平台暂时不提供自动安装，请到官网下载新版本。",
            ),
            ManualReason::Unrecognized => t(
                "Mist couldn't tell how it was installed, so it won't replace itself. Download the new version from the website.",
                "无法判断 Mist 的安装方式，不会自动替换。请到官网下载新版本。",
            ),
        }
    }

    /// 机器可读的短名（`mist update --check --json`）。
    pub fn code(&self) -> &'static str {
        match self {
            ManualReason::SourceBuild => "source_build",
            ManualReason::PackageManager { .. } => "package_manager",
            ManualReason::MacOs { .. } => "macos_manual",
            ManualReason::NotWritable(_) => "not_writable",
            ManualReason::GlibcTooOld { .. } => "glibc_too_old",
            ManualReason::NoBuildForPlatform => "no_build_for_platform",
            ManualReason::DisabledByPublisher => "disabled_by_publisher",
            ManualReason::Unrecognized => "unrecognized_install",
        }
    }
}

#[derive(Debug, Clone)]
pub enum InstallPlan {
    /// 可以一键更新。
    Auto {
        kind: InstallKind,
        asset_key: &'static str,
        asset: PlatformAsset,
    },
    /// 只提醒：打开下载页 / 提示命令。
    Manual {
        reason: ManualReason,
        asset: Option<PlatformAsset>,
        download_url: String,
    },
}

impl InstallPlan {
    pub fn is_auto(&self) -> bool {
        matches!(self, InstallPlan::Auto { .. })
    }

    /// Windows 安装版：安装时会关闭并重新打开 Mist。
    pub fn uses_installer(&self) -> bool {
        matches!(
            self,
            InstallPlan::Auto {
                kind: InstallKind::WindowsInstaller { .. },
                ..
            }
        )
    }

    pub fn download_size(&self) -> Option<u64> {
        match self {
            InstallPlan::Auto { asset, .. } => Some(asset.size),
            InstallPlan::Manual { asset, .. } => asset.as_ref().map(|a| a.size),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub current: semver::Version,
    pub manifest: Manifest,
    /// 实际取到清单的地址（日志用）。
    pub source_url: String,
    pub plan: InstallPlan,
}

impl UpdateInfo {
    pub fn latest(&self) -> semver::Version {
        self.manifest.semver()
    }
}

#[derive(Debug, Clone)]
pub enum CheckOutcome {
    UpToDate {
        current: semver::Version,
        latest: semver::Version,
    },
    Available(Box<UpdateInfo>),
}

/// 取回并验证一份清单（多个地址按顺序尝试）。
pub fn fetch_verified_manifest(
    opts: &CheckOptions,
    state: &UpdateState,
) -> Result<(Manifest, String), UpdateError> {
    if super::disabled_by_env() {
        return Err(UpdateError::DisabledByEnv);
    }
    if opts.keys.is_empty() {
        return Err(UpdateError::NoTrustedKeys);
    }
    let fetcher = Fetcher::for_manifest()?;
    let mut last_err: Option<UpdateError> = None;
    let mut saw_stale = false;
    for url in &opts.manifest_urls {
        let sig_url = format!("{url}.minisig");
        let attempt = (|| {
            let body = fetcher.get_small(url, MAX_MANIFEST_BYTES)?;
            let sig = fetcher.get_small(&sig_url, 4096)?;
            verify::verify_manifest(&body, &sig, &opts.keys, opts.channel.as_str(), opts.allow_loopback_http)
        })();
        match attempt {
            Ok(m) => {
                if state.is_stale(&m.semver(), m.pub_date_utc()) {
                    log::warn!("updater: stale manifest {} from {url}, trying next source", m.version);
                    saw_stale = true;
                    continue;
                }
                log::info!("updater: manifest {} verified from {url}", m.version);
                return Ok((m, url.clone()));
            }
            Err(e) => {
                log::info!("updater: manifest source {url} failed: {e}");
                // 优先保留更有信息量的错误（例如签名错误比网络错误更值得告诉用户）。
                let keep_previous = matches!(
                    (&last_err, &e),
                    (Some(UpdateError::BadSignature | UpdateError::UnsupportedSchema(_)), UpdateError::Network(_))
                );
                if !keep_previous {
                    last_err = Some(e);
                }
            }
        }
    }
    if saw_stale {
        return Err(UpdateError::StaleManifest);
    }
    Err(last_err.unwrap_or_else(|| UpdateError::Network("no update source configured".into())))
}

/// 完整的一次检查。成功时更新 `state` 里的检查时间和见过的最高版本（调用方负责保存）。
pub fn check_for_update(opts: &CheckOptions, state: &mut UpdateState) -> Result<CheckOutcome, UpdateError> {
    let (manifest, source_url) = fetch_verified_manifest(opts, state)?;
    state.last_check_unix = Some(chrono::Utc::now().timestamp());
    state.record_seen(&manifest.semver(), &manifest.pub_date);
    let latest = manifest.semver();
    let newer = latest > opts.current;
    let downgrade_ok = opts.allow_downgrade && latest != opts.current;
    if !newer && !downgrade_ok {
        return Ok(CheckOutcome::UpToDate {
            current: opts.current.clone(),
            latest,
        });
    }
    let kind = super::paths::current_exe()
        .map(|exe| install_kind::detect_current(&exe))
        .unwrap_or(InstallKind::Unknown);
    let plan = plan_install(&manifest, &kind, install_kind::local_glibc_version(), install_kind::dir_is_writable);
    Ok(CheckOutcome::Available(Box::new(UpdateInfo {
        current: opts.current.clone(),
        manifest,
        source_url,
        plan,
    })))
}

/// 根据安装方式和清单决定能否自动安装（纯逻辑，便于测试）。
pub fn plan_install(
    manifest: &Manifest,
    kind: &InstallKind,
    local_glibc: Option<(u32, u32)>,
    writable: impl Fn(&std::path::Path) -> bool,
) -> InstallPlan {
    let asset_key = kind.asset_key();
    let asset = asset_key.and_then(|k| manifest.asset(k)).cloned();
    let download_url = asset
        .as_ref()
        .and_then(|a| a.manual_url.clone())
        .unwrap_or_else(|| DOWNLOAD_PAGE_URL.to_string());
    let manual = |reason: ManualReason| InstallPlan::Manual {
        reason,
        asset: asset.clone(),
        download_url: download_url.clone(),
    };

    match kind {
        InstallKind::SourceBuild => return manual(ManualReason::SourceBuild),
        InstallKind::PackageManager { name, upgrade_hint } => {
            return manual(ManualReason::PackageManager {
                name,
                upgrade_hint,
            })
        }
        InstallKind::MacApp { translocated, .. } => {
            return manual(ManualReason::MacOs {
                translocated: *translocated,
            })
        }
        InstallKind::MacBinary { .. } => return manual(ManualReason::MacOs { translocated: false }),
        InstallKind::Unknown => return manual(ManualReason::Unrecognized),
        InstallKind::LinuxPortable { .. }
        | InstallKind::WindowsPortable { .. }
        | InstallKind::WindowsInstaller { .. } => {}
    }

    let (Some(asset_key), Some(asset)) = (asset_key, asset.clone()) else {
        return manual(ManualReason::NoBuildForPlatform);
    };
    if !asset.auto_update {
        return manual(ManualReason::DisabledByPublisher);
    }
    let kind_ok = match kind {
        InstallKind::LinuxPortable { .. } => asset.kind == KIND_TAR_GZ,
        InstallKind::WindowsPortable { .. } => asset.kind == KIND_ZIP,
        InstallKind::WindowsInstaller { .. } => asset.kind == KIND_INNO_SETUP,
        _ => false,
    };
    if !kind_ok {
        return manual(ManualReason::NoBuildForPlatform);
    }
    if let (Some(need), Some(have)) = (
        asset.min_glibc.as_deref().and_then(super::manifest::parse_glibc_version),
        local_glibc,
    ) {
        if have < need {
            return manual(ManualReason::GlibcTooOld { need, have });
        }
    }
    if let Some(dir) = kind.replace_dir() {
        if !writable(dir) {
            return manual(ManualReason::NotWritable(dir.to_path_buf()));
        }
    }
    InstallPlan::Auto {
        kind: kind.clone(),
        asset_key,
        asset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::updater::manifest::tests::sample_json;
    use std::path::Path;

    fn manifest() -> Manifest {
        Manifest::parse_and_validate(sample_json("1.2.0", "2026-10-05T12:00:00Z").as_bytes(), "stable", false)
            .unwrap()
    }

    fn linux() -> InstallKind {
        InstallKind::LinuxPortable {
            dir: "/home/u/Mist".into(),
        }
    }

    #[test]
    fn linux_auto_when_everything_ok() {
        let plan = plan_install(&manifest(), &linux(), Some((2, 39)), |_| true);
        if cfg!(target_arch = "x86_64") {
            assert!(plan.is_auto(), "{plan:?}");
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn glibc_too_old_is_manual() {
        let plan = plan_install(&manifest(), &linux(), Some((2, 35)), |_| true);
        assert!(matches!(
            plan,
            InstallPlan::Manual {
                reason: ManualReason::GlibcTooOld {
                    need: (2, 39),
                    have: (2, 35)
                },
                ..
            }
        ));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn read_only_dir_is_manual() {
        let plan = plan_install(&manifest(), &linux(), Some((2, 39)), |_| false);
        assert!(matches!(
            plan,
            InstallPlan::Manual {
                reason: ManualReason::NotWritable(_),
                ..
            }
        ));
    }

    #[test]
    fn macos_is_notify_only() {
        let kind = InstallKind::MacApp {
            app: "/Applications/Mist.app".into(),
            translocated: false,
        };
        let plan = plan_install(&manifest(), &kind, None, |_| true);
        match plan {
            InstallPlan::Manual {
                reason, download_url, ..
            } => {
                assert_eq!(reason, ManualReason::MacOs { translocated: false });
                assert_eq!(download_url, "https://mistlab.dev/download.html");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn source_build_and_package_manager_are_manual() {
        assert!(matches!(
            plan_install(&manifest(), &InstallKind::SourceBuild, None, |_| true),
            InstallPlan::Manual {
                reason: ManualReason::SourceBuild,
                ..
            }
        ));
        let kind = InstallKind::PackageManager {
            name: "Homebrew",
            upgrade_hint: "brew upgrade",
        };
        assert!(matches!(
            plan_install(&manifest(), &kind, None, |_| true),
            InstallPlan::Manual {
                reason: ManualReason::PackageManager { .. },
                ..
            }
        ));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn missing_platform_entry_is_manual() {
        let kind = InstallKind::WindowsPortable {
            dir: Path::new("C:/x").to_path_buf(),
        };
        assert!(matches!(
            plan_install(&manifest(), &kind, None, |_| true),
            InstallPlan::Manual {
                reason: ManualReason::NoBuildForPlatform,
                ..
            }
        ));
    }

    #[test]
    fn no_keys_is_reported_before_network() {
        let opts = CheckOptions {
            channel: UpdateChannel::Stable,
            manifest_urls: vec!["https://127.0.0.1:9/never".into()],
            keys: TrustedKeys::from_base64(Vec::<&str>::new()),
            current: semver::Version::new(1, 0, 0),
            allow_downgrade: false,
            allow_loopback_http: false,
        };
        let mut st = UpdateState::default();
        assert!(matches!(check_for_update(&opts, &mut st), Err(UpdateError::NoTrustedKeys)));
    }
}
