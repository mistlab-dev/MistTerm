//! 桌面客户端自动更新（检查、提醒、Linux / Windows 一键更新、回滚）。
//!
//! 设计要点（详见 `docs/release/AUTO_UPDATE.md`）：
//! - 发布流水线生成很小的更新清单 `latest.json`，并用 minisign 签名为 `latest.json.minisig`。
//! - 客户端内置公钥：**先验签、再解析**；任何镜像都不需要被信任。
//! - 清单里写明每个平台文件的 SHA-256，下载后逐字节核对。
//! - 只接受更高的版本；本地记住见过的最高版本，防止被旧清单「冻结」。
//! - 只发一个不带任何账号或设备信息的 GET 请求；`MIST_DISABLE_UPDATE_CHECK=1` 可彻底关闭。
//! - 绝不自动重启；macOS 目前只提醒、引导手动更新。

pub mod apply;
pub mod check;
pub mod error;
pub mod fetch;
pub mod install_kind;
pub mod keys;
pub mod lock;
pub mod manifest;
pub mod paths;
pub mod state;
pub mod verify;

pub use check::{check_for_update, CheckOptions, CheckOutcome, InstallPlan, ManualReason, UpdateInfo};
pub use error::UpdateError;
pub use manifest::{Manifest, PlatformAsset};
pub use state::UpdateState;

use serde::{Deserialize, Serialize};

/// 清单与签名里使用的产品名。
pub const PRODUCT: &str = "mistterm";

/// 当前程序版本（与 `--version`、「关于」一致）。
pub const APP_VERSION: &str = crate::platform::APP_VERSION;

/// 官网下载页：无法自动安装时引导用户去这里。
pub const DOWNLOAD_PAGE_URL: &str = "https://mistlab.dev/download.html";

/// GitHub 发布页（备用）。
pub const RELEASES_URL: &str = "https://github.com/mistlab-dev/MistTerm/releases";

/// 稳定渠道清单地址，按顺序尝试：mistlab.dev 为主，GitHub 附件直链为备用（不走 GitHub API，不受限流影响）。
///
/// 两处内容和签名完全相同；客户端只信任签名，不信任地址本身。
pub const STABLE_MANIFEST_URLS: &[&str] = &[
    "https://mistlab.dev/downloads/mistterm/stable/latest.json",
    "https://github.com/mistlab-dev/MistTerm/releases/latest/download/latest.json",
];

/// 更新渠道。目前只开放 stable；清单格式和签名里的渠道字段为以后的 beta 预留。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Stable,
}

impl UpdateChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            UpdateChannel::Stable => "stable",
        }
    }

    pub fn manifest_urls(self) -> Vec<String> {
        #[cfg(feature = "update-test")]
        if let Some(urls) = test_hooks::manifest_urls_override() {
            return urls;
        }
        match self {
            UpdateChannel::Stable => STABLE_MANIFEST_URLS.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// 用户偏好（存进加密的 `settings.json`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSettings {
    /// 启动后和运行中每天自动检查一次（默认开）。
    #[serde(default = "default_true")]
    pub auto_check: bool,
    /// 发现新版后在后台先下载好（默认关；安装仍需用户点按钮）。
    #[serde(default)]
    pub auto_download: bool,
    #[serde(default)]
    pub channel: UpdateChannel,
}

fn default_true() -> bool {
    true
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            auto_check: true,
            auto_download: false,
            channel: UpdateChannel::Stable,
        }
    }
}

/// 构建时注入的分发渠道：官方 CI 发布构建为 `github-release`；从源码自行编译时为空。
pub fn dist_channel() -> &'static str {
    option_env!("MIST_DIST_CHANNEL").unwrap_or("")
}

/// 是否官方发布构建（只有官方构建才会自动替换程序；其它构建只提醒）。
pub fn is_official_build() -> bool {
    dist_channel() == "github-release" || cfg!(feature = "update-test")
}

/// 管理员 / 企业可用环境变量 `MIST_DISABLE_UPDATE_CHECK=1` 彻底关闭检查（含手动检查）。
pub fn disabled_by_env() -> bool {
    env_flag_set(std::env::var("MIST_DISABLE_UPDATE_CHECK").ok().as_deref())
}

pub(crate) fn env_flag_set(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

/// `MistTerm/1.2.0 (linux; x86_64)`：只含版本和平台，不含任何设备或账号信息。
pub fn user_agent() -> String {
    format!(
        "MistTerm/{APP_VERSION} ({}; {})",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// 当前版本（解析失败时退回 0.0.0，保证总能比较）。
pub fn current_version() -> semver::Version {
    semver::Version::parse(APP_VERSION).unwrap_or_else(|_| semver::Version::new(0, 0, 0))
}

/// 仅 `--features update-test` 构建可用的测试钩子（正式构建不包含）。
#[cfg(feature = "update-test")]
pub mod test_hooks {
    /// `MIST_UPDATE_MANIFEST_URL`：逗号分隔的清单地址列表，指向本地假发布服务器。
    pub fn manifest_urls_override() -> Option<Vec<String>> {
        let raw = std::env::var("MIST_UPDATE_MANIFEST_URL").ok()?;
        let urls: Vec<String> = raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        (!urls.is_empty()).then_some(urls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_flag_values() {
        assert!(env_flag_set(Some("1")));
        assert!(env_flag_set(Some(" TRUE ")));
        assert!(env_flag_set(Some("yes")));
        assert!(!env_flag_set(Some("0")));
        assert!(!env_flag_set(Some("")));
        assert!(!env_flag_set(None));
    }

    #[test]
    fn user_agent_has_no_identifiers() {
        let ua = user_agent();
        assert!(ua.starts_with(&format!("MistTerm/{APP_VERSION} (")));
        assert!(!ua.contains('@'));
    }

    #[test]
    fn settings_defaults_match_policy() {
        // 决策 6：默认自动检查开、自动下载关。
        let s = UpdateSettings::default();
        assert!(s.auto_check);
        assert!(!s.auto_download);
        let parsed: UpdateSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, s);
    }

    #[test]
    fn stable_manifest_urls_are_https_and_mistlab_first() {
        assert!(STABLE_MANIFEST_URLS[0].starts_with("https://mistlab.dev/"));
        assert!(STABLE_MANIFEST_URLS[1].starts_with("https://github.com/"));
        assert!(STABLE_MANIFEST_URLS.iter().all(|u| u.starts_with("https://")));
    }
}
