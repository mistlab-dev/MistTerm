//! 更新相关错误，以及给用户看的中英文说明。

use std::path::PathBuf;

#[derive(Debug, Clone, thiserror::Error)]
pub enum UpdateError {
    #[error("update check disabled by MIST_DISABLE_UPDATE_CHECK")]
    DisabledByEnv,
    #[error("this build has no update signing key")]
    NoTrustedKeys,
    #[error("network error: {0}")]
    Network(String),
    #[error("update manifest signature is invalid")]
    BadSignature,
    #[error("update manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error("update manifest uses a newer format (schema {0})")]
    UnsupportedSchema(u32),
    #[error("update server returned older information than seen before")]
    StaleManifest,
    #[error("downloaded file checksum mismatch")]
    Checksum,
    #[error("downloaded file has unexpected size")]
    SizeMismatch,
    #[error("another update is already running")]
    Locked,
    #[error("no write permission: {0}")]
    NotWritable(PathBuf),
    #[error("cancelled")]
    Cancelled,
    #[error("new version failed its self-check: {0}")]
    SmokeTest(String),
    #[error("cannot install automatically: {0}")]
    NotAutoInstallable(String),
    #[error("no backup to roll back to")]
    NoBackup,
    #[error("install failed: {0}")]
    Install(String),
}

impl UpdateError {
    /// 面向用户的一句话说明（`zh = true` 为简体中文）。
    pub fn user_message(&self, zh: bool) -> String {
        let t = |en: &str, cn: &str| if zh { cn.to_string() } else { en.to_string() };
        match self {
            UpdateError::DisabledByEnv => t(
                "Update checks are turned off on this computer (MIST_DISABLE_UPDATE_CHECK).",
                "这台电脑关闭了检查更新（MIST_DISABLE_UPDATE_CHECK）。",
            ),
            UpdateError::NoTrustedKeys => t(
                "This build can't verify updates, so it won't check for them. Download new versions from the website.",
                "这个版本没有内置验证更新用的公钥，无法检查更新。请到官网下载新版本。",
            ),
            UpdateError::Network(detail) => {
                if zh {
                    format!("连不上更新服务器，请检查网络后重试。（{detail}）")
                } else {
                    format!("Couldn't reach the update server. Check your connection and try again. ({detail})")
                }
            }
            UpdateError::BadSignature | UpdateError::InvalidManifest(_) => t(
                "The update information didn't pass verification, so it was ignored. Try again later.",
                "收到的更新信息没有通过验证，已忽略。请稍后再试。",
            ),
            UpdateError::UnsupportedSchema(_) => t(
                "A new version needs a manual download. Please get it from the website.",
                "新版本需要手动下载，请到官网下载。",
            ),
            UpdateError::StaleManifest => t(
                "The update server returned older information than before, so it was ignored. Try again later.",
                "更新服务器返回的信息比之前看到的还旧，已忽略。请稍后再试。",
            ),
            UpdateError::Checksum | UpdateError::SizeMismatch => t(
                "The downloaded file is damaged or was changed, so it wasn't installed.",
                "下载的文件已损坏或被改动，没有安装。",
            ),
            UpdateError::Locked => t(
                "Another update is already running (in another Mist window or the mist command).",
                "已有另一个更新在进行（可能在别的 Mist 窗口或 mist 命令里）。",
            ),
            UpdateError::NotWritable(dir) => {
                if zh {
                    format!(
                        "没有权限写入 {}。请用有权限的账号运行 `mist update`（例如 `sudo mist update`），或手动下载安装。",
                        dir.display()
                    )
                } else {
                    format!(
                        "No permission to write to {}. Run `mist update` with an account that can (e.g. `sudo mist update`), or download it manually.",
                        dir.display()
                    )
                }
            }
            UpdateError::Cancelled => t("Update cancelled.", "已取消更新。"),
            UpdateError::SmokeTest(detail) => {
                if zh {
                    format!("新版本没能正常启动，已保留当前版本。（{detail}）")
                } else {
                    format!("The new version failed to start, so the current version was kept. ({detail})")
                }
            }
            UpdateError::NotAutoInstallable(detail) => {
                if zh {
                    format!("这个安装方式不能自动更新：{detail}")
                } else {
                    format!("This installation can't be updated automatically: {detail}")
                }
            }
            UpdateError::NoBackup => t(
                "There is no previous version saved, so there's nothing to roll back to.",
                "没有保存上一个版本，无法回退。",
            ),
            UpdateError::Install(detail) => {
                if zh {
                    format!("安装失败，当前版本没有改动。（{detail}）")
                } else {
                    format!("Install failed; the current version was left unchanged. ({detail})")
                }
            }
        }
    }
}
