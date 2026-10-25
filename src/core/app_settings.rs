//! 应用级设置（Vault、审计等），与 egui 窗口几何持久化分离。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::core::ai_settings::AiSettings;
use crate::core::audit::AuditSettings;
use crate::core::team::TeamSettings;
use crate::core::vault::VaultSettings;
use crate::i18n::UiLanguage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// UI language (default English).
    #[serde(default)]
    pub ui_language: UiLanguage,
    #[serde(default)]
    pub vault: VaultSettings,
    #[serde(default)]
    pub audit: AuditSettings,
    #[serde(default)]
    pub ai: AiSettings,
    #[serde(default)]
    pub team: TeamSettings,
    /// 自动更新偏好（默认：自动检查开、后台下载开）。
    #[serde(default)]
    pub update: crate::core::updater::UpdateSettings,
    /// 首次启动已自动展示「新人上手」帮助页（只弹一次）。
    #[serde(default)]
    pub onboarding_help_shown: bool,
    /// 团队多机「确认并执行」走服务端「自动执行」（Plan/Lease/Run），默认开启。
    #[serde(default = "default_true")]
    pub control_plane_plans: bool,
    /// 默认本地目录：SFTP 本机侧初始目录与 ZMODEM `sz` 保存目录共用；空 = 系统「下载」目录。
    #[serde(default)]
    pub default_local_dir: String,
    /// 用户显式禁用的内置模块列表（#118）。
    /// 注意：若环境变量 `MISTTERM_DISABLE_MODULES` 同时存在，env 中的模块强制禁用（优先级更高）。
    #[serde(default)]
    pub disabled_modules: Vec<String>,
}

fn default_true() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ui_language: UiLanguage::default(),
            vault: VaultSettings::default(),
            audit: AuditSettings::default(),
            ai: AiSettings::default(),
            team: TeamSettings::default(),
            update: crate::core::updater::UpdateSettings::default(),
            onboarding_help_shown: false,
            control_plane_plans: true,
            default_local_dir: String::new(),
            disabled_modules: Vec::new(),
        }
    }
}

impl AppSettings {
    pub fn default_path() -> PathBuf {
        let mut p = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        p.push("mistterm");
        p.push("settings.json");
        p
    }

    pub fn load() -> Self {
        let path = Self::default_path();
        let mut settings: Self = crate::security::encrypted_file::load_encrypted_json(&path);
        let mut changed = settings.ai.migrate_legacy_secrets();
        changed |= settings.update.migrate_auto_download_default_on();
        settings.team.lock_to_product_defaults();
        if changed {
            let _ = settings.save();
        }
        settings
    }

    pub fn save(&self) -> std::io::Result<()> {
        crate::security::encrypted_file::save_encrypted_json(&Self::default_path(), self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_without_local_dir_default_to_system_downloads() {
        let s: AppSettings = serde_json::from_str("{}").expect("parse");
        assert!(s.default_local_dir.is_empty());
    }
}
